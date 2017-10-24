//! KVCache interface for implementation of storage with possible cache : Route, QueryStore,
//! KVStore.
//!
use std::collections::HashMap;
use std::marker::PhantomData;
use std::hash::Hash;
use mydhtresult::{
  Result,
  Error,
  ErrorKind,
};
use std::cmp::min;
use rand::thread_rng;
use rand::OsRng;
use rand::Rng;
use bit_vec::BitVec;


/// cache base trait to use in storage (transient or persistant) relative implementations
/// TODO refacto : do not &K, K must be &Something
pub trait Cache<K, V> {
  /// Add value, pair is boolean for do persistent local store, and option for do cache value for
  /// CachePolicy duration TODO ref to key (key are clone)
  fn add_val_c(& mut self, K, V);
  /// Get value TODO ret ref
  fn get_val_c<'a>(&'a self, &K) -> Option<&'a V>;
  fn get_val_mut_c<'a>(&'a mut self, &K) -> Option<&'a mut V>;
  fn has_val_c<'a>(&'a self, k : &K) -> bool {
    self.get_val_c(k).is_some()
  }

  fn len_c (& self) -> usize;
  /// Remove value
  fn remove_val_c(& mut self, &K) -> Option<V>;
}

/// usize key cache with key creation on insertion
pub trait SlabCache<E> {
  fn insert(&mut self, val: E) -> usize;
  fn remove(&mut self, k : usize) -> Option<E>;
  fn get(&self, k : usize) -> Option<&E>;
  fn get_mut(&mut self, k : usize) -> Option<&mut E>;
  fn has(&self, k : usize) -> bool;
}
pub trait RandCache<K, V : Clone> : KVCache<K, V> {
  /// default impl, pretty inefficient, please use specialized implementation when needed
  /// (currently this is use for tunnel creation in mydht-tunnel crate).
  /// Here a big array is use each time, this should be stored in the cache implementation and not
  /// instantiated each time. Same with osrng.
  fn exact_rand(&mut self, min_no_repeat : usize, nb_tot : usize) -> Result<Vec<V>> {
    let c_len = self.len_c();
    if c_len < min_no_repeat {
      return Err(Error("Unchecked call to cache exact_rand, insufficient value".to_string(), ErrorKind::Bug, None));
    }
    let nb_ret = min(nb_tot,c_len);
    let mult_nb_ret = (nb_ret / min_no_repeat) * min_no_repeat;
    let mut positions : Vec<usize> = vec![0;mult_nb_ret];
    let mut positions2 : Vec<usize> = (0..c_len).collect();
    let mut result : Vec<Option<V>> = Vec::with_capacity(mult_nb_ret);
    for _ in 0..mult_nb_ret {
      result.push(None);
    }
    let mut rng = OsRng::new().unwrap();
    let mut tot_nb_done = 0;
    while tot_nb_done < mult_nb_ret {
      let mut nb_done = 0;
      while nb_done < min_no_repeat {
        let end = c_len - nb_done;
        let pos = rng.next_u64() as usize % end;
        positions[tot_nb_done] = positions2[pos];
        positions2[pos] = positions2[end - 1];
        positions2[end - 1] = positions[tot_nb_done];
        nb_done += 1;
        tot_nb_done += 1;
      }
    }
    // so costy
    self.strict_fold_c((&mut result,&positions,0), |(result, positions,i), (_,v)|{
      for j in 0..mult_nb_ret {
        if positions[j] == i {
          result[j] = Some((*v).clone());
        }
      }
      (result, positions, i + 1)
    });
 
    Ok(result.into_iter().map(|v|v.unwrap()).collect())

  }

}

/// cache base trait to use in storage (transient or persistant) relative implementations
/// TODO simplify after refact (delete??)
pub trait KVCache<K, V> : Sized + Cache<K,V> {
  /// update value, possibly inplace (depending upon impl (some might just get value modify it and
  /// set it again)), return true if update effective
  fn update_val_c<F>(& mut self, &K, f : F) -> Result<bool> where F : FnOnce(& mut V) -> Result<()>;
 
  /// fold without closure over all content
  fn strict_fold_c<'a, B, F>(&'a self, init: B, f: F) -> B where F: Fn(B, (&'a K, &'a V)) -> B, K : 'a, V : 'a;
  /// very permissive fold (allowing closure)
  fn fold_c<'a, B, F>(&'a self, init: B, f: F) -> B where F: FnMut(B, (&'a K, &'a V)) -> B, K : 'a, V : 'a;

  // TODO lightweigh map (more lightweight than  fold (no fn mut), find name where result is a
  // Result () not a vec + default impl in trem of fold_c TODO a result for fail computation : work
  // in Mdht result
  /// map allowing closure, stop on first error, depending on sematic error may be used to end
  /// computation (fold with early return).
  fn map_inplace_c<'a,F>(&'a self, mut f: F) -> Result<()> where F: FnMut((&'a K, &'a V)) -> Result<()>, K : 'a, V : 'a {
    self.fold_c(Ok(()),|err,kv|{if err.is_ok() {f(kv)} else {err}})
  }

  fn len_where_c<F> (& self, f : &F) -> usize where F : Fn(&V) -> bool {
    self.fold_c(0, |nb, (_,v)|{
      if f(v) {
        nb + 1
      } else {
        nb
      }
    })
  }
