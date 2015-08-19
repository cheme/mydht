//! KVCache interface for implementation of storage with possible cache : Route, QueryStore,
//! KVStore.
//!
use std::collections::HashMap;
use std::marker::PhantomData;
use std::hash::Hash;
use mydhtresult::Result;

/// cache base trait to use in storage (transient or persistant) relative implementations
pub trait KVCache<K, V> {
  /// Add value, pair is boolean for do persistent local store, and option for do cache value for
  /// CachePolicy duration TODO ref to key (key are clone)
  fn add_val_c(& mut self, K, V);
  /// Get value TODO ret ref
  fn get_val_c<'a>(&'a self, &K) -> Option<&'a V>;
  fn has_val_c<'a>(&'a self, k : &K) -> bool {
    self.get_val_c(k).is_some()
  }
  /// update value, possibly inplace (depending upon impl (some might just get value modify it and
  /// set it again)), return true if update effective
  fn update_val_c<'a, F>(&'a mut self, &K, f : F) -> Result<bool> where F : FnOnce(&'a mut V) -> Result<()>;
 
  /// Remove value
  fn remove_val_c(& mut self, &K);

  /// fold without closure over all content
  fn strict_fold_c<'a, B, F>(&'a self, init: B, f: &F) -> B where F: Fn(B, (&'a K, &'a V)) -> B;
  /// very permissive fold (allowing closure)
  fn fold_c<'a, B, F>(&'a self, init: B, mut f: F) -> B where F: FnMut(B, (&'a K, &'a V)) -> B;

  // TODO lightweigh map (more lightweight than  fold (no fn mut), find name where result is a
  // Result () not a vec + default impl in trem of fold_c TODO a result for fail computation : work
  // in Mdht result
  /// map allowing closure, stop on first error, depending on sematic error may be used to end
  /// computation (fold with early return).
  fn map_inplace_c<'a,F>(&'a self, mut f: F) -> Result<()> where F: FnMut((&'a K, &'a V)) -> Result<()> {
    self.fold_c(Ok(()),|err,kv|{if err.is_ok() {f(kv)} else {err}})
  }

  fn len_c (& self) -> usize;
//  fn it_next<'b>(&'b mut Self::I) -> Option<(&'b K,&'b V)>;
  //fn iter_c<'a, I>(&'a self) -> I where I : Debug;
  fn new() -> Self;
}



pub struct NoCache<K, V>(PhantomData<(K, V)>);

impl<K , V > KVCache<K,V> for NoCache<K,V> {
  //type I = ();
  fn add_val_c(& mut self, _ : K, _ : V) {
    ()
  }
  fn get_val_c<'a>(&'a self, _ : &K) -> Option<&'a V> {
    None
  }
  fn update_val_c<'a, F>(&'a mut self, _ : &K, _ : F) -> Result<bool> where F : FnOnce(&'a mut V) -> Result<()> {
    Ok(false)
  }
  fn remove_val_c(& mut self, _ : &K) {
    ()
  }
  fn strict_fold_c<'a, B, F>(&'a self, init: B, _: &F) -> B where F: Fn(B, (&'a K, &'a V)) -> B {
    init
  }
  fn fold_c<'a, B, F>(&'a self, init: B, _ : F) -> B where F: FnMut(B, (&'a K, &'a V)) -> B {
    init
  }
  fn len_c (& self) -> usize {
    0
  }
/*  fn it_next<'b>(_ : &'b mut Self::I) -> Option<(&'b K,&'b V)> {
    None
  }*/
  fn new() -> Self {
    NoCache::new()
  }
}

impl<K: Hash + Eq, V> KVCache<K,V> for HashMap<K,V> {
  fn add_val_c(& mut self, key : K, val : V) {
    self.insert(key, val);
  }
  
  fn get_val_c<'a>(&'a self, key : &K) -> Option<&'a V> {
    self.get(key)
  }

  fn has_val_c<'a>(&'a self, key : &K) -> bool {
    self.contains_key(key)
  }

  fn update_val_c<'a, F>(&'a mut self, k : &K, f : F) -> Result<bool> where F : FnOnce(&'a mut V) -> Result<()> {
    if let Some(x) = self.get_mut(k) {
      try!(f(x));
      Ok(true)
    } else {
      Ok(false)
    }
  }
  fn remove_val_c(& mut self, key : &K) {
    self.remove(key);
  }

  fn new() -> Self {
    HashMap::new()
  }

  fn len_c (& self) -> usize {
    self.len()
  }

  fn strict_fold_c<'a, B, F>(&'a self, init: B, f: &F) -> B where F: Fn(B, (&'a K, &'a V)) -> B {
    let mut res = init;
    for kv in self.iter(){
      res = f(res,kv);
    };
    res
  }
  fn fold_c<'a, B, F>(&'a self, init: B, mut f: F) -> B where F: FnMut(B, (&'a K, &'a V)) -> B {
    let mut res = init;
    for kv in self.iter(){
      res = f(res,kv);
    };
    res
  }
  fn map_inplace_c<'a,F>(&'a self, mut f: F) -> Result<()> where F: FnMut((&'a K, &'a V)) -> Result<()> {
    for kv in self.iter(){
      try!(f(kv));
    };
    Ok(())
  }


}
/*
fn type_test<K, V, C : KVCache<K,V>>(ca : & mut C, k : K, v : V) {
  ca.get_val_c(&k);
  ca.add_val_c(k,v);
}*/
