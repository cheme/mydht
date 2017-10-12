use std::collections::{HashMap};
use query::{QueryID, Query};
use query::cache::QueryCache;
//use std::time::Duration;
use peer::Peer;
use std::time::Instant;
use kvcache::{KVCache};
use mydhtresult::Result as MDHTResult;
//use rustc_serialize::{Encodable, Decodable, Encoder, Decoder};
//use std::fs::{File};
//use utils::ArcKV;
use std::marker::{PhantomData};
use rand::{
  thread_rng,
  Rng,
};
use utils::Ref;

pub type HashMapQuery<P,V,RP> = HashMap<QueryID, Query<P,V,RP>>;
//pub type CacheQuery<P,V> = KVCache<QueryID, Query<P,V>>;
/// A simple implementation (basic hashmap) to store/cache query
//pub struct SimpleCacheQuery<P : Peer, V : KeyVal> {
pub struct SimpleCacheQuery<P : Peer, V, RP : Ref<P>, C : KVCache<QueryID, Query<P,V,RP>>> {
  cache : C,
  // cache : HashMap<QueryID, Query<P,V>>,
  /// use randow id, if false sequential ids will be used
  randomids : bool,
  lastid : QueryID,
  _phdat : PhantomData<(P,V,RP)>,
}


impl<P : Peer, V, RP : Ref<P>> SimpleCacheQuery<P,V,RP,HashMapQuery<P,V,RP>> {
  pub fn new (randid : bool) -> Self{
    SimpleCacheQuery{cache : HashMap::new(),randomids : randid,lastid : 0, _phdat : PhantomData}
  }
}


// query being transiant : no serialize -> could not use simple cache
// TODO a transient cache with transient keyval which could also be stored 
// -> need fn to_storable but also from_storable : this is only for query
// not sure usefull -> more likely implement both when possible
impl<P : Peer, V, RP : Ref<P>, C : KVCache<QueryID, Query<P,V,RP>>> QueryCache<P,V,RP> for SimpleCacheQuery<P,V,RP,C>  where P::Key : Send {
  #[inline]
  fn query_add(&mut self, qid : QueryID, query : Query<P,V,RP>) {
    self.cache.add_val_c(qid, query);
  }
  #[inline]
  fn query_get(&mut self, qid : &QueryID) -> Option<&Query<P,V,RP>> {
    self.cache.get_val_c(qid)
  }
  #[inline]
  fn query_get_mut(&mut self, qid : &QueryID) -> Option<&mut Query<P,V,RP>> {
    self.cache.get_val_mut_c(qid)
  }
  fn query_update<F>(&mut self, qid : &QueryID, f : F) -> MDHTResult<bool> where F : FnOnce(&mut Query<P,V,RP>) -> MDHTResult<()> {
    self.cache.update_val_c(qid,f)
  }
  #[inline]
  fn query_remove(&mut self, quid : &QueryID) -> Option<Query<P,V,RP>> {
      self.cache.remove_val_c(quid)
  }
  fn new_id (&mut self) -> QueryID {
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



  fn cache_clean_nodes(& mut self)-> Vec<Query<P,V,RP>> {
    let expire = Instant::now();
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
    for q in initexpire.iter() {
      // clean cache at next clean : why ?? TODO document it
      self.cache.update_val_c(q,|mq| {mq.set_expire(expire); Ok(())}).unwrap();
    };

    let mut remq : Vec<Query<P,V,RP>> = Vec::with_capacity(remqid.len());
    for qid in remqid.iter(){
      match self.cache.remove_val_c(qid) {
        Some(q) => remq.push(q),
        None => error!("Not removed clean query, may be race"),
      }
    };
    remq
  }
}