//  fn it_next<'b>(&'b mut Self::I) -> Option<(&'b K,&'b V)>;
  //fn iter_c<'a, I>(&'a self) -> I where I : Debug;
  // TODO see if we can remove that
  fn new() -> Self;


  fn distrib_ratio(&self) -> (usize,usize) {
    (2,3)
  }




  /// Return n random value from cache (should be used in routes using cache).
  ///
  /// Implementation should take care on returning less result if ratio of content in cache and
  /// resulting content does not allow a good random distribution (default implementation uses 0.66
  /// ratio (distrib ratio fn)).
  ///
  /// Warning default implementation is very inefficient (get size of cache and random depending
  /// on it on each values). Cache implementation should use a secondary cache with such values.
  /// Plus non optimized vec instantiation (buffer should be internal to cache).
  /// Note that without enough value in cache (need at least two time more value than expected nb
  /// result). And result is feed through a costy iteration.
  fn next_random_values<'a,F>(&'a mut self, queried_nb : usize, f : Option<&F>) -> Vec<&'a V> where K : 'a,
  F : Fn(&V) -> bool,
  {
    
    let l = match f {
      Some(fil) => self.len_where_c(fil),
      None => self.len_c(),
    };
    if l == 0 || queried_nb == 0 { 
      return Vec::new();
    }


    let rat = self.distrib_ratio();
    let randrat = l * rat.0 / rat.1;
    let nb = if queried_nb > randrat {
      //println!("applying randrat");
      if randrat == 0 {
        // round up
        1
      } else {
        // round down
        randrat
      }
    } else {
      queried_nb
    };
    
    let xess = {
      let m = l % 8;
      if m == 0 {
        0
      } else {
        8 - m
      }
    };
    let mut v = if xess == 0 {
      vec![0; l / 8]
    } else {
      vec![0; (l / 8) + 1]
    };
    loop {
      thread_rng().fill_bytes(&mut v);
      match inner_cache_bv_rand(self,&v, xess, nb, f) {
        Some(res) => {
          if res.len() == nb {
            return res;
          }
        },
        None => {
     warn!("Rerolling random (not enough results)");
        },
      }
    };
  }
}

#[inline]
/// c : a cache containing value to return reference on
/// buf : a buf of random generated byte, on bit corresponding to one cache element 
/// xess : number of unused bit (maximum 7)
/// nb : number of values to return
/// filtfn : a filter over random values (reduce the ration even more), plus an iter over
/// collection to count
///
fn inner_cache_bv_rand<'a,K, V, C : KVCache<K,V>, F : Fn(&V) -> bool> (c : &'a C, buf : &[u8], xess : usize, nb : usize, filtfn : Option<&F>) -> Option<Vec<&'a V>>  where K : 'a {
 
  let filter = {
    let mut r = BitVec::from_bytes(buf);
    for i in 0..xess {
      r.set(i,false);
    }
    r
  };

  let fsize = match filtfn {
    Some(f) => c.strict_fold_c((xess,0), |(i, mut tot), (_,v)|{
       if f(v) {
         if filter[i] {
           tot = tot + 1;
         }
         (i + 1, tot)
       } else {
         (i, tot)
       }
     }).1,
    None => filter.iter().filter(|x| *x).count(),
  };

  let ratio : usize = fsize / nb;
  //println!("TODO rem : ratio : {}, fsize : {}, nb : {}, xess : {}", ratio, fsize, nb,xess);
   if ratio == 0 {
     None
   } else {
     Some(c.strict_fold_c((xess,0,Vec::with_capacity(nb)), |(i, mut tot, mut r), (_,v)|{
       if filtfn.is_none() || (filtfn.unwrap())(v) {
         if filter[i] {
           if tot < nb && tot % ratio == 0 {
             r.push(v);
           }
           tot = tot + 1;
         }
         (i + 1, tot, r)
       } else {
         (i, tot, r)
       }
     }).2)
   }
}
 
pub struct NoCache<K,V>(PhantomData<(K,V)>);

impl<K,V> Cache<K,V> for NoCache<K,V> {
  fn len_c (& self) -> usize {
    0
  }

