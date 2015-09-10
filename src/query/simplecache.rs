use std::collections::{HashMap};
use std::hash::Hash;
use query::{QueryID, Query};
use query::cache::QueryCache;
//use std::time::Duration;
use peer::Peer;
use time;
use keyval::{KeyVal,Key};
use kvstore::{KVStore,KVStoreRel};
use kvcache::{KVCache};
use query::cache::CachePolicy;
use mydhtresult::Result as MDHTResult;
//use rustc_serialize::{Encodable, Decodable, Encoder, Decoder};
use rustc_serialize::json;
//use std::fs::{File};
use std::fs::{copy,PathExt};
use std::path::{PathBuf};
//use utils::ArcKV;
use std::marker::{PhantomData};
use std::io::{SeekFrom,Write,Read,Seek};
use std::fs::OpenOptions;
use rand::{thread_rng,Rng};
use num::traits::ToPrimitive;

//TODO rewrite with parameterization ok!! (generic simplecache)

//pub trait Key : fmt::Debug + Hash + Eq + Clone + Send + Sync + Ord + 'static{}
/// A KeyVal storage/cache. Good for testing, simple implementation (a native
/// trust hashmap).
pub struct SimpleCache<V : KeyVal, C : KVCache<<V as KeyVal>::Key, V>> where V::Key : Hash {
  cache : C,
  persi : Option<PathBuf>,

  _phdat : PhantomData<V>
}


