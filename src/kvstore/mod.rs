use std::hash::Hash;
use procs::{ClientChanel};
use peer::{Peer,PeerPriority};
use rustc_serialize::{Encodable, Decodable, Encoder, Decoder};
use std::fmt;
use super::query::{Query, QueryConfMsg, QueryPriority};
use std::sync::{Arc,Condvar,Mutex};
use utils::OneResult;
use query::cache::CachePolicy;
use query::{LastSent};
use std::io::Write;
use std::io::Read;
use std::fs::File;
use rustc_serialize::hex::ToHex;
use super::utils;
use std::str;
use std::path::{Path, PathBuf};
use msgenc;
use utils::ArcKV;

pub mod filestore;

// TODO evolve to allow a transient cache before kvstore (more in kvstore impl but involve using
// arc<V>?? 
// TODO get rid of those Arc except for transient.
// Note we use hasher cause still unstable hash but need some genericity here - this is for storage
// of keyval (serializable ...)

/// Non serialize binary attached content.
pub type Attachment = PathBuf; // TODO change to Path !!! to allow copy ....

pub trait Key : Encodable + Decodable + fmt::Debug + Hash + Eq + Clone + Send + Sync + Ord + 'static{}

/// Note linked anywhere. Cache for `KeyVal` TODO retry later usage (currently type inference fails) cf KVStore2
pub trait KVCache<K, V> : Send + 'static {
  /// Add value, pair is boolean for do persistent local store, and option for do cache value for
  /// CachePolicy duration
  fn c_add_val(& mut self, K, V, (bool, Option<CachePolicy>));
  /// Get value
  fn c_get_val(& self, &K) -> Option<V>;
  /// Remove value
  fn c_remove_val(& mut self, &K);
}

/// Note linked anywhere. TODO retry associated trait with later compiler to see if still no found type in fstore test
pub trait KVCacheA : Send {
  type K;
  type KV;
  /// Add value, pair is boolean for do persistent local store, and option for do cache value for
  /// CachePolicy duration
  fn c_add_val(& mut self, Self::KV, (bool, Option<CachePolicy>));
  /// Get value
  fn c_get_val(& self, &Self::K) -> Option<Self::KV>;
  /// Remove value
  fn c_remove_val(& mut self, &Self::K);
}

/// Note linked anywhere. TODO retry associated trait with later compiler to see if still no found type in fstore test
pub trait KVStore2<V : KeyVal> : KVCache<V::Key, Arc<V>> {
  #[inline]
  fn add_val(& mut self, kv : Arc<V>, op : (bool, Option<CachePolicy>)){
    self.c_add_val(kv.get_key(), kv,op)
  }
 
  #[inline]
  fn get_val(& self, k : &V::Key) -> Option<Arc<V>>{
    self.c_get_val(k)
  }
  #[inline]
  fn remove_val(& mut self, k : &V::Key){
    self.remove_val(k)
  }
}

#[cfg(test)]
mod test {
  extern crate dht as odht;
  extern crate num;
  extern crate rand;
  use rustc_serialize as serialize;
  use kvstore::KeyVal;
  use kvstore::KVStore2;
  use query::simplecache::SimpleCache;
  use std::sync::{Arc};
  use peer::node::{Node,NodeID};
  use std::net::Ipv4Addr;
  use self::odht::Peer as DhtPeer;
  use self::num::{BigUint};
  use self::num::bigint::RandBigInt;
  use std::net::{ToSocketAddrs, SocketAddr};
  use std::io::Result as IoResult;
  use peer::Peer;
  use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};
  use std::fs::File;

  use std::str::FromStr;

  use kvstore::Attachment;

  // Testing only nodeK, with key different from id
  type NodeK2 = (Node,String);

  impl KeyVal for NodeK2 {
    type Key = String;
    fn get_key(&self) -> NodeID {
        self.1.clone()
    }
    nospecificencoding!(NodeK2);
    noattachment!();
  }

  impl Peer for NodeK2 {
    //type Address = <Node as Peer>::Address;
    #[inline]
    //fn to_address(&self) -> <Node as Peer>::Address {
    fn to_address(&self) -> SocketAddr {
        self.0.to_address()
    }
  }

} 

/// Storage for `KeyVal`
//pub trait KVStore<V : KeyVal> : KVCache<K=V::Key, KV=Arc<V>> {
//pub trait KVStore<V : KeyVal> : KVCache<V::Key, Arc<V>> {
pub trait KVStore<V : KeyVal> : Send + 'static {
  /// Add value, pair is boolean for do persistent local store, and option for do cache value for
  /// CachePolicy duration
  fn add_val(& mut self, V, (bool, Option<CachePolicy>));
  /*  #[inline]
  fn add_val(& mut self, kv : Arc<V>, op : (bool, Option<CachePolicy>)){
    self.c_add_val(kv.get_key(), kv,op)
  }*/
  /// Get value
  fn get_val(& self, &V::Key) -> Option<V>;
  /*  #[inline]
  fn get_val(& self, k : &V::Key) -> Option<Arc<V>>{
    self.c_get_val(k)
  }*/
  /// Remove value
  fn remove_val(& mut self, &V::Key);
  /*  #[inline]
  fn remove_val(& mut self, k : &V::Key){
    self.remove_val(k)
  }*/

  /// Do periodic time consuming action. Typically serialize (eg on kvmanager shutdown)
  fn commit_store(& mut self) -> bool;
}

