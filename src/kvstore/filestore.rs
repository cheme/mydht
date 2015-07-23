use bincode;
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
use std::fs::PathExt;
use utils;
use keyval::{FileKeyVal,KeyVal};
use kvstore::KVStore;
use kvcache::{KVCache,NoCache};
use std::sync::Arc;
use query::cache::CachePolicy;
use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;

//type BoxedStore<V> = Box<KVStore<V, Cache = KVCache<K = <V as KeyVal>::Key, V = V> >>;
pub type BoxedStore<V> = Box<KVStore<V>>;
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
    let mut jsonCont = Vec::new();
    fpaths.read_to_end(&mut jsonCont);
    // if fail to load : just reset (currently only use to fasten init)
    let mut bpath : BTreeSet<PathBuf> = bincode::decode(&jsonCont[..]).unwrap_or(BTreeSet::new()); 

    // add ref to KVStore
    if (fillref) {
      info!("Filestore initiating from local path");
      for e in walk_dir(&rep).unwrap() {
        let p = e.unwrap().path();
        if (p.is_file() && !bpath.contains(&p)) {
         
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
        let r = hard_link(&path, &newpath);
        let r2 = match r {
          Err(_) => copy(&path, &newpath),
            _ => Ok(0),
        };
        let ur = remove_file(&path);
        let kv = <V as FileKeyVal>::from_path(newpath.to_path_buf()).unwrap();
        if(kv.get_key() == v.get_key()) {
          self.data.add_val(kv,(local,cp));
          self.paths.insert(newpath);
        } else {
          error!( "invalid FileKeyVal hash receive for {:?}", kv.name());
          remove_file(&newpath);
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
          remove_file(&path);
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
      OpenOptions::new().read(true).write(true).open(&confpath).map(|mut confFile|{
        let mut ior = true;
        ior && confFile.seek(SeekFrom::Start(0)).is_ok();
        // remove content
        ior && confFile.set_len(0).is_ok();
        info!("writing paths cache for filestore : {:?}", self.paths);
        // write new content
        ior && confFile.write_all(&bincode::encode(&self.paths, bincode::SizeLimit::Infinite).unwrap()[..]).is_ok();
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
  use super::super::{KVStore};
  use keyval::{FileKV,FileKeyVal,KeyVal};
  //use super::super::KVCache;
  use query::simplecache::SimpleCache;
  use std::collections::HashMap;
  use std::sync::Arc;
  use std::path::Path;
  use std::path::PathBuf;


  //#[test]
  fn init () {
    let ok = true;
    let mut ca : SimpleCache<FileKV,HashMap<<FileKV as KeyVal>::Key,FileKV>> = SimpleCache::new(None);
    // TODO set a path for serialize and redesign build fn to first check in simplecache
    // TODO mkdir in temp before + random size binary files in it -> util fn
    // + hash those plus get file after init by hash on ca then get file and cmp path.
    // + hash those plus get file after init by hash on ca then get file and cmp path.
    let mut cab = Box::new(ca) ;
    let fs : FileStore<FileKV> = FileStore::new(PathBuf::from("fstore/files"), PathBuf::from("fstore/reverse.db"),cab, true, &mut |_|()).unwrap();
    assert!(ok, "Dummy test only initialized a filestore" );
  }
}
