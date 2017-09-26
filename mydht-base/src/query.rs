use serde::{Serializer,Serialize,Deserialize,Deserializer};
use serde::de::DeserializeOwned;
use keyval::KeyVal;
use peer::Peer;
use rules::DHTRules;
use kvstore::StoragePriority;
use std::collections::VecDeque;
use std::sync::Arc;
use std::mem::replace;
use utils::{
  Ref,
};
use std::cmp::min;
/// Query Priority.
pub type QueryPriority = u8; // TODO rules for getting number of hop from priority -> convert this to a trait with methods.
/// Query ID.
pub type QueryID = usize;

#[derive(Deserialize,Serialize,Debug,Clone)]
#[serde(bound(deserialize = ""))]
/// keep trace of route to avoid loop when proxying
pub enum LastSent<P : Peer> {
  LastSentHop(usize, VecDeque<P::Key>),
  LastSentPeer(usize, VecDeque<P::Key>),
}


#[derive(Deserialize,Serialize,Debug,Clone)]
#[serde(bound(deserialize = ""))]
///  Main infos about a running query. Notably running mode, chunk config, no loop info, priorities
///  and remaing hop (on a new query it is the max number of hop) plus number of query (on each hop
///  number of peer where query should be forwarded).
pub struct QueryMsg<P : Peer> {
  /// Info required to identify query and its mode
  /// TODO most of internal info should be in queryconfmsg
  pub mode_info : QueryModeMsg<P>,
//  /// Query chunk
//  pub chunk : QueryChunk,
  /// history of previous hop for routing (especialy in small non anonymous networks)
  pub hop_hist : Option<LastSent<P>>,
  /// storage mode (propagate, store if proxy...) 
  pub storage : StoragePriority,
  /// remaining nb hop
  pub rem_hop : u8,
  /// nb query forward
  pub nb_forw : u8,
  /// prio
  pub prio : QueryPriority,
  /// nb result expected (forwarding query is done after receiving result)
  pub nb_res : usize,
}

#[derive(Deserialize,Serialize,Debug,Clone)]
#[serde(bound(deserialize = ""))]
pub struct PropagateMsg<P : Peer> {
  /// history of previous hop for routing (especialy in small non anonymous networks)
  pub hop_hist : Option<LastSent<P>>,
  /// remaining nb hop
  pub rem_hop : u8,
  /// nb query forward
  pub nb_forw : u8,
  /// nb propagate (no need to wait reply to propagate)
  pub nb_res : usize,
}


impl<P : Peer> QueryMsg<P> {
  pub fn dec_nbhop<QR : DHTRules>(&mut self,qr : &QR) {
    self.rem_hop -= qr.nbhop_dec();
  }
  pub fn get_query_id(&self) -> QueryID {
    self.mode_info.get_qid().clone()
  }

  pub fn to_next_hop<QR : DHTRules> (&mut self, p : &P, qid : QueryID, qr : &QR) -> QueryModeMsg<P> {
    let nbdec = qr.nbhop_dec();
    self.rem_hop -= nbdec;
    let n_mode_info = self.mode_info.new_hop(p,qid,nbdec);
    let o_mode_info = replace(&mut self.mode_info, n_mode_info);
    let (nb_for, nb_res_for) =  qr.nb_proxy_with_nb_res(self);
    self.nb_res = nb_res_for;
    self.nb_forw = nb_for;

    o_mode_info


  }
  /// after getting a route, update query
  pub fn update_lastsent_conf<RP : Ref<P>>(&mut self,  peers : &Vec<RP>, nbquery : u8) {
    match self.hop_hist {
      Some(LastSent::LastSentPeer(maxnb,ref mut lpeers)) => {
        let totalnb = peers.len() + lpeers.len();
        if totalnb > maxnb {
          for _ in 0..(totalnb - maxnb) {
            lpeers.pop_front();
          };
        }else{};
        for p in peers.iter(){
          lpeers.push_back(p.borrow().get_key());
        };
      },
      Some(LastSent::LastSentHop(ref mut hop,ref mut lpeers)) => {

        if *hop > 0 {
          // buffer one more hop
          *hop -= 1;
        } else {
          for _ in 0..nbquery { // this is an approximation (could be less)
            lpeers.pop_front();
          };
        };
        for p in peers.iter(){
          lpeers.push_back(p.borrow().get_key());
        };
      },
      None => (),
    };
  }


}


