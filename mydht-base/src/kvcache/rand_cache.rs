use super::{
  KVCache,
  Cache,
  inner_cache_bv_rand,
};
use mydhtresult::Result;
use std::marker::PhantomData;
use rand::thread_rng;
use rand::Rng;

#[cfg(test)]
use std::collections::HashMap;

pub enum RandCacheMode {
  /// cache random key until we need more than what is cached
  Keys,
  /// cached only random results
  Rand,
}

/// automatic implementation of random for cache by composition
/// TODO Vec or VecDeque of rand key from one iter : could be use for multiple random (until not
/// enough element in it). The vec need also some form of shuffle at init.
pub struct RandCache<K, V, C : KVCache<K,V>> {
  cache : C,

  // cache to limit number of call to rand
  randcache : Vec<u8>,

  randcachepos : usize,
  numratio : usize,
  denratio : usize,
  mode : RandCacheMode,


  _phdat :  PhantomData<(K,V)>,
}

const DEF_RANDCACHE_SIZE : usize = 128;

impl<K, V, C : KVCache<K,V>> RandCache<K, V, C> {
  fn new (c : C) -> RandCache<K,V,C> {
    Self::new_add(c, DEF_RANDCACHE_SIZE)
  }
  fn new_add (c : C, rcachebytelength : usize) -> RandCache<K,V,C> {
    let mut r = vec!(0;rcachebytelength);
    thread_rng().fill_bytes(&mut r);
    RandCache {
      cache : c,
      randcache : r,
      randcachepos : 0,
      numratio : 2,
      denratio : 3,
      mode : RandCacheMode::Rand,
      _phdat : PhantomData,
    } 
  }
}

impl<K,V, C : KVCache<K,V>> Cache<K, V> for RandCache<K,V,C> {

  fn add_val_c(& mut self, k: K, v: V) {
    self.cache.add_val_c(k,v)
  }
  fn get_val_c<'a>(&'a self, k : &K) -> Option<&'a V> {
    self.cache.get_val_c(k)
  }
  fn has_val_c<'a>(&'a self, k : &K) -> bool {
    self.cache.has_val_c(k)
  }
  fn remove_val_c(& mut self, k : &K) -> Option<V> {
    self.cache.remove_val_c(k)
  }

}
impl<K,V, C : KVCache<K,V>> KVCache<K, V> for RandCache<K,V,C> {
  fn update_val_c<F>(& mut self, k : &K, f : F) -> Result<bool> where F : FnOnce(& mut V) -> Result<()> {
    self.cache.update_val_c(k,f)
  }

  fn strict_fold_c<'a, B, F>(&'a self, init: B, f: F) -> B where F: Fn(B, (&'a K, &'a V)) -> B, K : 'a, V : 'a {
    self.cache.strict_fold_c(init,f)
  }
  fn fold_c<'a, B, F>(&'a self, init: B, f: F) -> B where F: FnMut(B, (&'a K, &'a V)) -> B, K : 'a, V : 'a {
    self.cache.fold_c(init,f)
  }
  fn map_inplace_c<'a,F>(&'a self, f: F) -> Result<()> where F: FnMut((&'a K, &'a V)) -> Result<()>, K : 'a, V : 'a {
    self.cache.map_inplace_c(f)
  }
  fn len_c (& self) -> usize {
    self.cache.len_c()
  }
  fn new() -> Self {
    RandCache::new(
      C::new()
    )
  }
  fn distrib_ratio(&self) -> (usize,usize) {
    (self.numratio, self.denratio)
  }

  fn next_random_values<'a,F>(&'a mut self, queried_nb : usize, filter : Option<&F>) -> Vec<&'a V> where K : 'a, F : Fn(&V) -> bool {
    let l = match filter {
      Some(fil) => self.len_where_c(fil),
      None => self.len_c(),
    };
 
    let rat = self.distrib_ratio();
    let randrat = l * rat.0 / rat.1;
    let nb = if queried_nb > randrat {
      //println!("applying randrat2 : l {}", l);
      randrat
    } else {
      queried_nb
    };
    
    if nb == 0 {
      return Vec::new();
    }

    let xess = {
      let m = l % 8;
      if m == 0 {
        0
      } else {
        8 - m
      }
    };
    let vlen = if xess == 0 {
      l / 8
    } else {
      (l / 8) + 1
    };
    loop {
      let v = if self.randcachepos + vlen < self.randcache.len() {
        self.randcachepos += vlen;
        &self.randcache[self.randcachepos - vlen .. self.randcachepos]
      } else {
        if vlen > self.randcache.len() {
          // update size
          self.randcache = vec!(0;vlen);
        }
        thread_rng().fill_bytes(&mut self.randcache);
        self.randcachepos = vlen;
        &self.randcache[.. vlen]
      };
      match inner_cache_bv_rand(&self.cache,&v, xess, nb, filter) {
        Some(res) => return res,
        None => {
          warn!("Rerolling random (not enough results)");
          warn!("Rerolling random (not enough results)");
        },
      }
    };
  }
}

