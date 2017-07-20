use keyval::KeyVal;
use peer::Peer;
use rules::DHTRules;
use kvstore::StoragePriority;
use std::collections::VecDeque;
use std::sync::Arc;

/// Query Priority.
pub type QueryPriority = u8; // TODO rules for getting number of hop from priority -> convert this to a trait with methods.
/// Query ID.
pub type QueryID = usize;

#[derive(RustcDecodable,RustcEncodable,Debug,Clone)]
/// keep trace of route to avoid loop when proxying
pub enum LastSent<P : Peer> {
  LastSentHop(usize, VecDeque<P::Key>),
  LastSentPeer(usize, VecDeque<P::Key>),
}


#[derive(RustcDecodable,RustcEncodable,Debug,Clone)]
///  Main infos about a running query. Notably running mode, chunk config, no loop info, priorities
///  and remaing hop (on a new query it is the max number of hop) plus number of query (on each hop
///  number of peer where query should be forwarded).
pub struct QueryMsg<P : Peer> {
  /// Info required to identify query and its mode
  /// TODO most of internal info should be in queryconfmsg
  pub modeinfo : QueryModeMsg<P>,
  /// Query chunk
  pub chunk : QueryChunk,
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
  /// nb result expected
  pub nb_res : usize,
}


impl<P : Peer> QueryMsg<P> {
  pub fn dec_nbhop<QR : DHTRules>(&mut self,qr : &QR) {
    self.rem_hop -= qr.nbhop_dec();
  }
  pub fn get_query_id(&self) -> QueryID {
    self.modeinfo.get_qid_clone()
  }
}


// variant of query mode to use as a configuration in application
#[derive(RustcDecodable,RustcEncodable,Debug,Clone)]
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
#[derive(RustcDecodable,RustcEncodable,Debug,Clone)]
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


// variant of query mode to communicate with peers
#[derive(RustcDecodable,RustcEncodable,Debug,Clone)]
/// QueryMode info to use in message between peers.
pub enum QueryModeMsg<P : Peer> {
    /// The node to reply to, and the managed query id for this node (not our id).
    AProxy(Arc<P>, QueryID), // reply to preceding Node which keep a trace of this query  // TODO switc to arc node to avoid all clone
    /// The node to reply to, and the managed query id for this node (not our id).
    Asynch(Arc<P>, QueryID), // reply directly to given Node which keep a trace of this query
    /// The remaining number of hop before switching to AProxy. The node to reply to, and the managed query id for this node (not our id).
    AMix(u8, Arc<P>, QueryID), // after a few hop switch to asynch
}

/// Query mode utilities
impl<P : Peer> QueryModeMsg<P> {
  /// get corresponding querymode
  pub fn get_mode (&self) -> QueryMode {
    match self {
      &QueryModeMsg::AProxy (_, _) => QueryMode::AProxy,
      &QueryModeMsg::Asynch (_, _) => QueryMode::Asynch,
      &QueryModeMsg::AMix (h,_, _) => QueryMode::AMix(h),
    }
   
  }
    /// Get peers to reply to
    pub fn get_rec_node(&self) -> &Arc<P> {
        match self {
            &QueryModeMsg::AProxy (ref n, _) => n,
            &QueryModeMsg::Asynch (ref n, _) => n,
            &QueryModeMsg::AMix (_,ref n, _) => n,
        }
    }
    /// Get queryid if the mode use managed query.
    pub fn get_qid (&self) -> &QueryID {
        match self {
            &QueryModeMsg::AProxy (_, ref q) => q,
            &QueryModeMsg::Asynch (_, ref q) => q,
            &QueryModeMsg::AMix (_,_, ref q) => q,
        }
    }


    pub fn get_qid_clone (&self) -> QueryID {
        match self {
            &QueryModeMsg::AProxy (_, ref q) => q.clone(),
            &QueryModeMsg::Asynch (_, ref q) => q.clone(),
            &QueryModeMsg::AMix (_,_, ref q) => q.clone(),
        }
    }
    pub fn to_qid (self) -> QueryID {
        match self {
            QueryModeMsg::AProxy (_, q) => q,
            QueryModeMsg::Asynch (_, q) => q,
            QueryModeMsg::AMix (_,_, q) => q,
        }
    }
 
    /// Copy conf with a new qid and peer to reply to : when proxying a managed query we do not use the previous id.
    /// TODO see in mut not better
    pub fn new_hop (&self, p : Arc<P>, qid : QueryID) -> Self {
        match self {
            &QueryModeMsg::AProxy (_, _) => QueryModeMsg::AProxy (p, qid),
            &QueryModeMsg::Asynch (_, _) => QueryModeMsg::Asynch (p, qid),
            &QueryModeMsg::AMix (ref a,_, _) => QueryModeMsg::AMix (a.clone(),p, qid),
        }
    }
    /// Copy conf with a new qid : when proxying a managed query we do not use the previous id.
    /// TODO see in mut not better
    pub fn new_qid (&self, qid : QueryID) -> Self {
        match self {
            &QueryModeMsg::AProxy (ref a, _) => QueryModeMsg::AProxy (a.clone(), qid),
            &QueryModeMsg::Asynch (ref a, _) => QueryModeMsg::Asynch (a.clone(), qid),
            &QueryModeMsg::AMix (ref a, ref b, _) => QueryModeMsg::AMix (a.clone(), b.clone(), qid),
        }
    }
    pub fn set_qid (&mut self, qid : QueryID) {
        match self {
            &mut QueryModeMsg::AProxy (_, ref mut q) => *q = qid,
            &mut QueryModeMsg::Asynch (_, ref mut q) => *q = qid,
            &mut QueryModeMsg::AMix (_, _, ref mut q) => *q = qid,
        }
    }
 
    /// Copy conf with a new  andpeer to reply to : when proxying a managed query we do not use the previous id.
    /// TODO see in mut not better
    pub fn new_peer (&self, p : Arc<P>) -> Self {
        match self {
            &QueryModeMsg::AProxy (_, ref a) => QueryModeMsg::AProxy (p, a.clone()),
            &QueryModeMsg::Asynch (_, ref a) => QueryModeMsg::Asynch (p, a.clone()),
            &QueryModeMsg::AMix (ref a,_, ref b) => QueryModeMsg::AMix (a.clone(), p, b.clone()),
        }
    }
}

