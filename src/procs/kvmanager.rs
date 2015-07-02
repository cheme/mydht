
//! Process storing keyvalue.
//! It uses a `KVStore` object and reply to instruction given through a dedicated channel.
use rustc_serialize::json;
use procs::mesgs::{self,PeerMgmtMessage,ClientMessage,KVStoreMgmtMessage};
use peer::{PeerMgmtRules, PeerPriority};
use procs::{RunningContext, RunningProcesses};
use std::sync::mpsc::{Sender,Receiver};
use std::str::from_utf8;
use procs::client;
use std::collections::{HashMap,BTreeSet};
use std::sync::{Arc,Semaphore,Condvar,Mutex};
use query::{self,QueryRules,QueryModeMsg};
use route::Route;
use std::sync::mpsc::channel;
use std::thread::Thread;
use transport::TransportStream;
use transport::Transport;
use peer::Peer;
use utils;
use utils::Either;
use kvstore::{KVStore,StoragePriority};
use keyval::{KeyVal};
use msgenc::MsgEnc;
use num::traits::ToPrimitive;

// Multiplexing kvstore can be fastly bad (only one process).
// Best should be a dedicated dispatch process with independant code : TODO this is ok : aka
// kvmanager selector

/// Start kvmanager process using a specific store.
pub fn start
 <P : Peer,
  V : KeyVal,
  R : PeerMgmtRules<P,V>,
  Q : QueryRules,
  E : MsgEnc,
  S : KVStore<V>,
  F : FnOnce() -> Option<S> + Send + 'static,
  T : Transport> 
 (rc : RunningContext<P,V,R,Q,E,T>, 
  mut storei : F, 
  r : &Receiver<KVStoreMgmtMessage<P,V>>,
  rp : RunningProcesses<P,V>,
  sem : Arc<Semaphore>) {
  // actula store init - TODO better error management
  let mut store = storei().unwrap_or_else(||panic!("store initialization failed"));
  loop {
    match r.recv() {
      Ok(KVStoreMgmtMessage::KVAddPropagate(kv,ares,stconf)) => {
        info!("Adding kv : {:?} from {:?}", kv, rc.0);
        let remhop = query::get_nbhop(&stconf);
        let qp = query::get_prio(&stconf);
        let qps = query::get_sprio(&stconf);
        let esthop = (rc.2.nbhop(qp) - remhop).to_usize().unwrap();
        let storeconf = rc.2.do_store(true, qp, qps, Some(esthop)); // first hop

        let hasnode = match store.get_val(&kv.get_key()) {
          Some(_) => {true},
          None => {false},
        };
        let res = if !hasnode {
          info!("val added");
          store.add_val(kv,storeconf);
          true
        } else {
          info!("val existing : not added");
          // TODO compare value + recheck for right one ??? or add peer signing this value to
          // wrong after rerequesting -> aka send to conflict mgmt process
          // TODO also some update of stconf could happen : longer cache, present cache
          // -> TODO a function for status better than hasnode : return in persistent and in cache
          // or simply send kvadd and implement those updates in kvadd???
          true
        };
        // TODO all propagate stuff!!! Do it here because quite convenient (from query release and
        // other)
        ares.map(|ares|{
          utils::ret_one_result(ares, res);
        });
      },
      Ok(KVStoreMgmtMessage::KVAdd(kv,ares,storeconf)) => {
        info!("Adding kv : {:?} from {:?}", kv, rc.0);
        let hasnode = match store.get_val(&kv.get_key()) { // TODO new function has_val??
          Some(_) => {true},
          None => {false},
        };
        let res = if(!hasnode){
           info!("val added");
           store.add_val(kv,storeconf);
           true
        } else {
          info!("val existing : not added");
          true
        };
        ares.map(|ares|{
          utils::ret_one_result(ares, res);
        });
      },
      // Local find
      Ok(KVStoreMgmtMessage::KVFindLocally(key, ares)) => {
        let res = store.get_val(&key);
        ares.map(|ares|{
          utils::ret_one_result(ares, res);
        });
      },
      //asynch find
      Ok(KVStoreMgmtMessage::KVFind(key, None, queryconf)) => {
        match store.get_val(&key) {
          Some(val) => {
            rp.0.send(PeerMgmtMessage::StoreKV(queryconf, Some(val.clone())));
          },
          None => {
            if query::get_nbhop(&queryconf) > 0 {
              // proxy
              rp.0.send(PeerMgmtMessage::KVFind(key, None, queryconf));
            } else {
              // no result
              rp.0.send(PeerMgmtMessage::StoreKV(queryconf, None));
            };
          },
        };
      },
      Ok(KVStoreMgmtMessage::KVFind(key, Some(query), queryconf)) => {
        let success = match store.get_val(&key) {
          Some(val) => {
            debug!("!!!KV Found match!!! no proxying");
            let querysp = query.clone();
            let ssp = rp.0.clone();
            if querysp.set_query_result(Either::Right(Some(val.clone())),&rp.2) {
              query.release_query(& rp.0);
              true
            } else {
              // no lessen on local or more results needed : proxy
              false
            }
          },
          None => {
            false
          },
        };
        if !success {
          if query::get_nbhop(&queryconf) > 0 {
            debug!("!!!KV not Found match or multiple result needed !!! proxying");
            rp.0.send(PeerMgmtMessage::KVFind(key, Some(query), queryconf));
          } else {
             query.release_query(& rp.0);
          };
        };
      },
      Ok(KVStoreMgmtMessage::Commit) => {
        store.commit_store();
      },
      Ok(KVStoreMgmtMessage::Shutdown) => {
        break
      },
      Err(pois) => {
        error!("channel pb"); // TODO something to relaunch store
      },
    };
  };
  // write on quit
  store.commit_store();
  sem.release();
}

