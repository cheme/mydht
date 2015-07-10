use std::sync::{Arc,Mutex,Semaphore,Condvar};
use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};
use peer::{PeerPriority};
use time::Duration;
use time::{self,Timespec};
use peer::Peer;
use std::sync::mpsc::{Sender};
use procs::mesgs::{KVStoreMgmtMessage,PeerMgmtMessage,QueryMgmtMessage};
use query::cache::CachePolicy;
use std::collections::VecDeque;
use procs::RunningProcesses;
use keyval::{KeyVal};
use kvstore::{StoragePriority};
use utils::Either;
use num::traits::ToPrimitive;
use rules::DHTRules;
pub mod cache;
pub mod simplecache;
// a semaphore with access to its current state
pub type SemaState = (Semaphore,isize);

#[derive(RustcDecodable,RustcEncodable,Debug,Clone)]
/// keep trace of route to avoid loop when proxying
pub enum LastSent<P : Peer> {
  LastSentHop(usize, VecDeque<P::Key>),
  LastSentPeer(usize, VecDeque<P::Key>),
}

/// Usage of route tracing to avoid loop. It involve some tracing of the route. Depending on
/// routing implementation and query mode it could be relevant or not.
/// Depending on bool value usize is :
/// bool is true, `LastSentHop` mode : usize is the nb of hop for which we should store peer.
/// Note that number of peer could be bigger than int due to multiple peer query.
/// bool is false, `LastSentPeer` mode : usize is the max nb of peer in the last sent history.
pub type LastSentConf = Option<(usize,bool)>; // if bool true then ls hop else ls peer
//pub type LastSend<P : Peer> = (int, Vec<P::Key>); // int is nb of hop for which we should store info. Vec contain id of the peer having been requested. Note that Vec could be bigger than int due to multiple peer query (we obviously store others peer in Vec). This is use when proxying a query to avoid some peer. // TODO variant of last sent where we only limit number of peer (no hop estimation : simplier).


// TODO switch condvar to something with timeout
/// Internal data type to manage query reply
pub enum QReply<P : Peer> {
  /// a local thread is waiting for a reply on a condvar
  /// when querying we start a query and wait on the semaphore (actually a condvar/mutex) for a result
  Local(Condvar),
  /// reply should be forwarded given a query conf.
  Dist(QueryConfMsg<P>),
}

/// Query Priority.
pub type QueryPriority = u8; // TODO rules for getting number of hop from priority -> convert this to a trait with methods.
/// Query ID.
pub type QueryID = String;
/// Query conf used to initiat a query. See `QueryMode`, `QueryChunk` and `LastSentConf`.
pub type QueryConf = (QueryMode, QueryChunk, LastSentConf); // TODO remove if only use once (currently yes)
// TODO put it in an arc : with last sent it could be big, clone look painfull

///  Main infos about a running query. Notably running mode, chunk config, no loop info, priorities
///  and remaing hop (on a new query it is the max number of hop) plus number of query (on each hop
///  number of peer where query should be forwarded).
///  TODO replace by struct
//pub type QueryConfMsg<P : Peer> = (QueryModeMsg<P>, QueryChunk, Option<LastSent<P>>, StoragePriority, u8, u8, QueryPriority, usize);
pub type QueryConfMsg<P> = (QueryModeMsg<P>, QueryChunk, Option<LastSent<P>>, StoragePriority, u8, u8, QueryPriority, usize); // first u8 is remaining nb hop second one is nbquery last uint is the nb or result expected

/////////////////////////////////////
//Query conf utilities  /////////////
/////////////////////////////////////

#[inline]
pub fn get_req_nb_res<P : Peer>(c : &QueryConfMsg<P>) -> usize {
  c.7
}

#[inline]
pub fn get_sprio<P : Peer>(c : &QueryConfMsg<P>) -> StoragePriority {
  c.3
}
#[inline]
pub fn dec_nbhop<P : Peer, QR : DHTRules>(c : & mut QueryConfMsg<P>,qr : &QR) {
  c.4 -= qr.nbhop_dec();
}
#[inline]
pub fn get_nbhop<P : Peer>(c : &QueryConfMsg<P>) -> u8 {
  c.4
}
#[inline]
pub fn get_nbquer<P : Peer>(c : &QueryConfMsg<P>) -> u8 {
  c.5
}
#[inline]
pub fn get_prio<P : Peer>(c : &QueryConfMsg<P>) -> QueryPriority {
  c.6
}


