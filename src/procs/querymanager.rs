//! Query manager is the process used to manage query persistence.
//! This is mainly for asynch mode (proxy mode do not need to store a query
//! since we immediatly wait for a reply to each query).
//! This process use a `query::cache` implementation. A clean job could run
//! cleaning periodically expired query.



use query::{self,QueryID, Query};
use query::cache::{cache_clean,QueryCache};
use std::sync::{Semaphore,Arc};
use std::sync::mpsc::{Sender,Receiver};
use time::Duration;
use num::traits::ToPrimitive;
use std::thread;
use peer::Peer;
use keyval::{KeyVal};
use kvstore::{StoragePriority};
use utils::Either;
use procs::RunningTypes;
use transport::Transport;

use procs::mesgs::{PeerMgmtMessage, KVStoreMgmtMessage, QueryMgmtMessage};

/// TODO replace all pair of optional peer or value to an Either struct enum.

/// Start and run the process
pub fn start
 <RT : RunningTypes,
  CN : QueryCache<RT::P,RT::V>,
  F : FnOnce() -> Option<CN> + Send + 'static,
  >
 (r : &Receiver<QueryMgmtMessage<RT::P,RT::V>>, 
  s : &Sender<QueryMgmtMessage<RT::P,RT::V>>, 
  p : &Sender<PeerMgmtMessage<RT::P,RT::V,<RT::T as Transport>::ReadStream,<RT::T as Transport>::WriteStream>>, 
  k : &Sender<KVStoreMgmtMessage<RT::P,RT::V>>,
  mut cni : F, 
  cleanjobdelay : Option<Duration>) {
  let mut cn = cni().unwrap_or_else(||panic!("query cache initialization failed"));
  // start clean job if needed
  match cleanjobdelay{
    Some(delay) => {
      let delaysp = delay.clone();
      let sp = s.clone();
      thread::spawn(move || {
        loop {
          thread::sleep_ms(delaysp.num_milliseconds().to_u32().unwrap());
          info!("running scheduled clean");
          match sp.send(QueryMgmtMessage::PerformClean) {
            Ok(()) => (),
            Err(e) => {
              warn!("ending querymanager clean timer, the associated querymanager is not accessible");
              break;
            },
          }
        }
      });
    },
    _ => {}
  }
  // start
  loop {
    match r.recv() {
      Ok(QueryMgmtMessage::NewQuery(query, mut msg))  => {
  // store a new query, forward to peermanager
        // Add query to cache
        let qid = cn.newid(); 
        // update query with qid
        msg.change_query_id(qid.clone());
        debug!("adding query to cache with id {:?}", qid);
        cn.query_add(qid, query.clone());
        // Unlock parent thread (we cannot send query if not in cache already) 
        p.send(msg.to_peermsg(query));
      },
      Ok(QueryMgmtMessage::NewReply(qid, reply)) => {
        // get query if xisting
        let rem = match cn.query_get(&qid) {
          Some (query) => {
            match reply {
              (r@Some(_),_) => {
                query.set_query_result(Either::Left(r),k);
                query.release_query(p);
                true
              },
              (_,r@Some(_)) => {
                query.set_query_result(Either::Right(r),k);
                query.release_query(p);
                true
              },
              _ => {
                query.lessen_query(1, p)
              },
            }
          },
          None => {
            warn!("received reply to missing query (expired?) {:?}",qid); 
            false
          },
        };
        if rem {
          cn.query_remove(&qid)};
        },
      Ok(QueryMgmtMessage::PerformClean) => {
        debug!("Query manager called for cache clean");
        cache_clean(& mut cn,p);
      },
      Err(recErr) => {
       // TODO channel error to relaunch a query man??
      },
    }
  }
}