#[test]
fn test_rand_generic_ca () {
  let t_none : Option <&(fn(&bool) -> bool)> = None;
  let filter_in = |b : &bool| *b;
  let filter = Some(&filter_in);
  let h : HashMap<usize, bool> = HashMap::new();
  let mut m = 
    RandCache::new_add (h, 3);
 
  assert!(0 == m.next_random_values(1,t_none).len());
  m.cache.insert(1,true);
  assert!(0 == m.next_random_values(1,t_none).len());
  m.cache.insert(2,true);
  assert!(1 == m.next_random_values(1,t_none).len());
  assert!(1 == m.next_random_values(2,t_none).len());
  m.cache.insert(11,false);
  m.cache.insert(12,false);
  assert!(2 == m.next_random_values(2,t_none).len());
  assert!(1 == m.next_random_values(2,filter).len());
  m.cache.insert(3,true);
  assert!(1 == m.next_random_values(1,filter).len());
  assert!(2 == m.next_random_values(2,filter).len());
  // fill exactly 8 val
  for i in 4..9 {
    m.cache.insert(i,true);
  }
  assert!(10 == m.cache.len());
  assert!(1 == m.next_random_values(1,filter).len());
  assert!(5 == m.next_random_values(8,filter).len());
  m.cache.insert(9,true);
  assert!(1 == m.next_random_values(1,filter).len());
  assert!(6 == m.next_random_values(8,filter).len());
  //assert!(false);
}


/*
pub trait DerRandCache<K, V, C : KVCache<K,V>> {
  fn der_get_mut_cache(&mut self) -> &mut C;
  fn der_get_cache(&self) -> &C;
  fn der_get_mut_randcache(&mut self) -> &mut Vec<u8>;
  fn der_get_randcache(& self) -> &Vec<u8>;
  fn der_get_mut_randcachepos(&mut self) -> &mut usize;
  // prim type return not a ref (exceptions)
  fn der_get_randcachepos(&mut self) -> usize;
  fn der_get_mut_numratio(&mut self) -> &mut usize;
  fn der_get_numratio(&mut self) -> usize;
  fn der_get_mut_denratio(&mut self) -> &mut usize;
  fn der_get_denratio(&mut self) -> &usize;
  // TODO remove new (or this for fn that could/should not be derive, opposed to fn that could be
  // written generically) TODO as another trait??
  fn der_new() -> Self;
}

impl<K,V, C : KVCache<K,V>, D : DerRandCache<K,V,C>> KVCache<K, V> for D  {
  fn add_val_c(& mut self, k: K, v: V) {
    self.der_get_mut_cache().add_val_c(k,v)
  }
  fn get_val_c<'a>(&'a self, k : &K) -> Option<&'a V> {
    self.der_get_cache().get_val_c(k)
  }
  fn has_val_c<'a>(&'a self, k : &K) -> bool {
    self.der_get_cache().has_val_c(k)
  }
  fn update_val_c<F>(& mut self, k : &K, f : F) -> Result<bool> where F : FnOnce(& mut V) -> Result<()> {
    self.der_get_mut_cache().update_val_c(k,f)
  }
  fn remove_val_c(& mut self, k : &K) -> Option<V> {
    self.der_get_mut_cache().remove_val_c(k)
  }
  fn strict_fold_c<'a, B, F>(&'a self, init: B, f: F) -> B where F: Fn(B, (&'a K, &'a V)) -> B, K : 'a, V : 'a {
    self.der_get_cache().strict_fold_c(init,f)
  }
  fn fold_c<'a, B, F>(&'a self, init: B, f: F) -> B where F: FnMut(B, (&'a K, &'a V)) -> B, K : 'a, V : 'a {
    self.der_get_cache().fold_c(init,f)
  }
  fn map_inplace_c<'a,F>(&'a self, f: F) -> Result<()> where F: FnMut((&'a K, &'a V)) -> Result<()>, K : 'a, V : 'a {
    self.der_get_cache().map_inplace_c(f)
  }
  fn len_c (& self) -> usize {
    self.der_get_cache().len_c()
  }
  // TODO rem new
  fn new() -> Self {
    D::der_new()
  }
  fn next_random_values<'a>(&'a mut self, queried_nb : usize) -> Vec<&'a V> where K : 'a {
    let l = self.len_c();
    let randrat = l * self.der_get_numratio() / self.der_get_denratio();
    let nb = if queried_nb > randrat {
      randrat
    } else {
      queried_nb
    };
    
    if nb == 0 {
      return Vec::new();
    }

    let xess = {
      let m = l % 8;
      if m == 0 {
        0
      } else {
        8 - m
      }
    };
    let vlen = if xess == 0 {
      l / 8
    } else {
      (l / 8) + 1
    };
    loop {
      let v = if self.der_get_randcachepos() + vlen < self.der_get_randcache().len() {
        (*self.der_get_mut_randcachepos()) += vlen;
        &self.der_get_randcache()[self.der_get_randcachepos() - vlen .. self.der_get_randcachepos()]
      } else {
        if vlen > self.der_get_randcache().len() {
          // update size
          self.randcache = vec!(0;vlen);
        }
        thread_rng().fill_bytes(self.der_get_mut_randcache());
        (*self.der_get_mut_randcachepos()) = vlen;
        &self.der_get_randcache()[.. vlen]
      };
      match inner_cache_bv_rand(self.der_get_cache(),&v, xess, nb) {
        Some(res) => return res,
        None => {
          warn!("Rerolling random (not enough results)");
        },
      }
    };
  }
}

*/