#[derive(Clone)]
///  Query type, it is related to a kind of `Peer` and a `KeyVal`.
/// The query is seen as ok when all peer reply None or the first peer replies something (if number
/// of result needed is one (common case), otherwhise n).
pub enum Query<P : Peer, V : KeyVal> {
  /// Querying for peer. With reply info, current query reply value (initiated to None the second
  /// pair value is the number of replies send (or to send)) and the possible query timeout (a
  /// must have for managed query).
  PeerQuery(Arc<(QReply<P>, Mutex<(Option<Arc<P>>, usize)>, Option<CachePolicy>)>),
  /// Querying for KeyVal. Same as `PeerQuery`, with an additional storage policy (pair is local
  /// plus possible timeout for cache). Typically storage policiy is used to automatically store
  /// on query with one result needed only, otherwhise application may choose the right result
  /// and storage may happen later.
  KVQuery(Arc<(QReply<P>, Mutex<(Vec<Option<V>>, usize)>, Option<CachePolicy>, (bool, Option<CachePolicy>), usize)>),
}  // boolean being pending or not (if not pending and option at none : nothing were found : replace by semaphore // TODO option is not the right type) - TODO replace duration by start time!!
// to free all semaphore of a query

/// Query methods
impl<P : Peer, V : KeyVal> Query<P, V> {

  #[inline]
  /// Reply with current query value
  pub fn release_query // TODO refactor to use sender only when needed eg two function or option (here we clone sender a lot for nothing)
  (& self, 
   sp : &Sender<PeerMgmtMessage<P,V>>
  )
  where PeerMgmtMessage<P,V> : Send {
    debug!("Query Full release");
    match self {
      &Query::PeerQuery(ref s) => {
        let mut mutg = s.1.lock().unwrap();
        (*mutg).1 = 0;
        match(s.0){
          QReply::Local(ref cv) => {
            cv.notify_all();
          },
          QReply::Dist(ref conf) => {
            sp.send(PeerMgmtMessage::StoreNode((*conf).clone(),(*mutg).0.clone()));
          },
        };
      },
      &Query::KVQuery(ref s) => {
        let mut mutg = s.1.lock().unwrap();
        (*mutg).1 = 0;
        match(s.0) {
          QReply::Local(ref cv) => {
            cv.notify_all();
          },
          QReply::Dist(ref conf) => {
            // Send them result one by one
            for v in (*mutg).0.iter() {
              sp.send(PeerMgmtMessage::StoreKV((*conf).clone(),v.clone()));
            }
          },
        };
      },
    };
  }

#[inline] // TODO closure to avoid redundant code??
// return true if unlock query (so that cache man know it can remove its query
/// Remove one peer to wait on, if no more peer query is released
pub fn lessen_query
 (&self, 
  i : usize, 
  sp : &Sender<PeerMgmtMessage<P,V>>)
 -> bool
 where PeerMgmtMessage<P,V> : Send {
  debug!("Query lessen {:?}", i);
  match self {
    &Query::PeerQuery(ref s) => {
      let mut mutg = s.1.lock().unwrap();
      let nowcount = if i < (*mutg).1 {
        (*mutg).1 - i
      } else {
        0
      };
      (*mutg).1 = nowcount;
      // if it unlock
      if (nowcount == 0) {
        match(s.0){
          QReply::Local(ref cv) =>
               {cv.notify_all();},
          QReply::Dist(ref conf) =>{
             sp.send(PeerMgmtMessage::StoreNode((*conf).clone(),(*mutg).0.clone()));
          },
        };
        true
      } else {
        false
      }
    },
    &Query::KVQuery(ref s) => {
      let mut mutg = s.1.lock().unwrap();
      let nowcount = if i < (*mutg).1 {
        (*mutg).1 - i
      } else {
        0
      };
      (*mutg).1 = nowcount;
      // if it unlock
      if (nowcount == 0){
        match(s.0){
          QReply::Local(ref cv) =>
               {cv.notify_all();},
          QReply::Dist(ref conf) => {
            for v in (*mutg).0.iter() {
              sp.send(PeerMgmtMessage::StoreKV((*conf).clone(),v.clone()));
            }
          },
        };
        true
      } else {
        false
      }
    },
  }
}

#[inline]
/// Update query result. If the query is keyval result, the value is send to its KeyVal storage.
/// It return true if we got enough result, otherwhise false.
pub fn set_query_result (&self, r: Either<Option<Arc<P>>,Option<V>>,
  sv : &Sender<KVStoreMgmtMessage<P,V>>) -> bool { // TODO switch to two function set_qu_res_peer and val
  debug!("Query setresult");
  match self {
    &Query::PeerQuery(ref q) => {
      (*q.1.lock().unwrap()).0 = r.left().unwrap();
      true
    },
    &Query::KVQuery(ref q) => {
      let mut avec = q.1.lock().unwrap();
      avec.0.push(r.clone().right().unwrap());
      let res = avec.0.len() >= q.4;
      match q.3 {
        (true,_) | (_,Some(_)) => {
          match r.right().unwrap() {
            None => {},
            Some(r) =>{
              sv.send(KVStoreMgmtMessage::KVAdd(r,None,q.3)); 
            },
          }
    // no sync (two consecutive query might go network two time
    // TODO sync to avoid it ??? probably for querying node, less for proxying nodes
    // sync will involve release done here in another thread
        },
        _ => {},
      };
      res
    },
  }
}

#[inline]
// TODO try remove pair result when all is stable + closure to avoid dup code
///  for local query, blocking wait for a result (either peer or keyval).
pub fn wait_query_result (&self) -> Either<Option<Arc<P>>,Vec<Option<V>>> { // TODO switch to two function set_qu_res_peer and val
  match self {
    &Query::PeerQuery(ref s) => {
      let r = match s.0 {
        QReply::Local(ref cv) => {
          debug!("Query wait");
          let mut l = s.1.lock().unwrap();
          debug!("Query l is {:?}", l.1);
          while l.1 > 0 {
            debug!("Query l is {:?}", l.1);
            l = cv.wait(l).unwrap();
          }
          debug!("Query Wait finished");
          l.0.clone()
        },
        _ => {
          error!("waiting result on non condvar non local query"); // TODO up local query to (Qreply) to query type -> single level match
          None
        },
      };
      Either::Left(r)
    },
    &Query::KVQuery(ref s) => {
      let r = match s.0 {
      QReply::Local(ref cv) => {
        debug!("Query wait");
        let mut l = s.1.lock().unwrap();
        debug!("Query l is {:?}", l.1);
        while l.1 > 0 {
          debug!("Query l is {:?}", l.1);
          l = cv.wait(l).unwrap();
        }
        debug!("Query Wait finished");
        l.0.clone()
      },
      _ => {
        error!("waiting result on non condvar non local query"); // TODO up local query to (Qreply) to query type -> single level match
        vec!()
      },
      };
      Either::Right(r)
    },
  }
}

#[inline]
/// Get expire date for query (used by cleaning process of query cache).
pub fn get_expire(&self) -> Option<Timespec> {
  match self {
    &Query::PeerQuery(ref q) => (q.2).map(|c|c.0.clone()),
    &Query::KVQuery(ref q) =>  (q.2).map(|c|c.0.clone()),
  }
}

#[inline]
/// Update expire date of query.
pub fn set_expire(&mut self, expire : Timespec) {
  let mut d = match self {
    &mut Query::PeerQuery(ref q) => q.2,
    &mut Query::KVQuery(ref q) => q.2,
  };
  d = Some(CachePolicy(expire));
}

}

