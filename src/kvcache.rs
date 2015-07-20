//! KVCache interface for implementation of storage with possible cache : Route, QueryStore,
//! KVStore.
//!
use mydhtresult::Result as MDHTResult;
use num::traits::ToPrimitive;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::hash::Hash;
use std::iter::Iterator;
use std::iter::IntoIterator;
use std::collections::hash_map::Iter as HMIter;
use std::fmt::Debug;

/// cache base trait to use in storage (transient or persistant) relative implementations
pub trait KVCache<'a,K, V> {
  type I : 'a;
  /// Add value, pair is boolean for do persistent local store, and option for do cache value for
  /// CachePolicy duration TODO ref to key (key are clone)
  fn add_val_c(& mut self, K, V);
  /// Get value
  fn get_val_c(& self, &K) -> Option<V>;
  /// Remove value
  fn remove_val_c(& mut self, &K);
  // TODO see if we can replace iter TODO use iterator 
  fn iter_c(&'a self) -> Self::I;
  fn it_next<'b>(&'b mut Self::I) -> Option<(&'b K,&'b V)>;
  //fn iter_c<'a, I>(&'a self) -> I where I : Debug;
  fn new() -> Self;
}
/*
impl<'a, K : 'a, V : 'a, I : Iterator<Item = (&'a K, &'a V)>> IntoIterator for &'a KVCache<K,V> {
  type Item = (&'a K, &'a V);
  type IntoIter = I;
  fn into_iter(self) -> Self::IntoIter {
    self.iter_c()
  }
} 
*/

pub struct NoCache<K, V>(PhantomData<(K, V)>);
//pub struct NoIter<'a, K , V >(PhantomData<(&'a(),K, V)>);
impl<K, V> NoCache<K,V> {
  pub fn new() -> NoCache<K,V> {
    NoCache(PhantomData)
  }
}
/*impl<'a,K, V> NoIter<'a,K,V> {
  pub fn new() -> NoIter<'a,K,V> {
    NoIter(PhantomData)
  }
}*/

/*
impl<'a, K, V> Iterator for NoIter<'a,K,V> {
  type Item = (&'a K, &'a V);
  fn next(&mut self) -> Option<Self::Item> {
    None
  }
}*/
impl<'a,K, V> KVCache<'a,K,V> for NoCache<K,V> {
  type I = ();
  fn add_val_c(& mut self, key : K, val : V) {
    ()
  }
  fn get_val_c(& self, key : &K) -> Option<V> {
    None
  }
  fn remove_val_c(& mut self, key : &K) {
    ()
  }

  fn iter_c(&'a self) -> Self::I {
    ()
  }
  fn it_next<'b>(_ : &'b mut Self::I) -> Option<(&'b K,&'b V)> {
    None
  }
  fn new() -> Self {
    NoCache::new()
  }
}

impl<'a, K: Hash + Eq + 'a, V : Clone + 'a> KVCache<'a,K,V> for HashMap<K,V> {
  type I = HMIter<'a, K,V>;
  fn add_val_c(& mut self, key : K, val : V) {
    self.insert(key, val);
  }
  // TODO return a ref : here constraint on clone is bad -> remove clone constraint
  fn get_val_c(& self, key : &K) -> Option<V> {
    self.get(key).cloned()

  }
  fn remove_val_c(& mut self, key : &K) {
    self.remove(key);
  }
  fn iter_c(&'a self) -> Self::I {
    self.iter()
  }
  fn it_next<'b>(it : &'b mut Self::I) -> Option<(&'b K, &'b V)> {
    it.next()
  }

  fn new() -> Self {
    HashMap::new()
  }

}

