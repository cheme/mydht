use query::{QueryID, Query};
//use std::time::Duration;
use time;
use rand::thread_rng;
use rand::Rng;
use kvcache::KVCache;
use std::collections::HashMap;
use std::sync::mpsc::{Sender};
use std::marker::PhantomData;
use procs::mesgs::{PeerMgmtMessage};
use peer::Peer;
use keyval::KeyVal;
use serde::{Serializer,Serialize,Deserializer,Deserialize};
use transport::{ReadTransportStream,WriteTransportStream};
use mydhtresult::Result as MDHTResult;
use kvstore::CachePolicy;


//TODO rewrite with parameterization ok!! (generic simplecache) TODO move to mod.rs
// TODO cache of a value : here only cache of peer query
/// cache of query interface
pub trait QueryCache<P : Peer, V : KeyVal> {
  fn query_add(&mut self, QueryID, Query<P,V>);
  fn query_get(&mut self, &QueryID) -> Option<&Query<P,V>>; // TODO map in place to mut queries and remove dirty mutex in query definition
  fn query_update<F>(&mut self, &QueryID, f : F) -> MDHTResult<bool> where F : FnOnce(&mut Query<P,V>) -> MDHTResult<()>;
  fn query_remove(&mut self, &QueryID) -> Option<Query<P,V>>;
  fn cache_clean_nodes(&mut self) -> Vec<Query<P,V>>;
  /// get a new id , used before query_add
  fn newid(&mut self) -> QueryID;
}

pub fn cache_clean
 <P : Peer,
  V : KeyVal,
  TR : ReadTransportStream,
  TW : WriteTransportStream,
  QC : QueryCache<P,V>>
 (qc : & mut QC, 
  sp : &Sender<PeerMgmtMessage<P,V,TR,TW>>) {
    let to_clean = qc.cache_clean_nodes();
    for q in to_clean.into_iter(){
      debug!("Cache clean process relase a query");
      q.release_query(sp);
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
  fn query_update<F>(&mut self, qid : &QueryID, f : F) -> MDHTResult<bool> where F : FnOnce(&mut Query<P,V>) -> MDHTResult<()> {
    self.cache.update_val_c(qid,f)
  }
  #[inline]
  fn query_remove(&mut self, quid : &QueryID) -> Option<Query<P,V>> {
      self.cache.remove_val_c(quid)
  }
  fn newid (&mut self) -> QueryID {
    if self.randomids {
      let mut rng = thread_rng();
      // (eg database connection)
      //rng.gen_range(0,65555)
      rng.next_u64() as usize
    } else {
      // TODO implement ID recycling!!! a stack (plug it in query rem)
      self.lastid += 1;
      self.lastid
    }
  }



  fn cache_clean_nodes(& mut self)-> Vec<Query<P,V>> {
    let expire = time::get_time();
    let mut remqid : Vec<QueryID> = Vec::new();
    let mut initexpire : Vec<QueryID> = Vec::new();
    // use of fnmut (should be cleaner with foldm and vec in params
    let mr = self.cache.map_inplace_c(|q| {
      Ok(match (q.1).get_expire() {
        Some(date) => if date > expire {
          warn!("expired query to be cleaned : {:?}", q.0);
          remqid.push(q.0.clone());
        },
        None => {
          initexpire.push(q.0.clone());
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
    for mut q in initexpire.iter() {
      self.cache.update_val_c(q,|mut mq| {mq.set_expire(expire); Ok(())});
    };

    let mut remq : Vec<Query<P,V>> = Vec::with_capacity(remqid.len());
    for qid in remqid.iter(){
      match self.cache.remove_val_c(qid) {
        Some(q) => remq.push(q),
        None => error!("Not removed clean query, may be race"),
      }
    };
    remq
  }
}