#[inline]
/// Utility function to create a query.
/// Warning, this could be slow (depends upon query manager implementation)
pub fn init_query<P : Peer + Send, V : KeyVal + Send> 
 (semsize : usize,
 nbresp   : usize,
 lifetime : Duration, 
 s : & Sender<QueryMgmtMessage<P, V>>, 
 replyto : Option<QueryConfMsg<P>>, 
 managed : Option<QueryID>,
// senthist : Option<LastSent<P>>, 
// storepol : (bool,Option<CachePolicy>),
 peerquery : Option<(bool,Option<CachePolicy>)>) 
-> Query<P,V> 
where P::Key : Send, QueryMgmtMessage<P,V> : Send  {
  let expire = CachePolicy(time::get_time() + lifetime);
  let q : Query<P,V> = match peerquery { 
    None => {
      let query = match replyto {
        Some(node) => 
          Arc::new((QReply::Dist(node), Mutex::new((None, semsize)),Some(expire))),
        None =>
          Arc::new((QReply::Local(Condvar::new()),Mutex::new((None, semsize)),Some(expire))),
       };
       Query::PeerQuery(query.clone())
    },
    Some(storeconf) => {
      let query = match replyto {
        Some(node) => 
          Arc::new((QReply::Dist(node), Mutex::new((vec!(), semsize)),Some(expire),storeconf, nbresp)),
        None =>
          Arc::new((QReply::Local(Condvar::new()),Mutex::new((vec!(), semsize)),Some(expire),storeconf,nbresp)),
        };
        Query::KVQuery(query.clone())
    },
  };

  // add query in query manager unless we use proxy mode (unmanaged query
  match managed {
    Some(qid) => {
      let asem = Arc::new(Semaphore::new(0));
      let mess : QueryMgmtMessage<P,V> = QueryMgmtMessage::NewQuery(qid, q.clone(), asem.clone());
      s.send(mess);
      // wait added
      asem.acquire(); // TODO this is a bottleneck (also in asynch) think about a way to keep query reply in query manager when no query xisting -> like adding it before any send of PeerMgmtMessage::PeerFind( -> just search = at the time of query init : add query manager channel in param of query init function!!!!!
    },
    _ => (),
  };

  q
}
/*
// for dist query
#[inline]
pub fn get_origin_queryID
<'a, P : Peer, V : KeyVal>
(q : &'a Query<P,V>) -> Option<&'a QueryID> {
    match q.0 {
      QReply::Local(_) => None,
      QReply::Dist((ref conf,_)) => conf.get_qid_ref(),
    }
}*/



