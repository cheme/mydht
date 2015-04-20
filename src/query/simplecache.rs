use std::collections::{HashMap};
use std::hash::Hash;
use query::{self,QueryID, Query};
use query::cache::QueryCache;
use std::time::Duration;
use peer::Peer;
use time;
use kvstore::{KeyVal,KVStore,KVStoreRel, Key};
use kvstore::{KVStoreCache,KVCache,KVStore2};
use query::cache::CachePolicy;
use std::sync::Arc;
use rustc_serialize::{Encodable, Decodable, Encoder, Decoder};
use rustc_serialize::json;
use std::fs::{File};
use std::fs::{copy,PathExt};
use std::path::{Path,PathBuf};
use utils::ArcKV;
use std::marker::NoCopy;
use std::io::{SeekFrom,Write,Read,Seek};
use std::fs::OpenOptions;

//TODO rewrite with parameterization ok!! (generic simplecache)

/// A KeyVal storage/cache. Good for testing, simple implementation (a native
/// trust hashmap).
pub struct SimpleCache<V : KeyVal> {
  cache : HashMap<V::Key, V>,
  persi : Option<File>,
  _nocopy : NoCopy,
}

/// KVStoreRel implementation, slow, only for small size or testing.
impl<K1 : Key, K2 : Key, V : KeyVal<Key=(K1,K2)>> KVStoreRel<K1, K2, V> for SimpleCache<V> {
  fn get_vals_from_left(& self, k1 : &K1) -> Vec<V> {
    self.cache.iter().filter(|&(ref k,ref v)| k.0 == *k1).map(|(ref k, ref v)|(*v).clone()).collect()
  }
  fn get_vals_from_right(& self, k2 : &K2) -> Vec<V> {
    self.cache.iter().filter(|&(ref k,ref v)| k.1 == *k2).map(|(ref k, ref v)|(*v).clone()).collect()
  }
}

/*
//impl<T : KeyVal> KVCache<T> for SimpleCache<T> {
//impl<T : KeyVal> KVCache for SimpleCache<T> {
/// Not used correctly for the time being need some more work
impl<T : KeyVal> KVCache<T::Key, Arc<T>> for SimpleCache<T> {
  // type K = T::Key;
  //type KV = Arc<T>;
  #[inline]
  fn c_add_val(& mut self, k : T::Key, v : Arc<T>, (persistent, _) : (bool, Option<CachePolicy>)){
    // if cache we should consider caching priority and 
    // time in cache In fact only for testing cause persistent is not even persistent
    if persistent {
      self.cache.insert(k, v);
    }
  }

  #[inline]
  fn c_get_val(& self, k : &T::Key) -> Option<Arc<T>>{
    self.cache.get(k).cloned()
  }

  #[inline]
  fn c_remove_val(& mut self, k : &T::Key){
    self.cache.remove(k);
  }

}

*/


/// KVStore implementation with serialization to json (best for testing experimenting) if needed.
impl<T : KeyVal> KVStore<T> for SimpleCache<T> {
//impl<T : KeyVal> KVCache for SimpleCache<T> {
//impl<T : KeyVal> KVCache<T::Key, Arc<T>> for SimpleCache<T> {
 // type K = T::Key;
  //type KV = Arc<T>;
  #[inline]
  fn add_val(& mut self,  v : T, (persistent, _) : (bool, Option<CachePolicy>)){
    // if cache we should consider caching priority and 
    // time in cache In fact only for testing cause persistent is not even persistent
    if persistent {
      self.cache.insert(v.get_key(), v);
    }
  }

  #[inline]
  fn get_val(& self, k : &T::Key) -> Option<T>{
    self.cache.get(k).cloned()
  }

  #[inline]
  fn remove_val(& mut self, k : &T::Key){
    self.cache.remove(k);
  }

  #[inline]
  fn commit_store(& mut self) -> bool{
    match self.persi {
      Some(ref mut confFile) => {
        debug!("Commit call on Simple cache kvstore");
        // first bu copy TODO use rename instead.
        let bupath = confFile.path().unwrap().with_extension("_bu");
        if copy(confFile.path().unwrap(), &bupath).is_ok(){
          confFile.seek(SeekFrom::Start(0));
          // remove content
          confFile.set_len(0);
          // write new content
          // some issue to json serialize hash map due to key type so serialize vec of pair instead
          let vser : Vec<&T> = self.cache.values().collect();
          confFile.write(&json::encode(&vser).unwrap().into_bytes()[..]).is_ok()
        } else {
          false
        }
      },
      None => true,
    }
  }
}


