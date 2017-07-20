use rustc_serialize::{Encodable, Decodable, Encoder, Decoder};
use std::sync::{Arc};
use keyval::{KeyVal,Key};
use time::Timespec;


/// cache policies apply to queries to know how long they may cache before we consider current
/// result ok. Currently our cache policies are only an expiration date.
#[derive(Debug,Copy,Clone)]
pub struct CachePolicy(pub Timespec);

impl Decodable for CachePolicy {
    fn decode<D:Decoder> (d : &mut D) -> Result<CachePolicy, D::Error> {
        d.read_i64().and_then(|sec| d.read_i32().map(|nsec| CachePolicy(Timespec{sec : sec, nsec : nsec})))
    }
}

impl Encodable for CachePolicy {
    fn encode<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
        s.emit_i64(self.0.sec).and_then(|()| s.emit_i32(self.0.nsec))
    }
}



/// Storage for `KeyVal`
pub trait KVStore<V : KeyVal> {
  /// Add value, pair is boolean for do persistent local store, and option for do cache value for
  /// CachePolicy duration // TODO return MDHTResult<()>
  fn add_val(& mut self, V, (bool, Option<CachePolicy>));
  /*  #[inline]
  fn add_val(& mut self, kv : Arc<V>, op : (bool, Option<CachePolicy>)){
    self.c_add_val(kv.get_key(), kv,op)
  }*/
  /// Get value
  fn get_val(& self, &V::Key) -> Option<V>;
 
  fn has_val(& self, &V::Key) -> bool;
  /*  #[inline]
  fn get_val(& self, k : &V::Key) -> Option<Arc<V>>{
    self.c_get_val(k)
  }*/
  /// Remove value // TODO return MDHTResult
  fn remove_val(& mut self, &V::Key);
  /*  #[inline]
  fn remove_val(& mut self, k : &V::Key){
    self.remove_val(k)
  }*/

  /// Do periodic time consuming action. Typically serialize (eg on kvmanager shutdown) // TODO
  /// return MDHTResult<bool>
  fn commit_store(& mut self) -> bool;
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

/// A boxed store
pub type BoxedStore<V> = Box<KVStore<V>>;

/// A boxed store for relations
pub type BoxedStoreRel<K1,K2,V> = Box<KVStoreRel<K1,K2,V>>;

