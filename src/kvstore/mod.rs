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
use keyval::{KeyVal,Key};

pub mod filestore;

// TODO evolve to allow a transient cache before kvstore (more in kvstore impl but involve using
// arc<V>?? 
// TODO get rid of those Arc except for transient.
// Note we use hasher cause still unstable hash but need some genericity here - this is for storage
// of keyval (serializable ...)


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
  extern crate num;
  extern crate rand;
  use rustc_serialize as serialize;
  use keyval::KeyVal;
  use kvstore::KVStore2;
  use query::simplecache::SimpleCache;
  use std::sync::{Arc};
  use peer::node::{Node,NodeID};
  use std::net::Ipv4Addr;
  use self::num::{BigUint};
  use self::num::bigint::RandBigInt;
  use std::net::{ToSocketAddrs, SocketAddr};
  use std::io::Result as IoResult;
  use peer::Peer;
  use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};
  use std::fs::File;

  use std::str::FromStr;

  use keyval::Attachment;

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


