use query::{QueryID, Query};

use serde::{Serialize};
use serde::de::{DeserializeOwned};
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
use transport::{ReadTransportStream,WriteTransportStream};
use mydhtresult::Result as MDHTResult;
use kvstore::CachePolicy;
use utils::Ref;

//TODO rewrite with parameterization ok!! (generic simplecache) TODO move to mod.rs
// TODO cache of a value : here only cache of peer query
/// cache of query interface TODO make it kvcach + two methods??
pub trait QueryCache<P : Peer,V : KeyVal> {
  fn query_add(&mut self, QueryID, Query<P,V>);
  fn query_get(&mut self, &QueryID) -> Option<&Query<P,V>>;
  fn query_get_mut(&mut self, &QueryID) -> Option<&mut Query<P,V>>;
  fn query_update<F>(&mut self, &QueryID, f : F) -> MDHTResult<bool> where F : FnOnce(&mut Query<P,V>) -> MDHTResult<()>;
  fn query_remove(&mut self, &QueryID) -> Option<Query<P,V>>;
  fn cache_clean_nodes(&mut self) -> Vec<Query<P,V>>;
  /// get a new id , used before query_add
  fn new_id(&mut self) -> QueryID;
}
/*
pub fn cache_clean
 <P : Peer,
  R : 
  V : KeyVal,
  TR : ReadTransportStream,
  TW : WriteTransportStream,
  QC : QueryCache<P,R,V>>
 (qc : & mut QC, 
  sp : &Sender<PeerMgmtMessage<P,V,TR,TW>>) {
    let to_clean = qc.cache_clean_nodes();
    for q in to_clean.into_iter(){
      debug!("Cache clean process relase a query");
      q.release_query(sp);
    }
}
*/