  //type I = ();
  fn add_val_c(& mut self, _ : K, _ : V) {
    ()
  }
  fn get_val_c<'a>(&'a self, _ : &K) -> Option<&'a V> {
    None
  }
  fn get_val_mut_c<'a>(&'a mut self, _ : &K) -> Option<&'a mut V> {
    None
  }
  fn remove_val_c(&mut self, _ : &K) -> Option<V> {
    None
  }

}
impl<K,V> KVCache<K,V> for NoCache<K,V> {
  fn update_val_c<F>(&mut self, _ : &K, _ : F) -> Result<bool> where F : FnOnce(&mut V) -> Result<()> {
    Ok(false)
  }
  fn strict_fold_c<'a, B, F>(&'a self, init: B, _: F) -> B where F: Fn(B, (&'a K, &'a V)) -> B, K : 'a, V : 'a {
    init
  }
  fn fold_c<'a, B, F>(&'a self, init: B, _ : F) -> B where F: FnMut(B, (&'a K, &'a V)) -> B, K : 'a, V : 'a {
    init
  }
/*  fn it_next<'b>(_ : &'b mut Self::I) -> Option<(&'b K,&'b V)> {
    None
  }*/
  fn new() -> Self {
    NoCache(PhantomData)
  }
}

impl<K,V : Clone> RandCache<K,V> for NoCache<K,V> { }
impl<K: Hash + Eq, V> Cache<K,V> for HashMap<K,V> {
  fn add_val_c(& mut self, key : K, val : V) {
    self.insert(key, val);
  }
  
  fn get_val_c<'a>(&'a self, key : &K) -> Option<&'a V> {
    self.get(key)
  }
  fn get_val_mut_c<'a>(&'a mut self, key : &K) -> Option<&'a mut V> {
    self.get_mut(key)
  }


  fn has_val_c<'a>(&'a self, key : &K) -> bool {
    self.contains_key(key)
  }

  fn remove_val_c(& mut self, key : &K) -> Option<V> {
    self.remove(key)
  }
  fn len_c (& self) -> usize {
    self.len()
  }

}

impl<K: Hash + Eq, V> KVCache<K,V> for HashMap<K,V> {
  fn new() -> Self {
    HashMap::new()
  }
  fn update_val_c<F>(&mut self, k : &K, f : F) -> Result<bool> where F : FnOnce(&mut V) -> Result<()> {
    if let Some(x) = self.get_mut(k) {
      try!(f(x));
      Ok(true)
    } else {
      Ok(false)
    }
  }

  fn strict_fold_c<'a, B, F>(&'a self, init: B, f: F) -> B where F: Fn(B, (&'a K, &'a V)) -> B, K : 'a, V : 'a {
    let mut res = init;
    for kv in self.iter(){
      res = f(res,kv);
    };
    res
  }
  fn fold_c<'a, B, F>(&'a self, init: B, mut f: F) -> B where F: FnMut(B, (&'a K, &'a V)) -> B, K : 'a, V : 'a  {
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

impl<K: Hash + Eq, V : Clone> RandCache<K,V> for HashMap<K,V> { }

#[test]
/// test of default random
fn test_rand_generic () {
  let t_none : Option <&(fn(&bool) -> bool)> = None;
  let filter_in = |b : &bool| *b;
  let filter = Some(&filter_in);
  let mut m : HashMap<usize, bool> = HashMap::new();
  assert!(0 == m.next_random_values(1,t_none).len());
  m.insert(1,true);
  assert!(1 == m.next_random_values(1,t_none).len());
  m.insert(2,true);
  assert!(1 == m.next_random_values(1,t_none).len());
  assert!(1 == m.next_random_values(2,t_none).len());
  m.insert(11,false);
  m.insert(12,false);
  assert!(2 == m.next_random_values(2,t_none).len());
  assert!(1 == m.next_random_values(2,filter).len());
  m.insert(3,true);
  assert!(1 == m.next_random_values(1,filter).len());
  assert!(2 == m.next_random_values(2,filter).len());
  // fill exactly 8 val
  for i in 4..9 {
    m.insert(i,true);
  }
  assert!(10 == m.len());
  assert!(1 == m.next_random_values(1,filter).len());
  assert!(5 == m.next_random_values(8,filter).len());
  m.insert(9,true);
  assert!(1 == m.next_random_values(1,filter).len());
  assert!(6 == m.next_random_values(8,filter).len());
}
#[test]
/// test of exact random
fn test_exact_rand () {
  let mut m : HashMap<usize, usize> = HashMap::new();
  assert!(m.exact_rand(1,1).is_err());
  m.insert(1,1);
  assert!(1 == m.exact_rand(1,1000).unwrap().len());
  m.insert(2,2);
  assert!(2 == m.exact_rand(1,2).unwrap().len());
  assert!(2 == m.exact_rand(2,2).unwrap().len());
  m.insert(3,11);
  m.insert(4,12);
  assert!(2 == m.exact_rand(2,3).unwrap().len());
  assert!(4 == m.exact_rand(2,4).unwrap().len());
  assert!(4 == m.exact_rand(4,5).unwrap().len());
  let r = m.exact_rand(4,5).unwrap();
  for i in 1..5 {
    for j in 1..5 {
      if i != j {
        assert!(m.get(&i).unwrap().clone() != *m.get(&j).unwrap());
      }
    }
  }
}

