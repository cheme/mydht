
//! Process storing keyvalue.
//! It uses a `KVStore` object and reply to instruction given through a dedicated channel.

use procs::mesgs::{PeerMgmtMessage,PeerMgmtInitMessage,QueryMgmtMessage,KVStoreMgmtMessage};
//use peer::PeerPriority;
use procs::{ArcRunningContext,RunningProcesses,RunningTypes};
use std::sync::mpsc::{Receiver};


//use std::collections::{HashMap,BTreeSet};
//use std::sync::{Arc,Semaphore,Condvar,Mutex};
//use query::QueryModeMsg;
use route::Route;
//use std::sync::mpsc::channel;
//use std::thread::Thread;
//use transport::TransportStream;
//use transport::Transport;
use mydhtresult::{Error,ErrorKind};
use utils;
use utils::Either;
use kvstore::KVStore;
use keyval::{KeyVal};
use mydhtresult::Result as MDHTResult;
use num::traits::ToPrimitive;
use rules::DHTRules;

// Multiplexing kvstore can be fastly bad (only one process).
// Best should be a dedicated dispatch process with independant code : TODO this is ok : aka
// kvmanager selector

/// Start kvmanager process using a specific store.
pub fn start
 <RT : RunningTypes,
  S : KVStore<RT::V>,
  F : FnOnce() -> Option<S> + Send + 'static,
  > 
 (rc : ArcRunningContext<RT>, 
  storei : F, 
  r : &Receiver<KVStoreMgmtMessage<RT::P,RT::V>>,
  rp : RunningProcesses<RT>,
  ) -> MDHTResult<()> {
  let mut res = Ok(());
  // actula store init - TODO better error management
  let mut store = match storei() {
    Some(s) => s,
    None => {
      error!("store initialization failed");
      return Err(Error::new_from("kv init failure".to_string(), ErrorKind::StoreError, res.err()));
    },
  };

  loop {
    match r.recv() {
      Ok(m) => {
        match kvmanager_internal(&rc, &mut store, &rp, m) {
          Ok(true) => (),
          Ok(false) => {
            info!("exiting cleanly from kvmanager");
            break
          },
          Err(e) => {
            error!("ending kvmanager on error : {}",e);
            res = Err(e);
            break
          }, 
        };
      },
      Err(e) => {
        error!("kvman channel reception pb, ending kvman : {}", e); // TODO something to relaunch store
        res = Err(Error::from(e));
        break
      },
    };
  };
  // write on quit
  if !store.commit_store() {
    res = Err(Error::new_from("kv store commit failure".to_string(), ErrorKind::StoreError, res.err()));
  };
  res
}


pub fn kvmanager_internal
 <RT : RunningTypes,
  S : KVStore<RT::V>,
  > 
 (rc : &ArcRunningContext<RT>, 
  store : &mut S, 
  rp : &RunningProcesses<RT>,
  m : KVStoreMgmtMessage<RT::P,RT::V>,
  ) -> MDHTResult<bool> {
  match m {
    KVStoreMgmtMessage::KVAddPropagate(kv,ares,stconf) => {
      info!("Adding kv : {:?} from {:?}", kv, rc.me);
      let remhop = stconf.rem_hop;
      let qp = stconf.prio;
      let qps = stconf.storage;
      let esthop = (rc.rules.nbhop(qp) - remhop).to_usize().unwrap();
      let storeconf = rc.rules.do_store(true, qp, qps, Some(esthop)); // first hop

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
        utils::ret_one_result(&ares, res);
      });
    },
    KVStoreMgmtMessage::KVAdd(kv,ares,storeconf) => {
      info!("Adding kv : {:?} from {:?}", kv, rc.me);
      let hasnode = match store.get_val(&kv.get_key()) { // TODO new function has_val??
        Some(_) => {true},
        None => {false},
      };
      let res = if !hasnode {
         info!("val added");
         store.add_val(kv,storeconf);
         true
      } else {
        info!("val existing : not added");
        true
      };
      ares.map(|ares|{
        utils::ret_one_result(&ares, res);
      });
    },
    // Local find
    KVStoreMgmtMessage::KVFindLocally(key, ares) => {
      let res = store.get_val(&key);
      ares.map(|ares|{
        utils::ret_one_result(&ares, res);
      });
    },
    //asynch find
    KVStoreMgmtMessage::KVFind(key, None, queryconf,_) => {
      match store.get_val(&key) {
        Some(val) => {
          try!(rp.peers.send(PeerMgmtMessage::StoreKV(queryconf, Some(val.clone()))));
        },
        None => {
          if queryconf.rem_hop > 0 {
            // proxy
            try!(rp.peers.send(PeerMgmtMessage::KVFind(key, None, queryconf)));
          } else {
            // no result
            try!(rp.peers.send(PeerMgmtMessage::StoreKV(queryconf, None)));
          };
        },
      };
    },
    KVStoreMgmtMessage::KVFind(key, Some(query), queryconf, dostorequery) => {
      let success = match store.get_val(&key) {
        Some(val) => {
          debug!("!!!KV Found match!!! no proxying");
          let querysp = query.clone();

          if querysp.set_query_result(Either::Right(Some(val.clone())),&rp.store) {
            query.release_query(& rp.peers);
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
        if queryconf.rem_hop > 0 {
          debug!("!!!KV not Found match or multiple result needed !!! proxying");
          // If dostorequery then send to query manager for new query
          if dostorequery {
            try!(rp.queries.send(QueryMgmtMessage::NewQuery(query, PeerMgmtInitMessage::KVFind(key, queryconf))));
          } else {
            // else send directly to peers
            try!(rp.peers.send(PeerMgmtMessage::KVFind(key, Some(query), queryconf)));
          };
        } else {
           query.release_query(& rp.peers);
        };
      };
    },
    KVStoreMgmtMessage::Commit => {
      if !store.commit_store() {
        return Err(Error("Commit store failure !!! exiting".to_string(), ErrorKind::StoreError, None));
      };
    },
    KVStoreMgmtMessage::Shutdown => {
      return Ok(false)
    },
  };
  Ok(true)
}