/// kvstore with a cache
pub trait KVStoreCache<V : KeyVal> : Send + 'static {
  /// Add value, pair is boolean for do persistent local store, and option for do cache value for
  /// CachePolicy duration
  fn add_val_c(& mut self, ArcKV<V>, (bool, Option<CachePolicy>));
  /// Get value
  fn get_val_c(& self, &V::Key) -> Option<ArcKV<V>>;
  /// Remove value
  fn remove_val_c(& mut self, &V::Key);
  /// Do periodic time consuming action. Typically serialize (eg on kvmanager shutdown)
  fn commit_store_c(& mut self) -> bool;
}


/// A KVStore for keyval containing two key (a pair of key as keyval) and with request over one of
/// the key only. Typically this kind of store is easy to map over a relational table db.
pub trait KVStoreRel<K1 : Key, K2 : Key, V : KeyVal<Key=(K1,K2)>> : KVStore<V> {
  fn get_vals_from_left(& self, &K1) -> Vec<V>;
  fn get_vals_from_right(& self, &K2) -> Vec<V>;
}

// TODO retest this with new compiler version
pub trait KVStoreRel2<V : KeyVal<Key=(Self::K1,Self::K2)>> : KVStore<V> {
  type K1 : Key;
  type K2 : Key;
  fn get_vals_from_left(& self, &Self::K1) -> Vec<Arc<V>>;
  fn get_vals_from_right(& self, &Self::K2) -> Vec<Arc<V>>;
}
 

/* TODO this trait as non transient and previous one as transient (so it will be easier to combine
 * non transient with transient store (previous is kind of cache)
  pub trait KVStore<V : KeyVal> : Send {
  fn add_val(& mut self, &V, (bool, Option<CachePolicy>));
  fn get_val(& self, &V::Key) -> Option<&V>;
  fn remove_val(& mut self, &V::Key);
} */

/// Specialization of Keyval for FileStore
pub trait FileKeyVal : KeyVal {
  /// initiate from a file (usefull for loading)
  fn from_file(&mut File) -> Option<Self>;
  /// name of the file
  fn name(&self) -> String;
  /// get attachment
  fn get_file_ref(&self) -> &Attachment {
    self.get_attachment().unwrap()
  }
}

/// KeyVal is the basis for DHT content, a value with key.
pub trait KeyVal : Encodable + Decodable + fmt::Debug + Clone + Send + Sync + Eq + 'static {
  /// Key type of KeyVal
  type Key : Encodable + Decodable + fmt::Debug + Hash + Eq + Clone + Send + Sync + Ord + 'static; //aka key // Ord , Hash ... might be not mandatory but issue currently
  /// getter for key value
  fn get_key(&self) -> Self::Key; // TODO change it to return &Key (lot of useless clone in impls
  /// optional attachment
  fn get_attachment(&self) -> Option<&Attachment>;
  /// optional attachment
  fn set_attachment(& mut self, &Attachment) -> bool;
  /// default serialize encode of keyval is used at least to store localy distant without attachment,
  /// it could be specialize with the three next variant (or just call from).
  fn encode_dist_with_att<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error>;
  fn decode_dist_with_att<D:Decoder> (d : &mut D) -> Result<Self, D::Error>;
  fn encode_dist<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error>;
  fn decode_dist<D:Decoder> (d : &mut D) -> Result<Self, D::Error>;
  fn encode_loc_with_att<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error>;
  fn decode_loc_with_att<D:Decoder> (d : &mut D) -> Result<Self, D::Error>;
}

#[derive(RustcDecodable,RustcEncodable,Debug,Clone,Copy)]
/// Storage priority (closely related to rules implementation)
pub enum StoragePriority {
  /// local only
  Local, 
  /// depend on rules, but typically mean propagate low up to nb hop
  PropagateL(usize), 
  /// depend on rules, but typically mean propagate high up to nb hop
  PropagateH(usize),
  /// do not store but could cache
  Trensiant, 
  /// never store
  NoStore,
  /// allways store
  All,
}

#[derive(RustcDecodable,RustcEncodable,Debug,PartialEq,Eq,Hash,Clone)]
/// Possible implementation of a `FileKeyVal` : local info are stored with local file
/// reference
pub struct FileKV {
  /// key is hash of file
  hash : Vec<u8>,
  /// local path to file (not sent (see serialize implementation)).
  file : PathBuf,
  /// name of file
  name : String,
}

