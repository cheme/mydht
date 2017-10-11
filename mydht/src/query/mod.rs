use std::sync::{Arc,Mutex,Condvar};
use serde::{Serializer,Serialize,Deserializer};
use serde::de::{DeserializeOwned};
//use peer::{PeerPriority};
use std::time::{Instant,Duration};
use peer::Peer;
use std::sync::mpsc::{Sender};
use procs::mesgs::{KVStoreMgmtMessage,PeerMgmtMessage};
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


// TODO all sender peermgmt replaced by client handle


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

pub type QRepLoc<V> = Arc<(Condvar, Mutex<V>)>;

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
/*
impl<P : Peer, V : KeyVal> QueryHandle<P, V> {
  #[inline]
  /// release query
  pub fn release_query<TR : ReadTransportStream, TW : WriteTransportStream>
  (self, 
   sp : &Sender<PeerMgmtMessage<P,V,TR,TW>>,
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
*/
/// Query methods
impl<P : Peer,V, RP : Ref<P>> Query<P,V,RP> {
   
  pub fn is_local(&self) -> bool {
    if let &Query(_,QReply::Local(..),_) = self {
      true
    } else {
      false
    }
  }
/*
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
*/
/*  #[inline]
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
*/
#[inline] // TODO closure to avoid redundant code??
// return true if unlock query (so that cache man know it can remove its query
/// Remove one peer to wait on
/// return true if there is no remaining query to wait for (so we can release)
pub fn lessen_query
 (&mut self, 
  i : usize) 
//  sp : &Sender<PeerMgmtMessage<P,V,TR,TW>>)
 -> bool
 {
  fn minus_val_is_zero(initval : &mut usize, minus : usize) -> bool {
    let nowcount = if minus < *initval {
      *initval - minus
    } else {
      0
    };
    *initval = nowcount;
    nowcount == 0
  }

    panic!("TODO del ? ");
  //debug!("Query lessen {:?}", i);
/*  match self {
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
  }*/
}
/*
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
*/
#[inline]
/// Get expire date for query (used by cleaning process of query cache).
pub fn get_expire(&self) -> Option<Instant> {
  self.2.clone()
/*  match self {
    &Query::PeerQuery(_, ref q) => q.map(|c|c.0.clone()),
    &Query::KVQuery(_, ref q, _, _) =>  q.map(|c|c.0.clone()),
  }*/
}

#[inline]
/// Update expire date of query.
pub fn set_expire(&mut self, expire : Instant) {
  self.2 = Some(expire)
/*  let mut d = match self {
    &mut Query::PeerQuery(_, ref mut q) => q,
    &mut Query::KVQuery(_, ref mut q, _, _) => q,
  };
  if d.is_some() {
    debug!("overriding expire");
  };
  (*d) = Some(CachePolicy(expire));*/
}

}

#[inline]
/// Utility function to create a query.
/// Warning, this could be slow (depends upon query manager implementation)
/// TODO remove this : instead msg to forward send and send forward from here to peermanager
/// rename to send_for_query,  rp to param, managed is bool, qid calculated here with update of
/// QueryMsg,  + add peermgmt msg 
/// TODO something more implicit to know if dist or local
pub fn init_query<P : Peer, V : KeyVal,RP : Ref<P>> 
 (nbquer : usize,
 nbresp   : usize,
 lifetime : Duration, 
 replyto : Option<()>, 
// senthist : Option<LastSent<P>>, 
// storepol : (bool,Option<CachePolicy>),
 peerquery : Option<(bool,Option<CachePolicy>)>) 
-> Query<P,V,RP> {
  panic!("todel");
/*  let expire = CachePolicy(time::get_time() + lifetime);
  let q : Query<P,V> = match peerquery { 
    None => {
/     let query = match replyto {
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
  q*/
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








