use std::sync::{Arc,Mutex,Condvar};
use rustc_serialize::{Encoder,Encodable,Decoder};
//use peer::{PeerPriority};
use time::Duration;
use time::{self,Timespec};
use peer::Peer;
use std::sync::mpsc::{Sender};
use procs::mesgs::{KVStoreMgmtMessage,PeerMgmtMessage,QueryMgmtMessage};
use query::cache::CachePolicy;
use std::collections::VecDeque;
//use procs::RunningProcesses;
use keyval::{KeyVal};
use kvstore::{StoragePriority};
use utils::Either;
//use num::traits::ToPrimitive;
use rules::DHTRules;
use transport::{ReadTransportStream,WriteTransportStream};

pub mod cache;
pub mod simplecache;


// TODO all sender peermgmt replaced by client handle

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
#[derive(Clone)]
/// Internal data type to manage query reply
pub enum QReply<P : Peer,V> {
  /// a local thread is waiting for a reply on a condvar
  /// when querying we start a query and wait on the semaphore (actually a condvar/mutex) for a result
  Local(QRepLoc<V>),
  /// reply should be forwarded given a query conf.
  Dist(QueryMsg<P>,V),
}

type QRepLoc<V> = Arc<(Condvar, Mutex<V>)>;

#[derive(Clone)]
/// in process if there is a query handle, the query is not in query manager and reply could be use
/// directly through the query.
/// Query handle is clonable when query is not.
pub enum QueryHandle<P : Peer, V : KeyVal> {
  LocalP(QRepLoc<(Option<Arc<P>>,usize)>),
  LocalV(QRepLoc<(Vec<Option<V>>,usize)>),
  QueryManager(QueryID),
  NoHandle, // for direct asynch reply directly to peer
}

/// Query Priority.
pub type QueryPriority = u8; // TODO rules for getting number of hop from priority -> convert this to a trait with methods.
/// Query ID.
pub type QueryID = usize;

pub struct QueryConf {
  pub mode : QueryMode,
  pub chunk : QueryChunk,
  pub hop_hist : LastSentConf,
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


//#[derive(Clone)]
///  Query type, it is related to a kind of `Peer` and a `KeyVal`.
/// The query is seen as ok when all peer reply None or the first peer replies something (if number
/// of result needed is one (common case), otherwhise n).
/// TODO !!! move arc mutex in qreply (and non mutex/arc for dist query), plus remove arc
pub enum Query<P : Peer, V : KeyVal> {
  /// Querying for peer. With reply info, current query reply value (initiated to None the second
  /// pair value is the number of replies send (or to send)) and the possible query timeout (a
  /// must have for managed query).
  PeerQuery(QReply<P,(Option<Arc<P>>,usize)>, Option<CachePolicy>),
  /// Querying for KeyVal. Same as `PeerQuery`, with an additional storage policy (pair is local
  /// plus possible timeout for cache). Typically storage policiy is used to automatically store
  /// on query with one result needed only, otherwhise application may choose the right result
  /// and storage may happen later.
  KVQuery(QReply<P,(Vec<Option<V>>,usize)>, Option<CachePolicy>, (bool, Option<CachePolicy>), usize),
}  // boolean being pending or not (if not pending and option at none : nothing were found : replace by semaphore // TODO option is not the right type) - TODO replace duration by start time!!
// to free all semaphore of a query

impl<P : Peer, V : KeyVal> QueryHandle<P, V> {
  #[inline]
  /// release query
  pub fn release_query<TR : ReadTransportStream, TW : WriteTransportStream>
  (self, 
   sp : &Sender<PeerMgmtMessage<P,V,TR,TW>>,
   sq : &Sender<QueryMgmtMessage<P,V>>,
  )
  where PeerMgmtMessage<P,V,TR,TW> : Send {
    debug!("Query handle Full release");
    match self {
      QueryHandle::LocalP(cv) => {
        let mut mutg = cv.1.lock().unwrap();
        (*mutg).1 = 0;
        cv.0.notify_all();
      },
      QueryHandle::LocalV(cv) => {
        let mut mutg = cv.1.lock().unwrap();
        (*mutg).1 = 0;
        cv.0.notify_all();
      },
      QueryHandle::QueryManager(qid) => {
        sq.send(QueryMgmtMessage::Release(qid)).unwrap();
      },
      QueryHandle::NoHandle => {
        panic!("release on nohandle")
      },
    };
  }

#[inline] // TODO closure to avoid redundant code??
/// lessen query
pub fn lessen_query<TR : ReadTransportStream, TW : WriteTransportStream>