impl FileKV {
  /// New FileKV from path
  pub fn new(p : PathBuf) -> FileKV {
    let tmpf = &mut File::open(&p).unwrap();// TODO change type to manage error
    FileKV::from_file(tmpf).unwrap()
  }

  fn from_file(tmpf : &mut File) -> Option<FileKV> {
    // TODO choose hash lib
    //let hash = utils::hash_crypto(tmpf);
    let hasho = utils::hash_openssl(tmpf);
    //error!("{:?}", hash);
    //error!("{:?}", hash.to_hex());
    debug!("Hash of file : {:?}", hasho.to_hex());
    let fp = tmpf.path().unwrap().to_path_buf();
    Some(FileKV {
      hash : hasho,
      file : fp,
      name : tmpf.path().unwrap().file_name().unwrap().to_str().unwrap().to_string(),
    })
  }

  fn encode_distant<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
    s.emit_struct("FileKV",2, |s| {
      s.emit_struct_field("hash", 0, |s|{
        Encodable::encode(&self.hash, s)
      });
      s.emit_struct_field("name", 1, |s|{
        s.emit_str(&self.name[..])
      })
    })
  }

  fn decode_distant<D:Decoder> (d : &mut D) -> Result<FileKV, D::Error> {
    d.read_struct("nonlocaldesckv",2, |d| {
      let hash : Result<Vec<u8>, D::Error>= d.read_struct_field("hash", 0, |d|{
        Decodable::decode(d)
      });
      let name : Result<String, D::Error>= d.read_struct_field("name", 1, |d|{
        d.read_str()
      });
      name.and_then(move |n| hash.map (move |h| FileKV{hash : h, name : n, file : PathBuf::new()}))
    })
  }

  fn encode_with_file<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
    debug!("encode with file");
    s.emit_struct("filekv",2, |s| {
      s.emit_struct_field("hash", 0, |s|{
        Encodable::encode(&self.hash, s)
      });
      s.emit_struct_field("name", 1, |s|{
        s.emit_str(&self.name[..])
      })
    });
    match self.get_attachment(){
      Some(path) => {
        let mut f = File::open(path).unwrap();
        let mut v = Vec::new();
        f.read_to_end(&mut v);
        Encodable::encode(&v,s)// very unefficient : use only for small files (big memory print)
      },
      None => panic!("Trying to serialize filekv without file"),// raise error
    }
  }

  fn decode_with_file<D:Decoder> (d : &mut D) -> Result<FileKV, D::Error> {
    debug!("decode with file");
    d.read_struct("filekv",2, |d| {
      let hash : Result<Vec<u8>, D::Error>= d.read_struct_field("hash", 0, |d|{
        Decodable::decode(d)
      });
      let name : Result<String, D::Error>= d.read_struct_field("name", 1, |d|{
        d.read_str()
      });
      let filevec : Result<Vec<u8>, D::Error>= Decodable::decode(d);
      //write file to tmp
      let mut file = utils::create_tmp_file();
      file.write_all(&filevec.ok().unwrap()[..]);
      file.flush();
      let fp = file.path().unwrap().to_path_buf();

      debug!("File added to tmp {:?}", fp);
      name.and_then(move |n| hash.map (move |h| FileKV{hash : h, name : n, file : fp}))
    })
  }
}

impl KeyVal for FileKV {
  type Key = Vec<u8>;
  fn get_key(&self) -> Vec<u8> {
    self.hash.clone()
  }
  #[inline]
  /// encode without path and attachment is serialize in encoded content
  fn encode_dist_with_att<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error>{
    self.encode_with_file(s)
  }
  #[inline]
  fn decode_dist_with_att<D:Decoder> (d : &mut D) -> Result<FileKV, D::Error>{
    FileKV::decode_with_file(d)
  }
  #[inline]
   /// encode without path and attachment is to be sent as attachment (not included)
  fn encode_dist<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error>{
    self.encode_distant(s)
  }
  #[inline]
  fn decode_dist<D:Decoder> (d : &mut D) -> Result<FileKV, D::Error>{
    FileKV::decode_distant(d)
  }
  #[inline]
  /// encode with attachement in encoded content to be use with non filestore storage
  /// (default serialize include path and no attachment for use with filestore)
  fn encode_loc_with_att<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error>{
    self.encode_with_file(s)
  }
  #[inline]
  fn decode_loc_with_att<D:Decoder> (d : &mut D) -> Result<FileKV, D::Error>{
    FileKV::decode_with_file(d)
  }
  #[inline]
  /// attachment support as file (in tmp or in filestore)
  fn get_attachment(&self) -> Option<&Attachment>{
    Some(&self.file)
  }
  #[inline]
  /// attachment support as file (in tmp or in filestore)
  fn set_attachment(& mut self, f:&Attachment) -> bool{
    self.file = f.clone();
    true
  }
}


impl FileKeyVal for FileKV {
  fn name(&self) -> String {
    self.name.clone()
  }

  #[inline]
  fn from_file(tmpf : &mut File) -> Option<FileKV> {
    FileKV::from_file(tmpf)
  }
}