// variant of query mode to use as a configuration in application
#[derive(Deserialize,Serialize,Debug,Clone)]
#[serde(bound(deserialize = ""))]
/// Query Mode defines the way our DHT communicate, it is very important.
/// Unless messageencoding does not serialize it, all query mode could be used. 
/// For some application it could be relevant to forbid the reply to some query mode : 
/// TODO implement a filter in server process (and client proxy).
pub enum QueryMode {
  /// Asynch proxy. Query do not block, they are added to the query manager
  /// cache.
  /// When proxied, the query (unless using noloop history) may not give you the originator of the query (only
  /// the remaining number of hop which could be randon depending on rulse).
  AProxy,
  /// reply directly to the emitter of the query, even if the query has been proxied. 
  /// The peer to reply to is therefore added to the query. 
  Asynch,
  /// After a few hop switch to asynch (from AProxy). Therefore AProxy originator may not be the
  /// query originator.
  AMix(u8),
}
/*
#[derive(Deserialize,Serialize,Debug,Clone)]
// TODO serialize without Paths : Deserialize to dummy path
/// When KeyVal use an attachment we should use specific transport strategy.
pub enum QueryChunk{
    /// reply full value no chunks
    None,
    /// reply with file attached, no chunks
    Attachment,
    /// reply with Table of chunks TODO not implemented
    Table, // table for this size chunk // size and hash type is not in message : same rules must be used
//    Mix(u8), // reply depending on actual size of content with treshold
    /// reply with a specified chunk TODO not implemented
    Chunk, // chunk index // size and hash are from rules
}


// TODO add serialize trait to this one for transmission
pub trait ChunkTable<V : KeyVal> {
  fn init(&self, V) -> bool; // init the table for the stream (or get from persistence...)
  fn check(&self, u32, String) -> bool; // check chunk for a index (calculate and verify hash) TODO not string but some byte array for ressource
}
*/

// variant of query mode to communicate with peers
#[derive(Deserialize,Serialize,Debug,Clone)]
#[serde(bound(deserialize = ""))]
/// QueryMode info to use in message between peers.
/// R is PeerRef
pub enum QueryModeMsg<P : Peer> {
    /// The node to reply to, and the managed query id for this node (not our id).
    AProxy(<P as KeyVal>::Key, <P as Peer>::Address, QueryID), // reply to preceding Node which keep a trace of this query  // TODO switc to arc node to avoid all clone
    /// The node to reply to, and the managed query id for this node (not our id).
    Asynch(<P as KeyVal>::Key, <P as Peer>::Address, QueryID), // reply directly to given Node which keep a trace of this query
    /// The remaining number of hop before switching to AProxy. The node to reply to, and the managed query id for this node (not our id).
    AMix(u8, <P as KeyVal>::Key, <P as Peer>::Address, QueryID), // after a few hop switch to asynch
}

/// Query mode utilities
impl<R : Peer> QueryModeMsg<R> {
  pub fn do_store (&self) -> bool {
    match self {
      &QueryModeMsg::AProxy (..) => false,
      &QueryModeMsg::Asynch (..) => true,
      &QueryModeMsg::AMix (..) => true,
    }
  }
  /// get corresponding querymode
  pub fn get_mode (&self) -> QueryMode {
    match self {
      &QueryModeMsg::AProxy (_,_,_) => QueryMode::AProxy,
      &QueryModeMsg::Asynch (_,_,_) => QueryMode::Asynch,
      &QueryModeMsg::AMix (h,_,_,_) => QueryMode::AMix(h),
    }
   
  }
    /// Get peers to reply to
    pub fn get_rec_node(&self) -> &<R as KeyVal>::Key {
        match self {
            &QueryModeMsg::AProxy (ref n,_,_) => n,
            &QueryModeMsg::Asynch (ref n,_,_) => n,
            &QueryModeMsg::AMix (_,ref n,_,_) => n,
        }
    }
    /// Get peers to reply to
    pub fn get_rec_address(&self) -> &<R as Peer>::Address {
        match self {
            &QueryModeMsg::AProxy (_,ref n,_) => n,
            &QueryModeMsg::Asynch (_,ref n,_) => n,
            &QueryModeMsg::AMix (_,_,ref n,_) => n,
        }
    }

    /// Get queryid if the mode use managed query.
    pub fn get_qid (&self) -> &QueryID {
        match self {
            &QueryModeMsg::AProxy (_,_,ref q) => q,
            &QueryModeMsg::Asynch (_,_,ref q) => q,
            &QueryModeMsg::AMix (_,_,_,ref q) => q,
        }
    }

    /// Copy conf with a new qid and peer to reply to : when proxying a managed query we do not use the previous id.
    /// return bool true if the query need storage
    /// TODO see in mut not better
    pub fn new_hop (&self, p : &R, qid : QueryID,nbdec : u8) -> Self {
        match self {
            &QueryModeMsg::AProxy (_,_,_) => QueryModeMsg::AProxy (p.get_key(), p.get_address().clone(), qid),
            &QueryModeMsg::Asynch (ref k,ref a,ref qid) => QueryModeMsg::Asynch (k.clone(), a.clone(), qid.clone()),
            &QueryModeMsg::AMix (a,_,_, _) if a > 0 => QueryModeMsg::AMix (a - min(a,nbdec),p.get_key(), p.get_address().clone(),qid),
            &QueryModeMsg::AMix (_,_,_, _)  => QueryModeMsg::Asynch (p.get_key(), p.get_address().clone(), qid),
        }
    }
    pub fn set_qid (&mut self, qid : QueryID) {
        match self {
            &mut QueryModeMsg::AProxy (_,_,ref mut q) => *q = qid,
            &mut QueryModeMsg::Asynch (_,_,ref mut q) => *q = qid,
            &mut QueryModeMsg::AMix (_,_,_,ref mut q) => *q = qid,
        }
    }

}

