
use bincode::rustc_serialize as bincode;
use bincode::SizeLimit;
use std::io::Read;
use std::io::Write;
use std::io::Seek;
use std::io::SeekFrom;
use std::fs::File;
use std::fs::OpenOptions;
//use rustc_serialize::json;
use std::io::Result as IoResult;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::fs::walk_dir;
use std::fs::remove_file;
use std::fs::hard_link;
use std::fs::copy;
use mydht_base::utils;
use mydht_base::keyval::{FileKeyVal,KeyVal};
use mydht_base::kvstore::KVStore;
//use kvcache::{KVCache,NoCache};
//use std::sync::Arc;
use mydht_base::kvstore::CachePolicy;
use std::collections::BTreeSet;
//use std::path::Path;
use std::path::PathBuf;
use mydht_base::kvstore::BoxedStore;


/// File system based storage, it is hash related and need a supporting keystore to run (to manage
/// hash - path association).
/// This is an example of a storage with attachment.
/// Attachment are stored as file in a dedicated directory or directly used from the filesystem.
pub struct FileStore<V : KeyVal> {
  // TODO originaly was &'a KVStore<V> but issue with lifetime when implementing KeyVal : retry
  // with new rustc versions
  /// underlying kvstore associating KeyVal with its File path.
  /// Note that underlining kvstore must store in mode local_without_attachment
  /// TODO remove box...
  data : BoxedStore<V>,
  // store info on files existing (redundant with data, use to avoid unnecessary init // TODO change to kvstore??
  paths : BTreeSet<PathBuf>, 
  // path for storage
  pstore : PathBuf,
  repo : PathBuf,
}

impl<V : FileKeyVal> FileStore<V> {
  /// constructor, with filestore path, pathstore serialization path, kvstore for hash, reinit of
  /// content from path and a function to run during init on FileKeyVal.
  pub fn new(rep : PathBuf, pstore : PathBuf, mut st : BoxedStore<V>, fillref : bool, initfn :&mut (FnMut(&V) -> ())) -> IoResult<FileStore<V>> 
  { 
    //    if (!rep.exists() | !rep.is_dir())
    if !rep.is_dir() {
      return Err(IoError::new (
        IoErrorKind::Other,
        "Filestore is not a directory",
      ))
    }

    let mut fpaths = try!(File::open(&pstore));
    let mut jsoncont = Vec::new();
    try!(fpaths.read_to_end(&mut jsoncont));
    // if fail to load : just reset (currently only use to fasten init)
    let mut bpath : BTreeSet<PathBuf> = bincode::decode(&jsoncont[..]).unwrap_or(BTreeSet::new()); 

    // add ref to KVStore
    if fillref {
      info!("Filestore initiating from local path");
      for e in walk_dir(&rep).unwrap() {
        let p = e.unwrap().path();
        if p.is_file() && !bpath.contains(&p) {
         
          info!("  initiating {:?}", p);
          let kv = <V as FileKeyVal>::from_path(p.to_path_buf());
          kv.map(|kv| {
            initfn(&kv);
            bpath.insert(p);
            st.add_val(kv, (true,None))
          });
        };
      }
    };
     
    Ok(FileStore{
      data : st,
      repo : rep,
      paths : bpath,
      pstore : pstore,
    })
  }
}

impl<V : FileKeyVal> KVStore<V> for FileStore<V> {
  // no cache use (usage of underlying store cache instead)
  //type Cache = NoCache<<V as KeyVal>::Key, V>;
  fn add_val(& mut self,  v : V, (local, cp) : (bool, Option<CachePolicy>)) {
    // check if in tmpdir (utils fn) and if in tmpdir move to filestore dir
    if local {
      let path = v.get_file_ref().clone();
      if utils::is_in_tmp_dir(&(*path)) {
        let newpath = self.repo.join(&v.name()[..]);
        debug!("Moving file {:?} into filestore : {:?}", &path, newpath);
        let r = match hard_link(&path, &newpath) {
          Err(_) => copy(&path, &newpath),
            _ => Ok(0),
        };
        match r {
          Err(e) => {
            error!("Cannot move keyval file to file store, keyval is not added : {:?}", e);
            return
          },
          _ => (),
        };
        match remove_file(&path) {
          Err(e) => {
            error!("Cannot remove old file, some temporary files may remain : {:?} , cause : {:?}", &path, e);
          },
          _ => (),
        };
        let kv = <V as FileKeyVal>::from_path(newpath.to_path_buf()).unwrap();
        if kv.get_key() == v.get_key() {
          self.data.add_val(kv,(local,cp));
          self.paths.insert(newpath);
        } else {
          error!( "invalid FileKeyVal hash receive for {:?}", kv.name());
          match remove_file(&newpath) {
            Err(e) => {
              error!("Cannot remove invalid file, some extra files in store : {:?}, error : {:?}", &newpath, e);
            },
            _ => (),
          };

        };
        // TODO do something on failure
      } else {
        // just update refs
        self.data.add_val(v,(local,cp));
        self.paths.insert(path);
      }
    }
  }

  fn get_val(& self, k : &V::Key) -> Option<V> {
    self.data.get_val(k)
  }
  fn has_val(& self, k : &V::Key) -> bool {
    self.data.has_val(k)
  }


  fn remove_val(& mut self, k : &V::Key) {
    let rem = match self.data.get_val(k) {
      Some(kv) => {
        kv.get_attachment().map(|path|{
          // remove file 
          match remove_file(&path) {
            Err(e) => {
              error!("Cannot remove file of removed keyval, some extra files in store : {:?}, cause : {:?}", &path, e);
            },
            _ => (),
          };

          self.paths.remove(path)
        });
        true
      },
      None => false,
    };
    if rem {
      self.data.remove_val(k);
    };
  }

 
  #[inline]
  fn commit_store(& mut self) -> bool {
    let confpath = &self.pstore;
    let bupath = confpath.with_extension("_bu");
    let r = if copy(confpath, &bupath).is_ok() {
      OpenOptions::new().read(true).write(true).open(&confpath).map(|mut conffile|{
        let mut ior = true;
        ior = ior && conffile.seek(SeekFrom::Start(0)).is_ok();
        // remove content
        ior = ior && conffile.set_len(0).is_ok();
        info!("writing paths cache for filestore : {:?}", self.paths);
        // write new content
        ior = ior && conffile.write_all(&bincode::encode(&self.paths, SizeLimit::Infinite).unwrap()[..]).is_ok();
        ior
      }).unwrap_or(false)
    } else {
      false
    };
    self.data.commit_store() && r
  }

}

#[cfg(test)]
mod test {
  use super::FileStore;
  use mydht_base::keyval::{KeyVal};
  use filekv::{FileKV};
  //use super::super::KVCache;
  use mydht_base::simplecache::SimpleCache;
  use std::collections::HashMap;
  use std::path::PathBuf;


  //#[test]
  fn init () {
    let ok = true;
    let ca : SimpleCache<FileKV,HashMap<<FileKV as KeyVal>::Key,FileKV>> = SimpleCache::new(None);
    // TODO set a path for serialize and redesign build fn to first check in simplecache
    // TODO mkdir in temp before + random size binary files in it -> util fn
    // + hash those plus get file after init by hash on ca then get file and cmp path.
    // + hash those plus get file after init by hash on ca then get file and cmp path.
    let cab = Box::new(ca) ;
    let fs : FileStore<FileKV> = FileStore::new(PathBuf::from("fstore/files"), PathBuf::from("fstore/reverse.db"),cab, true, &mut |_|()).unwrap();
    assert!(ok, "Dummy test only initialized a filestore : {:?}", fs.pstore );
  }
}