 (&self,
  i : usize, 
  sp : &Sender<PeerMgmtMessage<P,V,TR,TW>>,
  sq : &Sender<QueryMgmtMessage<P,V>>)
 -> bool
 where PeerMgmtMessage<P,V,TR,TW> : Send {
  debug!("Query lessen {:?}", i);
  match self {

    &QueryHandle::LocalP(ref s) => {
      let mut mutg = s.1.lock().unwrap();
      let nowcount = if i < (*mutg).1 {
        (*mutg).1 - i
      } else {
        0
      };
      (*mutg).1 = nowcount;
      nowcount == 0
    },
    &QueryHandle::LocalV(ref s) => {
      let mut mutg = s.1.lock().unwrap();
      let nowcount = if i < (*mutg).1 {
        (*mutg).1 - i
      } else {
        0
      };
      (*mutg).1 = nowcount;
      nowcount == 0
    },
    &QueryHandle::QueryManager(ref qid) => {
      sq.send(QueryMgmtMessage::Lessen(qid.clone(),i)).unwrap();
      false // release will be triggered in queryman
    },
    &QueryHandle::NoHandle => {
      panic!("lessen on nohandle")
    },
 
  }
}

#[inline]
///  for local query, blocking wait for a result (either peer or keyval). // TODO timeout on wait   with returning content as in return result (plus spurious evo : see onersult)
pub fn wait_query_result (&self) -> Either<Option<Arc<P>>,Vec<Option<V>>> { // TODO switch to two function set_qu_res_peer and val
  match self {
    &QueryHandle::QueryManager(ref qid) => {
      panic!("wait over querymanager is not permitted");
    },
    &QueryHandle::NoHandle => {
      panic!("wait over nohandle is not permitted");
    },
    &QueryHandle::LocalP(ref cv) => {
          debug!("Query wait");
          let mut l = cv.1.lock().unwrap();
          debug!("Query l is {:?}", l.1);
          while l.1 > 0 {
            debug!("Query l is {:?}", l.1);
            l = cv.0.wait(l).unwrap();// TODO spurious catch with condition over either None and 0 waiting or not none
          }
          debug!("Query Wait finished");
      Either::Left(l.0.clone())
    },
    &QueryHandle::LocalV(ref cv) => {
        debug!("Query wait");
        let mut l = cv.1.lock().unwrap();
        debug!("Query l is {:?}", l.1);
        while l.1 > 0 {
          debug!("Query l is {:?}", l.1);
          l = cv.0.wait(l).unwrap();// TODO spurious catch with condition over either None and 0 waiting or not none
        }
        debug!("Query Wait finished");
        Either::Right(l.0.clone())
    },
  }
}



}
/// Query methods
impl<P : Peer, V : KeyVal> Query<P, V> {
   
  pub fn is_local(&self) -> bool {
    match self {
      &Query::PeerQuery(QReply::Local(..), _) => true,
      &Query::KVQuery(QReply::Local(..),_,_,_) => true,
      _ => false,
    }
  }

  /// get handle for query.
  /// Method does no work when in Asynch mode on dist.
  /// Result should not be send to client (client use querymanager handle or no handle (not local
  /// because at this point qreply local are in querymanager)
  pub fn get_handle(&self) -> QueryHandle<P,V> {
    match self {
      &Query::PeerQuery(QReply::Local(ref arc), _) => {
        QueryHandle::LocalP(arc.clone())
      },
      &Query::KVQuery(QReply::Local(ref arc), _,_,_) => {
        QueryHandle::LocalV(arc.clone())
      },
      &Query::PeerQuery(QReply::Dist(ref qmode, _), _) => {
        QueryHandle::QueryManager(qmode.get_query_id())
      },
      &Query::KVQuery(QReply::Dist(ref qmode, _), _,_,_) => {
        QueryHandle::QueryManager(qmode.get_query_id())
      },
    }
  }

