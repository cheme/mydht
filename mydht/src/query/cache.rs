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
use peer::Peer;
use keyval::KeyVal;
use transport::{ReadTransportStream,WriteTransportStream};
use mydhtresult::Result as MDHTResult;
use kvstore::CachePolicy;
use utils::Ref;

//TODO rewrite with parameterization ok!! (generic simplecache) TODO move to mod.rs
// TODO cache of a value : here only cache of peer query
/// cache of query interface TODO make it kvcach + two methods??
pub trait QueryCache<P : Peer,V,RP : Ref<P>> {
  fn query_add(&mut self, QueryID, Query<P,V,RP>);
  fn query_get(&mut self, &QueryID) -> Option<&Query<P,V,RP>>;
  fn query_get_mut(&mut self, &QueryID) -> Option<&mut Query<P,V,RP>>;
  fn query_update<F>(&mut self, &QueryID, f : F) -> MDHTResult<bool> where F : FnOnce(&mut Query<P,V,RP>) -> MDHTResult<()>;
  fn query_remove(&mut self, &QueryID) -> Option<Query<P,V,RP>>;
  fn cache_clean_nodes(&mut self) -> Vec<Query<P,V,RP>>;
  /// get a new id , used before query_add
  fn new_id(&mut self) -> QueryID;
}


