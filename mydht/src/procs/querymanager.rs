//! Query manager is the process used to manage query persistence.
//! This is mainly for asynch mode (proxy mode do not need to store a query
//! since we immediatly wait for a reply to each query).
//! This process use a `query::cache` implementation. A clean job could run
//! cleaning periodically expired query.




use query::cache::{cache_clean,QueryCache};
//use std::sync::{Semaphore,Arc};
use std::sync::mpsc::{Sender,Receiver};
use time::Duration;
use num::traits::ToPrimitive;
use std::thread;
//use peer::Peer;
use mydhtresult::{Error,ErrorKind};
use mydhtresult::Result as MDHTResult;
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
  cni : F, 
  cleanjobdelay : Option<Duration>) -> MDHTResult<()> {
  let mut res = Ok(());
  let mut cn = match cni() {
    Some(s) => s,
    None => {
      error!("query man initialization failed");
      return Err(Error::new_from("query man  failure".to_string(), ErrorKind::StoreError, res.err()));
    },
  };


  // start clean job if needed
  match cleanjobdelay {
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
              warn!("ending querymanager clean timer, the associated querymanager is not accessible : {}", e);
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
      Ok(m) => {
        match querymanager_internal::<RT,_>(&p, &k, &mut cn, m) {
          Ok(true) => (),
          Ok(false) => {
            info!("exiting cleanly from queryman");
            break
          },
          Err(e) => {
            error!("ending queryman on error : {}",e);
            res = Err(e);
            break
          }, 
        };
      },
      Err(e) => {
        error!("queryman channel reception pb, ending queryman : {}", e); // TODO something to relaunch store
        res = Err(Error::from(e));
        break
      },
    };
  };
 

  // no clean up (transient)
  res
}


pub fn querymanager_internal 
 <RT : RunningTypes,
  CN : QueryCache<RT::P,RT::V>,
  >
 (
  p : &Sender<PeerMgmtMessage<RT::P,RT::V,<RT::T as Transport>::ReadStream,<RT::T as Transport>::WriteStream>>, 
  k : &Sender<KVStoreMgmtMessage<RT::P,RT::V>>,
  cn : &mut CN, 
  m : QueryMgmtMessage<RT::P,RT::V>
  ) -> MDHTResult<bool> {

  match m {
    QueryMgmtMessage::NewQuery(query, mut msg) => {
      // store a new query, forward to peermanager
      // Add query to cache
      let qid = cn.newid(); 
      // update query with qid
      msg.change_query_id(qid.clone());
      debug!("adding query to cache with id {:?}", qid);
      cn.query_add(qid, query);
      // Unlock parent thread (we cannot send query if not in cache already) 
      // TODO send queryid instead and create lessen message
      try!(p.send(msg.to_peermsg()));
    },
    QueryMgmtMessage::NewReply(qid, reply) => {
      // update query if xisting
      let rem = cn.query_update(&qid, move |mut query|{
          if match reply {
            (r@Some(_),_) => {
              query.set_query_result(Either::Left(r),k)
            },
            (_,r@Some(_)) => {
              query.set_query_result(Either::Right(r),k)
            },
            _ => {
              query.lessen_query(1, p)
            },
          } {
            Err(Error("".to_string(), ErrorKind::ExpectedError,None))
          } else {
            Ok(())
          }

      }).is_err();
      if rem {
        let query = cn.query_remove(&qid);
        query.map(|q|q.release_query(p));
      }
    },
    QueryMgmtMessage::PerformClean => {
      debug!("Query manager called for cache clean");
      cache_clean(cn,p);
    },
    QueryMgmtMessage::Lessen(qid, nb) => {
      let rem = cn.query_update(&qid, move |mut query|{
        if query.lessen_query(nb, p) {
            Err(Error("".to_string(), ErrorKind::ExpectedError,None))
          } else {
            Ok(())
          }

      }).is_err();
      if rem {
        let query = cn.query_remove(&qid);
        query.map(|q|q.release_query(p));
      };
    },
    QueryMgmtMessage::Release(qid) => {
        let query = cn.query_remove(&qid);
        query.map(|q|q.release_query(p));
    },
  };
  Ok(true)
}
