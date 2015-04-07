extern crate bincode;
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
use super::super::utils;
use super::{FileKeyVal,KeyVal,KVStore};
//use super::KVCache;
use std::sync::Arc;
use query::cache::CachePolicy;
use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;

/// File system based storage, it is hash related and need a supporting keystore to run (to manage
/// hash - path association).
/// This is an example of a storage with attachment.
/// Attachment are stored as file in a dedicated directory or directly used from the filesystem.
pub struct FileStore<V : KeyVal> {
  // TODO originaly was &'a KVStore<V> but issue with lifetime when implementing KeyVal : retry
  // with new rustc versions
  /// underlying kvstore associating KeyVal with its File path.
  /// Note that underlining kvstore must store in mode local_without_attachment
  data : Box<KVStore<V>>,
  // store info on files existing (redundant with data, use to avoid unnecessary init // TODO change to kvstore??
  paths : BTreeSet<PathBuf>, 
  // path for storage
  pstore : File,
  repo : PathBuf,
}

impl<V : FileKeyVal> FileStore<V> {
  /// constructor, with filestore path, pathstore serialization path, kvstore for hash, reinit of
  /// content from path and a function to run during init on FileKeyVal.
  pub fn new(rep : PathBuf, pstore : PathBuf, mut st : Box<KVStore<V>>, fillref : bool, initfn :&mut (FnMut(&V) -> ())) -> IoResult<FileStore<V>> 
  { 
    //    if (!rep.exists() | !rep.is_dir())
    if !rep.is_dir() {
      return Err(IoError::new (
        IoErrorKind::Other,
        "Filestore is not a directory",
      ))
    }

    let mut fpaths = try!(OpenOptions::new().read(true).write(true).open(&pstore));
    let mut jsonCont = Vec::new();
    fpaths.read_to_end(&mut jsonCont);
    // if fail to load : just reset (currently only use to fasten init)
    let mut bpath : BTreeSet<PathBuf> = bincode::decode(jsonCont.as_slice()).unwrap_or(BTreeSet::new()); 

    // add ref to KVStore
    if (fillref) {
      info!("Filestore initiating from local path");
      for e in walk_dir(&rep).unwrap() {
        let p = e.unwrap().path();
        if (p.is_file() && !bpath.contains(&p)) {
          let mut tmpf = File::open(&p).unwrap();
         
          info!("  initiating {:?}", p);
          let kv = <V as FileKeyVal>::from_file(&mut tmpf);
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
      pstore : fpaths,
    })
  }
}

impl<V : FileKeyVal> KVStore<V> for FileStore<V> {
  fn add_val(& mut self,  v : V, (local, cp) : (bool, Option<CachePolicy>)) {
    // check if in tmpdir (utils fn) and if in tmpdir move to filestore dir
    if local {
      let path = v.get_file_ref().clone();
      if utils::is_in_tmp_dir(&(*path)) {
        let newpath = self.repo.join(v.name().as_slice());
        debug!("Moving file {:?} into filestore : {:?}", &path, newpath);
        let r = hard_link(&path, &newpath);
        let r2 = match r {
          Err(_) => copy(&path, &newpath),
            _ => Ok(0),
        };
        let ur = remove_file(&path);
        let mut newfile = File::open(&newpath).unwrap();
        let kv = <V as FileKeyVal>::from_file(&mut newfile).unwrap();
        if(kv.get_key() == v.get_key()) {
          self.data.add_val(kv,(local,cp));
          self.paths.insert(newpath);
        } else {
          error!( "invalid FileKeyVal hash receive for {:?}", kv.name());
          remove_file(newfile.path().unwrap());
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
  fn commit_store(& mut self) -> bool{
    let confFile = &mut self.pstore;
    let bupath = confFile.path().unwrap().with_extension("_bu");
    let r = if copy(confFile.path().unwrap(), &bupath).is_ok(){
      confFile.seek(SeekFrom::Start(0));
      // remove content
      confFile.set_len(0);
      info!("writing paths cache for filestore : {:?}", self.paths);
      // write new content
      confFile.write_all(bincode::encode(&self.paths, bincode::SizeLimit::Infinite).unwrap().as_slice()).is_ok()
    } else {
      false
    };
    self.data.commit_store() && r
  }

}

#[cfg(test)]
mod test {
  use super::FileStore;
  use super::super::{FileKV,KVStore,FileKeyVal,KeyVal};
  //use super::super::KVCache;
  use query::simplecache::SimpleCache;
  use std::sync::Arc;
  use std::path::Path;
  use std::path::PathBuf;


  //#[test]
  fn init () {
    let ok = true;
    let mut ca : SimpleCache<FileKV> = SimpleCache::new(None);
    // TODO set a path for serialize and redesign build fn to first check in simplecache
    // TODO mkdir in temp before + random size binary files in it -> util fn
    // + hash those plus get file after init by hash on ca then get file and cmp path.
    // + hash those plus get file after init by hash on ca then get file and cmp path.
    let mut cab = Box::new(ca) ;
    let fs : FileStore<FileKV> = FileStore::new(PathBuf::from("fstore/files"), PathBuf::from("fstore/reverse.db"),cab, true, &mut |_|()).unwrap();
    assert!(ok, "Dummy test only initialized a filestore" );
  }
}