  #[inline]
  /// Reply with current query value TODO might consume query (avoiding some clone and clearer
  /// semantic : check this)!! TODO right now error results in panic, rightfull err mgmet could be
  /// nice
  pub fn release_query<TR : ReadTransportStream, TW : WriteTransportStream>
  (self, 
   sp : &Sender<PeerMgmtMessage<P,V,TR,TW>>
  )
  where PeerMgmtMessage<P,V,TR,TW> : Send {
    debug!("Query Full release");
    match self {
      Query::PeerQuery(s,_) => {
        match s {
          QReply::Local(cv) => {
            let mut mutg = cv.1.lock().unwrap();
            (*mutg).1 = 0;
            cv.0.notify_all();
          },
          QReply::Dist(conf,v) => {
            sp.send(PeerMgmtMessage::StoreNode(conf,v.0)).unwrap();
          },
        };
      },
      Query::KVQuery(s,_,_,_) => {
        match s {
          QReply::Local(cv) => {
            let mut mutg = cv.1.lock().unwrap();
            (*mutg).1 = 0;
            cv.0.notify_all();
          },
          QReply::Dist(conf,v) => {
            // Send them result one by one
            for v in v.0.into_iter() {
              sp.send(PeerMgmtMessage::StoreKV(conf.clone(), v)).unwrap();
            }
          },
        };
      },
    };
  }

#[inline] // TODO closure to avoid redundant code??
// return true if unlock query (so that cache man know it can remove its query
/// Remove one peer to wait on
/// return true if there is no remaining query to wait for (so we can release)
pub fn lessen_query<TR : ReadTransportStream, TW : WriteTransportStream>

