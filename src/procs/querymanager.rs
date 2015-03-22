//! Query manager is the process used to manage query persistence.
//! This is mainly for asynch mode (proxy mode do not need to store a query
//! since we immediatly wait for a reply to each query).
//! This process use a `query::cache` implementation. A clean job could run
//! cleaning periodically expired query.



use query::{self,QueryID, Query};
use query::cache::{cache_clean,QueryCache};
use std::sync::{Semaphore,Arc};
use std::sync::mpsc::{Sender,Receiver};
use std::time::Duration;
use std::old_io::timer::Timer;
use std::thread::Thread;
use peer::Peer;
use kvstore::{KeyVal,StoragePriority};
use utils::Either;

use procs::mesgs::{PeerMgmtMessage, KVStoreMgmtMessage, QueryMgmtMessage};

/// TODO replace all pair of optional peer or value to an Either struct enum.

/// Start and run the process
pub fn start
 <P : Peer,
  V : KeyVal,
  CN : QueryCache<P,V>>
 (r : &Receiver<QueryMgmtMessage<P,V>>, 
  s : &Sender<QueryMgmtMessage<P,V>>, 
  p : &Sender<PeerMgmtMessage<P,V>>, 
  k : &Sender<KVStoreMgmtMessage<P,V>>,
  mut cn : CN, 
  cleanjobdelay : Option<Duration>) {
  // start clean job if needed
  match cleanjobdelay{
    Some(delay) => {
      let delaysp = delay.clone();
      let sp = s.clone();
      Thread::spawn(move || {
        let mut timer = Timer::new().unwrap();
        loop {
          timer.sleep(delaysp);
          info!("running scheduled clean");
          sp.send(QueryMgmtMessage::PerformClean);
        }
      });
    },
    _ => {}
  }
  // start
  loop {
    match r.recv() {
      Ok(QueryMgmtMessage::NewQuery(qid,query, sem))  => {
        debug!("adding query to cache {:?}", qid);
        // Add query to cache
        cn.query_add(&qid, query);
        // Unlock parent thread (we cannot send query if not in cache already) 
        sem.release();
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