/// KVStore implementation with serialization to json (best for testing experimenting) if needed.
impl<T : KeyVal> KVStoreCache<T> for SimpleCache<ArcKV<T>> {
  #[inline]
  fn add_val_c(& mut self,  v : ArcKV<T>, st : (bool, Option<CachePolicy>)){
    self.add_val(v,st)
  }

  #[inline]
  fn get_val_c(& self, k : &T::Key) -> Option<ArcKV<T>>{
    self.get_val(k)
  }

  #[inline]
  fn remove_val_c(& mut self, k : &T::Key){
    self.remove_val(k)
  }

  #[inline]
  fn commit_store_c(& mut self) -> bool{
    self.commit_store()
  }
}


impl<V : KeyVal> SimpleCache<V> {
  /// Optionaly specify a path for serialization and of course loading initial value.
  /// JSon is used, with some slow trade of due to issue when serializing hashmap with non string
  /// key.
  pub fn new (op : Option<PathBuf>) -> Self {
    let new = op.as_ref().map(|p|{
      let r = p.exists();
      r
    }).unwrap_or(true);
    debug!("Simple cache is new : {:?}", new);
    let mut persi = op.map(|p|OpenOptions::new().read(true).write(true).open(&p).unwrap());
    let map = match &mut persi {
      &mut Some(ref mut p) => {
        if !new {
          debug!("persi does not exist");
            HashMap::new()
        } else {
          debug!("reading simplecache");
          // TODO reput reading!!!!!!!!!!
          let mut jcont = String::new();
          p.read_to_string(&mut jcont).unwrap();
          let vser : Vec<V> = json::decode(&jcont[..]).unwrap_or_else(|e|panic!("Invalid config {:?}\n quiting",e));
          let map : HashMap<V::Key, V> = vser.into_iter().map(|v| (v.get_key(),v)).collect();
          map
        }
      },
      &mut None => HashMap::new(),
    };
    SimpleCache{cache : map, persi : persi, _nocopy : NoCopy}
  }
}
/*
impl<V : KeyVal> KVStore2<V> for SimpleCache<V> {
}*/


/// A simple implementation (basic hashmap) to store/cache query
pub struct SimpleCacheQuery<P : Peer, V : KeyVal> {
  cache : HashMap<QueryID, Query<P,V>>,
}

impl<P : Peer, V : KeyVal> SimpleCacheQuery<P, V> {
  pub fn new () -> Self{
    SimpleCacheQuery{cache : HashMap::new()}
  }
}


// query being transiant : no serialize -> could not use simple cache
// TODO a transient cache with transient keyval which could also be stored 
// -> need fn to_storable but also from_storable : this is only for query
// not sure usefull -> more likely implement both when possible
impl<P : Peer, V : KeyVal> QueryCache<P,V> for SimpleCacheQuery<P,V>  where P::Key : Send {
  #[inline]
  fn query_add(& mut self, qid : &QueryID, query : Query<P,V>) {
    self.cache.insert(qid.clone(), query);
  }
  #[inline]
  fn query_get(& mut self, qid : &QueryID) -> Option<&Query<P,V>> {
    self.cache.get(qid)
  }
  #[inline]
  fn query_remove(& mut self, quid : &QueryID){
      self.cache.remove(quid);
  }


  fn cache_clean_nodes(& mut self)-> Vec<Query<P,V>>{
    let expire = time::get_time();
    let mut remqid : Vec<QueryID> = Vec::new();
    let mut remq : Vec<Query<P,V>> = Vec::new();
    let mut initexpire : Vec<Query<P,V>> = Vec::new();
    for mut q in self.cache.iter(){
      match (q.1).get_expire() {
        Some(date) => if date > expire {
          warn!("expired query to be cleaned : {:?}", q.0);
          remq.push(q.1.clone());
          remqid.push(q.0.clone());
        },
        None => {
          initexpire.push(q.1.clone());//q is arc
        },
      }
    }
    for mut q in initexpire.into_iter(){
      q.set_expire(expire); 
    };
    for qid in remqid.iter(){
      self.cache.remove(qid);
    };
    remq
  }
}