 (&mut self, 
  i : usize, 
  sp : &Sender<PeerMgmtMessage<P,V,TR,TW>>)
 -> bool
 where PeerMgmtMessage<P,V,TR,TW> : Send {
  fn minus_val_is_zero(initval : &mut usize, minus : usize) -> bool {
    let nowcount = if minus < *initval {
      *initval - minus
    } else {
      0
    };
    *initval = nowcount;
    nowcount == 0
  }

  debug!("Query lessen {:?}", i);
  match self {
    &mut Query::PeerQuery(QReply::Local(ref cv),_) => {
          let mut c = &mut cv.1.lock().unwrap().1;
          minus_val_is_zero(c, i)
    },
    &mut Query::PeerQuery(QReply::Dist(_, ref mut v),_) => {
          minus_val_is_zero(&mut v.1, i)
    },
    &mut Query::KVQuery(QReply::Local(ref cv),_,_,_) => {
          let mut c = &mut cv.1.lock().unwrap().1;
          minus_val_is_zero(c, i)
    },
    &mut Query::KVQuery(QReply::Dist(_, ref mut v),_,_,_) => {
          minus_val_is_zero(&mut v.1, i)
    },
  }
}
#[inline]
/// Update query result. If the query is keyval result, the value is send to its KeyVal storage.
/// It return true if we got enough result, otherwhise false.
/// It also lessen query count (one query received)
pub fn set_query_result (&mut self, r: Either<Option<Arc<P>>,Option<V>>,
  sv : &Sender<KVStoreMgmtMessage<P,V>>) -> bool { // TODO switch to two function set_qu_res_peer and val
  debug!("Query setresult");
  // do store to kvstore if needed
  match self {
    &mut Query::KVQuery(_,_,do_store,_) => {
      match do_store {
        (true,_) | (_,Some(_)) => {
          match r.right_ref() {
            Some(&Some(ref r)) => {
              sv.send(KVStoreMgmtMessage::KVAdd(r.clone(),None,do_store)).unwrap(); 
            },
            _ => {},
          }
    // no sync (two consecutive query might go network two time
    // TODO sync to avoid it ??? probably for querying node, less for proxying nodes
    // sync will involve release done here in another thread
        },
        _ => {},
      };

    },
    _ => (),
  };
  // set result
  match self {
    &mut Query::PeerQuery(QReply::Local(ref cv),_) => {
      let mut mutg = cv.1.lock().unwrap();
      (*mutg).0 = r.left().unwrap();
      (*mutg).1 = 0;
      true
    },
    &mut Query::PeerQuery(QReply::Dist(_, ref mut v),_) => {
      v.0 = r.left().unwrap();
      v.1 = 0;
      true
    },
    &mut Query::KVQuery(QReply::Local(ref cv),_,_,ref nbres) => {
      let mut avec = cv.1.lock().unwrap();
      avec.0.push(r.right().unwrap());
      let res = avec.0.len() >= *nbres;
      if avec.1 > 1 {
        avec.1 = avec.1 - 1;
      };
      res
    },
    &mut Query::KVQuery(QReply::Dist(_, ref mut v),_,_,ref nbres) => {
      v.0.push(r.right().unwrap());
      let res = v.0.len() >= *nbres;
      if v.1 > 1 {
        v.1 = v.1 - 1;
      };
      res
    },
  }
}

#[inline]
/// Get expire date for query (used by cleaning process of query cache).
pub fn get_expire(&self) -> Option<Timespec> {
  match self {
    &Query::PeerQuery(_, ref q) => q.map(|c|c.0.clone()),
    &Query::KVQuery(_, ref q, _, _) =>  q.map(|c|c.0.clone()),
  }
}

#[inline]
/// Update expire date of query.
pub fn set_expire(&mut self, expire : Timespec) {
  let mut d = match self {
    &mut Query::PeerQuery(_, ref mut q) => q,
    &mut Query::KVQuery(_, ref mut q, _, _) => q,
  };
  if d.is_some() {
    debug!("overriding expire");
  };
  (*d) = Some(CachePolicy(expire));
}

}

#[inline]
/// Utility function to create a query.
/// Warning, this could be slow (depends upon query manager implementation)
/// TODO remove this : instead msg to forward send and send forward from here to peermanager
/// rename to send_for_query,  rp to param, managed is bool, qid calculated here with update of
/// QueryMsg,  + add peermgmt msg 
/// TODO something more implicit to know if dist or local
pub fn init_query<P : Peer, V : KeyVal> 
 (nbquer : usize,
 nbresp   : usize,
 lifetime : Duration, 
 replyto : Option<QueryMsg<P>>, 
// senthist : Option<LastSent<P>>, 
// storepol : (bool,Option<CachePolicy>),
 peerquery : Option<(bool,Option<CachePolicy>)>) 
-> Query<P,V> {
  let expire = CachePolicy(time::get_time() + lifetime);
  let q : Query<P,V> = match peerquery { 
    None => {
      let query = match replyto {
        Some(qconf) => 
          QReply::Dist(qconf, (None, nbquer)),
        None =>
          QReply::Local(Arc::new((Condvar::new(),Mutex::new((None, nbquer))))),
       };
       // PeerQuery(QReply<P,(Option<Arc<P>>,usize)>, Option<CachePolicy>),
       //Local(QRepLoc<V>),
//  /// reply should be forwarded given a query conf.
//  Dist(QueryMsg<P>,V),
//}

//type QRepLoc<V> = Arc<(Condvar, Mutex<V>)>;


       //
       Query::PeerQuery(query, Some(expire))
    },
    Some(storeconf) => {
      let query = match replyto {
        Some(node) => 
          QReply::Dist(node, (vec!(), nbquer)),
        None =>
          QReply::Local(Arc::new((Condvar::new(),Mutex::new((vec!(), nbquer))))),
        };
        Query::KVQuery(query, Some(expire), storeconf, nbresp)
    },
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
  /// get corresponding querymode
  pub fn get_mode (&self) -> QueryMode {
    match self {
      &QueryModeMsg::AProxy (_, _) => QueryMode::AProxy,
      &QueryModeMsg::Asynch (_, _) => QueryMode::Asynch,
      &QueryModeMsg::AMix (h,_, _) => QueryMode::AMix(h),
    }
   
  }
    /// Get peers to reply to if the mode allows it.
    pub fn get_rec_node(&self) -> Option<&Arc<P>> {
        match self {
            &QueryModeMsg::AProxy (ref n, _) => Some (n),
            &QueryModeMsg::Asynch (ref n, _) => Some (n),
            &QueryModeMsg::AMix (_,ref n, _) => Some (n),
        }
    }
    /// Get queryid if the mode use managed query.
    pub fn get_qid (&self) -> Option<&QueryID> {
        match self {
            &QueryModeMsg::AProxy (_, ref q) => Some (q),
            &QueryModeMsg::Asynch (_, ref q) => Some (q),
            &QueryModeMsg::AMix (_,_, ref q) => Some (q),
        }
    }
    pub fn get_qid_clone (&self) -> QueryID {
        match self {
            &QueryModeMsg::AProxy (_, ref q) => q.clone(),
            &QueryModeMsg::Asynch (_, ref q) => q.clone(),
            &QueryModeMsg::AMix (_,_, ref q) => q.clone(),
        }
    }
    pub fn to_qid (self) -> Option<QueryID> {
        match self {
            QueryModeMsg::AProxy (_, q) => Some (q),
            QueryModeMsg::Asynch (_, q) => Some (q),
            QueryModeMsg::AMix (_,_, q) => Some (q),
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