// variant of query mode to use as a configuration in application
#[derive(RustcDecodable,RustcEncodable,Debug,Clone)]
/// Query Mode defines the way our DHT communicate, it is very important.
/// Unless messageencoding does not serialize it, all query mode could be used. 
/// For some application it could be relevant to forbid the reply to some query mode : 
/// TODO implement a filter in server process (and client proxy).
pub enum QueryMode{ 
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
  // TODO new meeting point mode (involve asking (getting or setting meeting point as kv request) for meeting point and storing them in query). meeting point being only proxy making node id translation (possible n meeting point) -> kind of designing random routes. (meeting points do not read content just change dest/origin (no local search (content may be unreadable for the meeting points))). meeting point may communicate their routing table and when sending we creat a n route (each hop decriptable only by reader(and with a reply route (not same as query (and same thing preencoded)) for final dest)). -> in fact predesigned route with encoded/decoded each hop and if a hop do not know next, reply fail (may need some routing table publish to lower those fails). PB from size of frame you get nbhop... -> Random size message filler...??? so you probabilistic know your nb hop without knowing.
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
    /// Get peers to reply to if the mode allows it.
    pub fn get_rec_node(self) -> Option<Arc<P>> {
        match self {
            QueryModeMsg::AProxy (n, _) => Some (n),
            QueryModeMsg::Asynch (n, _) => Some (n),
            QueryModeMsg::AMix (_,n, _) => Some (n),
        }
    }
    /// Get queryid if the mode use managed query.
    pub fn get_qid (self) -> Option<QueryID> {
        match self {
            QueryModeMsg::AProxy (_, q) => Some (q),
            QueryModeMsg::Asynch (_, q) => Some (q),
            QueryModeMsg::AMix (_,_, q) => Some (q),
        }
    }
    /// Copy conf with a new qid and peer to reply to : when proxying a managed query we do not use the previous id.
    pub fn new_hop<p : Peer> (self, p : Arc<P>, qid : QueryID) -> Self {
        match self {
            QueryModeMsg::AProxy (_, _) => QueryModeMsg::AProxy (p, qid),
            QueryModeMsg::Asynch (_, _) => QueryModeMsg::Asynch (p, qid),
            QueryModeMsg::AMix (a,_, _) => QueryModeMsg::AMix (a,p, qid),
        }
    }
    /// Copy conf with a new qid : when proxying a managed query we do not use the previous id.
    pub fn new_qid (self, qid : QueryID) -> Self {
        match self {
            QueryModeMsg::AProxy (a, _) => QueryModeMsg::AProxy (a, qid),
            QueryModeMsg::Asynch (a, _) => QueryModeMsg::Asynch (a, qid),
            QueryModeMsg::AMix (a,b, _) => QueryModeMsg::AMix (a,b, qid),
        }
    }
    /// Copy conf with a new  andpeer to reply to : when proxying a managed query we do not use the previous id.
    pub fn new_peer<p : Peer> (self, p : Arc<P>) -> Self {
        match self {
            QueryModeMsg::AProxy (_, a) => QueryModeMsg::AProxy (p, a),
            QueryModeMsg::Asynch (_, a) => QueryModeMsg::Asynch (p, a),
            QueryModeMsg::AMix (a,_, b) => QueryModeMsg::AMix (a,p, b),
        }
    }
    /// Get the query id of a managed query.
    fn get_qid_ref (& self) -> Option<&QueryID> {
        match self {
            &QueryModeMsg::AProxy (_, ref q) => Some (q),
            &QueryModeMsg::Asynch (_, ref q) => Some (q),
            &QueryModeMsg::AMix (_,_, ref q) => Some (q),
        }
    }

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