/// KVStoreRel implementation, slow, only for small size or testing.
// TODO shouldn't we return a ref instead??: no as a store, but we could do a cacherel if TODO see
// if needed (in truststore)
impl<K1 : Key + Hash, K2 : Key + Hash, V : KeyVal<Key=(K1,K2)>, C : KVCache<(K1,K2),V>> KVStoreRel<K1, K2, V> for SimpleCache<V,C>
 {
  fn get_vals_from_left(& self, k1 : &K1) -> Vec<V> {
    self.cache.strict_fold_c(Vec::new(),|mut r, (ref k,ref v)| {
      if k.0 == *k1 {
        r.push((*v).clone())
      };
      r
    })
    //self.cache.iter_c().filter(|&(ref k,ref v)| k.0 == *k1).map(|(ref k, ref v)|(*v).clone()).collect()
  }
  fn get_vals_from_right(& self, k2 : &K2) -> Vec<V> {
    self.cache.strict_fold_c(Vec::new(),|mut r, (ref k,ref v)| {
      if k.1 == *k2 {
        r.push((*v).clone())
      };
      r
    })
 
//    self.cache.iter_c().filter(|&(ref k,ref v)| k.1 == *k2).map(|(ref k, ref v)|(*v).clone()).collect()
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
impl<T : KeyVal, C : KVCache<<T as KeyVal>::Key, T>> KVStore<T> for SimpleCache<T,C> 
  where T::Key : Hash  {

  #[inline]
  fn add_val(& mut self,  v : T, (persistent, _) : (bool, Option<CachePolicy>)){
    // if cache we should consider caching priority and 
    // time in cache In fact only for testing cause persistent is not even persistent
    if persistent {
      self.cache.add_val_c(v.get_key(), v);
    }
  }

  #[inline]
  fn get_val(& self, k : &T::Key) -> Option<T> {
    self.cache.get_val_c(k).cloned()
  }
  #[inline]
  fn has_val(& self, k : &T::Key) -> bool {
    self.cache.has_val_c(k)
  }


  #[inline]
  fn remove_val(& mut self, k : &T::Key){
    self.cache.remove_val_c(k);
  }
  #[inline] // TODO switch to new interface
  fn commit_store(& mut self) -> bool {
    self.commit_store_new().is_ok()
  }
}

 
impl<T : KeyVal, C : KVCache<<T as KeyVal>::Key, T>> SimpleCache<T,C> 
  where T::Key : Hash  {
  #[inline]
  /// TODO replace commit store
  fn commit_store_new(& mut self) -> MDHTResult<()> {
    match self.persi {
      Some(ref confpath) => {
        let mut conffile = OpenOptions::new().read(true).write(true).open(confpath).unwrap();
        debug!("Commit call on Simple cache kvstore");
        // first bu copy TODO use rename instead.
        let bupath = confpath.with_extension("_bu");
        try!(copy(confpath, &bupath));
        try!(conffile.seek(SeekFrom::Start(0)));
        // remove content
        try!(conffile.set_len(0));
        // write new content
        // some issue to json serialize hash map due to key type so serialize vec of pair instead
        //fn second<A, B>((_, b): (A, B)) -> B { b }
        //let second: fn((&'a <T as KeyVal>::Key,&'a T)) -> &'a T = second;
        let l  = self.cache.len_c();
        let vser : Vec<&T> = self.cache.strict_fold_c(Vec::with_capacity(l),|mut v,p| {v.push(p.1);v});
        try!(conffile.write(&json::encode(&vser).unwrap().into_bytes()[..]));
        Ok(())
      },
      None => Ok(()),
    }
  }


}

impl<V : KeyVal> SimpleCache<V,HashMap<<V as KeyVal>::Key, V>> where V::Key : Hash {



  /// Optionaly specify a path for serialization and of course loading initial value.
  /// JSon is used, with some slow trade of due to issue when serializing hashmap with non string
  /// key.
  /// Initialize a simplecache of HashMap.
  pub fn new (op : Option<PathBuf>) -> Self {
    let new = op.as_ref().map(|p|{
      let r = p.exists();
      r
    }).unwrap_or(true);
    debug!("Simple cache is new : {:?}", new);
    let mut inifile = op.as_ref().map(|p|OpenOptions::new().read(true).write(true).open(p).unwrap());
    let map = match &mut inifile {
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
    SimpleCache{cache : map, persi : op, _phdat : PhantomData}
  }
}

pub type HashMapQuery<P,V> = HashMap<QueryID, Query<P,V>>;
//pub type CacheQuery<P,V> = KVCache<QueryID, Query<P,V>>;
/// A simple implementation (basic hashmap) to store/cache query
//pub struct SimpleCacheQuery<P : Peer, V : KeyVal> {
pub struct SimpleCacheQuery<P : Peer, V : KeyVal, C : KVCache<QueryID, Query<P,V>>> {
  cache : C,
 // cache : HashMap<QueryID, Query<P,V>>,
  /// use randow id, if false sequential ids will be used
  randomids : bool,
  lastid : QueryID,
  _phdat : PhantomData<(P,V)>,
}


impl<P : Peer, V : KeyVal> SimpleCacheQuery<P,V, HashMapQuery<P,V>> {
  pub fn new (randid : bool) -> Self{
    SimpleCacheQuery{cache : HashMap::new(),randomids : randid,lastid : 0, _phdat : PhantomData}
  }
}


// query being transiant : no serialize -> could not use simple cache
// TODO a transient cache with transient keyval which could also be stored 
// -> need fn to_storable but also from_storable : this is only for query
// not sure usefull -> more likely implement both when possible
impl<P : Peer, V : KeyVal, C : KVCache<QueryID, Query<P,V>>> QueryCache<P,V> for SimpleCacheQuery<P,V,C>  where P::Key : Send {
  #[inline]
  fn query_add(&mut self, qid : QueryID, query : Query<P,V>) {
    self.cache.add_val_c(qid, query);
  }
  #[inline]
  fn query_get(&mut self, qid : &QueryID) -> Option<&Query<P,V>> {
    self.cache.get_val_c(qid)
  }
  #[inline]
  fn query_remove(&mut self, quid : &QueryID){
      self.cache.remove_val_c(quid);
  }
  fn newid (&mut self) -> QueryID {
    if self.randomids {
      let mut rng = thread_rng();
      // (eg database connection)
      //rng.gen_range(0,65555)
      rng.next_u64().to_usize().unwrap()
    } else {
      // TODO implement ID recycling!!! a stack (plug it in query rem)
      self.lastid += 1;
      self.lastid
    }
  }



  fn cache_clean_nodes(& mut self)-> Vec<Query<P,V>> {
    let expire = time::get_time();
    let mut remqid : Vec<QueryID> = Vec::new();
    let mut remq : Vec<Query<P,V>> = Vec::new();
    let mut initexpire : Vec<Query<P,V>> = Vec::new();
    // use of fnmut (should be cleaner with foldm and vec in params
    let mr = self.cache.map_inplace_c(|q| {
      Ok(match (q.1).get_expire() {
        Some(date) => if date > expire {
          warn!("expired query to be cleaned : {:?}", q.0);
          remq.push(q.1.clone());
          remqid.push(q.0.clone());
        },
        None => {
          initexpire.push(q.1.clone());//q is arc
        },
      })
    });
    if mr.is_err() { // TODO return result instead
      error!("map in place failure during cache clean : {:?}", mr.err());
    };
/* 
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
    }*/
    for mut q in initexpire.into_iter(){
      q.set_expire(expire); 
    };
    for qid in remqid.iter(){
      self.cache.remove_val_c(qid);
    };
    remq
  }
}

