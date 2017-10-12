use std::sync::{Arc,Mutex,Condvar};
use serde::{Serializer,Serialize,Deserializer};
use serde::de::{DeserializeOwned};
//use peer::{PeerPriority};
use std::time::{Instant,Duration};
use peer::Peer;
use std::sync::mpsc::{Sender};
use kvstore::CachePolicy;
use std::collections::VecDeque;
//use procs::RunningProcesses;
use keyval::{KeyVal};
use kvstore::{StoragePriority};
use utils::{
  Either,
  Ref,
};
use procs::api::ApiQueryId;
//use num::traits::ToPrimitive;
use rules::DHTRules;
use transport::{ReadTransportStream,WriteTransportStream};
pub use mydht_base::query::*;

pub mod cache;
pub mod simplecache;


/// Usage of route tracing to avoid loop. It involve some tracing of the route. Depending on
/// routing implementation and query mode it could be relevant or not.
/// Depending on bool value usize is :
/// bool is true, `LastSentHop` mode : usize is the nb of hop for which we should store peer.
/// Note that number of peer could be bigger than int due to multiple peer query.
/// bool is false, `LastSentPeer` mode : usize is the max nb of peer in the last sent history.
pub type LastSentConf = Option<(usize,bool)>; // if bool true then ls hop else ls peer
//pub type LastSend<P : Peer> = (int, Vec<P::Key>); // int is nb of hop for which we should store info. Vec contain id of the peer having been requested. Note that Vec could be bigger than int due to multiple peer query (we obviously store others peer in Vec). This is use when proxying a query to avoid some peer. // TODO variant of last sent where we only limit number of peer (no hop estimation : simplier).


// TODO switch condvar to something with timeout
#[derive(Clone)]
/// Internal data type to manage query reply
pub enum QReply<P : Peer,V,RP : Ref<P>> {
  /// send reply to api of query id, wait for nb res in vec or nb error
  Local(ApiQueryId,usize,Vec<V>,usize,QueryPriority),
  /// reply should be forwarded given a query conf.
  Dist(QueryModeMsg<P>,Option<RP>,usize,Vec<V>,usize),
}


pub struct QueryConf {
  pub mode : QueryMode,
//  pub chunk : QueryChunk,
  pub hop_hist : LastSentConf,
}
impl QueryConf {
  /// TODO should be use internally, not to build request + need param in queryConf
  pub fn query_message<P : Peer>(&self,me : &P, nb_res : usize, nb_hop : u8,nb_forw : u8, prio : QueryPriority) -> QueryMsg<P> {
    let mode_info = match self.mode {
      QueryMode::AProxy => QueryModeMsg::AProxy(QUERY_ID_DEFAULT),
      QueryMode::Asynch => QueryModeMsg::Asynch(me.get_key(),me.get_address().clone(),QUERY_ID_DEFAULT),
      QueryMode::AMix(ref nb) => QueryModeMsg::AMix(nb.clone(),QUERY_ID_DEFAULT),
    };
    let hop_hist = self.hop_hist.map(|(nb,mode)| {
      let mut hist = VecDeque::new();
      if nb > 0 {
        hist.push_back(me.get_key());
      }
      if mode {
        LastSent::LastSentHop(nb,hist)
      } else {
        LastSent::LastSentPeer(nb,hist)
      }
    });
    QueryMsg {
      mode_info : mode_info,
      hop_hist : hop_hist,
      // TODO delete storage prio
      storage : StoragePriority::Local,
      rem_hop : nb_hop,
      nb_forw : nb_forw,
      prio : prio,
      nb_res : nb_res,
    }
  }
}



//#[derive(Clone)]
/// The query is seen as ok when all peer reply None or the first peer replies something (if number
/// TODO remove QueryID (useless)
pub struct Query<P : Peer, V, RP : Ref<P>> (pub QueryID, pub QReply<P,V,RP>, pub Option<Instant>);

/// Query methods
impl<P : Peer,V, RP : Ref<P>> Query<P,V,RP> {
   
  /// Get expire date for query (used by cleaning process of query cache).
  pub fn get_expire(&self) -> Option<Instant> {
    self.2.clone()
  }

  #[inline]
  /// Update expire date of query.
  pub fn set_expire(&mut self, expire : Instant) {
    self.2 = Some(expire)
  }
}








