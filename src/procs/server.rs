//! Receiving process
//! It is the only process which receive messages (except proxy mode where we could wait for
//! messages in `client` process and ping / pong).
//! With connected transport, a thread is used to receive queries from peers, with non connected
//! transport a thread receive globally spawning short lives process to run server job.



use rustc_serialize::json;
use procs::mesgs::{PeerMgmtMessage,KVStoreMgmtMessage,QueryMgmtMessage};
use msgenc::{ProtoMessage};
use procs::{RunningContext,ArcRunningContext,RunningProcesses,RunningTypes};
use peer::{Peer,PeerMgmtRules, PeerPriority};
use std::str::from_utf8;
use transport::TransportStream;
use transport::Transport;
use std::sync::{Mutex,Semaphore,Arc,Condvar};
use std::sync::mpsc::{Sender,Receiver};
use std::thread;
use query::{QueryConfMsg, QueryRules, QueryMode, QueryPriority, QueryChunk, QueryModeMsg};
use time::Duration;
use query;
use query::{QueryID};
use utils;
use keyval::{Attachment,SettableAttachment};
use keyval::{KeyVal};
use msgenc::{MsgEnc,DistantEncAtt,DistantEnc};
use utils::{send_msg,receive_msg};
use num::traits::ToPrimitive;

// all update of query prio and nbquery and else are not done every where : it is a security issue
/// all update of query conf when receiving something, this is a filter to avoid invalid or network
/// dangerous queries (we apply local conf other requested conf)
pub fn update_query_conf<P : Peer> (qconf  : &mut QueryConfMsg<P>, r : &QueryRules) {
//(qmode, qchunk,lsent,store,remhop,nbquer,qp, rnbres)
  // ensure number of query is not changed
  qconf.5 = r.nbquery(qconf.5); // TODO special function for when hoping?? with initial nbquer in param : eg less and less??
    // TODO add a treshold for remhop
  //  let newremhop = remhop;
    // TODO a treshold for number of res
 //   let newrnbres = rnbres;
    // TODO priority lower function (no need to lower nbquer : only treshold and lower prio)
//    let newqp = qp;
}

/// new query mode to use when proxying a query (not for proxy mode and asynch mode where nothing
/// change).
fn new_query_mode<P : Peer> (qm : &QueryModeMsg<P>, me : &Arc<P>, qid : QueryID) -> QueryModeMsg<P> {
   match qm {
     &QueryModeMsg::AMix(0, ref recnode, ref queryid)  => 
       QueryModeMsg::Asynch(me.clone(), qid),
     &QueryModeMsg::AMix(i, ref recnode, ref queryid)  => 
       QueryModeMsg::AMix(i - 1, me.clone(), qid),
     &QueryModeMsg::AProxy(ref recnode, ref queryid)  => 
       QueryModeMsg::AProxy(me.clone(), qid),
     ref a => (*a).clone(), // should not happen
   }
}

/// Server loop
pub fn servloop <RT : RunningTypes>
 (rc : ArcRunningContext<RT>, 
  rp : RunningProcesses<RT::P,RT::V>) {

    let spserv = |s, co| {
        let pm2 = rp.clone();
        let rcsp = rc.clone();
        thread::spawn (move || {
          request_handler::<RT>(s, &rcsp, &pm2, &co)
        });
    };
    // loop in transport receive function
    rc.transport.receive(&rc.me.to_address(), spserv);
}

/// Spawn thread either for one message (non connected) or for one connected peer (loop on stream
/// receiver).
fn request_handler <RT : RunningTypes>
 (mut s1 : <RT::T as Transport>::Stream, 
  rc : &ArcRunningContext<RT>, 
  rp : &RunningProcesses<RT::P, RT::V>,
  oneonly : &Option<(Vec<u8>, Option<Attachment>)>,
 ) {
  let s = &mut s1;
  let anone = None;
  loop{
    // r is message and oa an optional attachmnet (in pair because a reference owhen disconnected
    // and not when connected).
    let (r, oa) = match oneonly {
      &None => {
        match receive_msg (s, &rc.msgenc) {
          None => {
            info!("Serving connection lost");
            break;
          },
          Some(r) => (r.0,(r.1,&anone)),
        }
      },
      // unconnected
      &Some((ref v, ref oa)) => match rc.msgenc.decode(v.as_slice()) {
         None => {
           error!("invalid message");
           break;
         },
         Some(r) => (r, (None,oa)),
      },
    };
    debug!("{:?} RECEIVED : {:?}", rc.me, r);
    match r {
      ProtoMessage::PING(from, chal, sig) => {
        // check ping authenticity
        if rc.peerrules.checkmsg(&from,&chal,&sig){
          let afrom = Arc::new(from);
          let accept = rc.peerrules.accept(&afrom, &rp, &rc);
          match accept {
            None => {
              warn!("refused node {:?}",afrom);
              rp.peers.send(PeerMgmtMessage::PeerUpdatePrio(afrom, PeerPriority::Blocked));
            },
            Some(pri)=> {
              let repsig = rc.peerrules.signmsg(&(*rc.me), &chal);
              let mess : ProtoMessage<RT::P,RT::V> = if oneonly.is_some() {
                // challenge use as query id
                ProtoMessage::APONG((*rc.me).clone(),chal, repsig)
              } else { 
                // connected
                ProtoMessage::PONG(repsig)
              };
              send_msg(&mess, None, s,&rc.msgenc);
              // add the peer
              rc.peerrules.for_accept_ping(&afrom, &rp, &rc);
              rp.peers.send(PeerMgmtMessage::PeerAdd(afrom, pri));
            }
          }
        } else {
          warn!("invalid ping, closing connection");
          break;
        }
      },
      ProtoMessage::APONG(node,qid,sign) => {
        // TODO  get chal from a global map (special query from querymanager) and check before adding eg newtype of query :
        // sign query or pong query  (+ possible remove some blocking peer afterward) 
        // waiting a signing before doing an action (here sending add peer : if possible
        // use a closure otherwhise unlock a thread (like local query but with signature passing
        // only)).
        // here we do not check that challenge (qid) is the righ one!!!
        let anode = Arc::new(node);
        let accept = rc.peerrules.accept(&anode, &rp, &rc);
        match accept {
           Some(pri) => {
             rp.peers.send(PeerMgmtMessage::PeerAdd(anode,pri));
           },
           None => (),
        };
      },
      // asynch mode we do not need to keep trace of the query
      ProtoMessage::FIND_NODE(mut qconf@(QueryModeMsg::Asynch(_,_), _,_,_,_,_,_,_), nid) => {
        update_query_conf (&mut qconf ,&rc.queryrules);
        debug!("Asynch Find peer {:?}", nid);
        rp.peers.send(PeerMgmtMessage::PeerFind(nid,None,qconf));
      },
      // particular case with synchronous blocking request : proxy query directly
      ProtoMessage::FIND_NODE(mut qconf@(QueryModeMsg::Proxy,_,_,_,_,_,_,_), nid) => {
        update_query_conf (&mut qconf ,&rc.queryrules);
        debug!("Proxying Find peer {:?}", nid);
        let qp = query::get_prio(&qconf);
        let nbquer = query::get_nbquer(&qconf);
        let lifetime = rc.queryrules.lifetime(qp); // TODO proxy no lifetime mgmt = null -> init fn without lifetime?
        // unmanaged query
        let query = query::init_query(nbquer.to_usize().unwrap(), 1, lifetime, & rp.queries, None, None, None);
        rp.peers.send(PeerMgmtMessage::PeerFind(nid,Some(query.clone()), qconf.clone()));
        // block until result (proxy mode)
        let result = query.wait_query_result();
        debug!("!! {:?} replying to find with {:?}",rc.me,result);
        let mess : ProtoMessage<RT::P,RT::V> = ProtoMessage::STORE_NODE(None, result.left().unwrap().map(|v|DistantEnc((*v).clone())));
        send_msg(&mess, None, s,&rc.msgenc);
      },
      // general case as asynch waiting for reply
      ProtoMessage::FIND_NODE(mut qconf@(_, _,_,_,_,_,_,_), nid) => {
        update_query_conf (&mut qconf ,&rc.queryrules);
        let nbquer = query::get_nbquer(&qconf);
        let qp = query::get_prio(&qconf);
        // TODO server query initialization is not really efficient it should be done after local
        debug!("Proxying Find peer {:?}", nid);
        let lifetime = rc.queryrules.lifetime(qp); // TODO special lifetime when hoping?
        let qid = rc.queryrules.newid();
        qconf.0 = new_query_mode (&qconf.0, &rc.me, qid.clone());
        // Warning here is old qmode stored in conf new mode is for proxied query
        let query : query::Query<RT::P,RT::V> = query::init_query(nbquer.to_usize().unwrap(), 1, lifetime, & rp.queries, Some(qconf.clone()), Some(qid.clone()), None);
        debug!("Asynch Find peer {:?}", nid);
        // warn here is new qmode
        rp.peers.send(PeerMgmtMessage::PeerFind(nid,Some(query.clone()),qconf));
      },
      // particular case for asynch where we skip node during reply process
      ProtoMessage::FIND_VALUE(mut qconf@(QueryModeMsg::Asynch(_,_), _,_,_,_,_,_,_), nid) => {
        update_query_conf (&mut qconf ,&rc.queryrules);
        debug!("Asynch Find val {:?}", nid);
        rp.store.send(KVStoreMgmtMessage::KVFind(nid,None,qconf));
      },
      // particular case with synchronous blocking request
      ProtoMessage::FIND_VALUE(mut queryconf@(QueryModeMsg::Proxy, _,_,_,_,_,_,_), nid) => {
        let oldhop = query::get_nbhop(&queryconf); // Warn no set of this value
        let oldqp = query::get_prio(&queryconf);
        update_query_conf (&mut queryconf ,&rc.queryrules);
        let nbquer = query::get_nbquer(&queryconf);
        let qp = query::get_prio(&queryconf);
        let sprio = query::get_sprio(&queryconf);
        let nb_req = query::get_req_nb_res(&queryconf);
        debug!("Proxying Find val {:?}", nid);
        let lifetime = rc.queryrules.lifetime(qp); // TODO proxy no lifetime mgmt = null -> init fn without lifetime?
        let esthop = (rc.queryrules.nbhop(oldqp) - oldhop).to_usize().unwrap();
        let store = rc.queryrules.do_store(false, qp, sprio, Some(esthop)); // first hop
        let query = query::init_query(nbquer.to_usize().unwrap(), nb_req, lifetime, & rp.queries, None, None, Some(store));
        rp.store.send(KVStoreMgmtMessage::KVFind(nid,Some(query.clone()), queryconf.clone()));
        // block until result (proxy mode)
        let result = query.wait_query_result();
        match (queryconf.1){
          QueryChunk::Attachment => {
            for val in result.right().unwrap().into_iter(){
              let att = val.as_ref().and_then(|kv|kv.get_attachment().map(|p|p.clone()));
              let mess : ProtoMessage<RT::P,RT::V> = ProtoMessage::STORE_VALUE(None, val.clone().map(|v|DistantEnc(v)));
              send_msg(&mess,att.as_ref(),s,&rc.msgenc);
            }
          },
          _ /* no attachment */  =>  {
            for val in result.right().unwrap().into_iter(){
              let mess : ProtoMessage<RT::P,RT::V> = ProtoMessage::STORE_VALUE_ATT(None, val.clone().map(|v|DistantEncAtt(v)));
              send_msg(&mess,None,s,&rc.msgenc);
            }
          },
        };
      },
      // general case as asynch waiting for reply
      ProtoMessage::FIND_VALUE(mut queryconf, nid) => {
        let oldhop = query::get_nbhop(&queryconf); // Warn no set of this value
        let oldqp = query::get_prio(&queryconf);
        update_query_conf (&mut queryconf ,&rc.queryrules);
        let nbquer = query::get_nbquer(&queryconf);
        let qp = query::get_prio(&queryconf);
        let sprio = query::get_sprio(&queryconf);
        let nb_req = query::get_req_nb_res(&queryconf);
        debug!("Proxying Find value {:?}", nid);
        let lifetime = rc.queryrules.lifetime(qp); // TODO special lifetime when hoping
        // TODO same thing for prio that is 
        let qid = rc.queryrules.newid();
        queryconf.0 = new_query_mode (&queryconf.0, &rc.me, qid.clone());
        let esthop = (rc.queryrules.nbhop(oldqp) - oldhop).to_usize().unwrap();
        let store = rc.queryrules.do_store(false, qp, sprio, Some(esthop)); // first hop

        let query : query::Query<RT::P,RT::V> = query::init_query(nbquer.to_usize().unwrap(), nb_req, lifetime, & rp.queries, Some(queryconf.clone()), Some(qid.clone()), Some(store));
 
        debug!("Asynch Find val {:?}", nid);
        rp.store.send(KVStoreMgmtMessage::KVFind(nid,Some(query.clone()),queryconf));

      },
      // store node receive by server is asynch reply
      ProtoMessage::STORE_NODE(oqid, sre) => match oqid {
        Some(qid) => {
          let res = sre.map(|r|Arc::new(r.0));
          debug!("node store {:?} for {:?}", res, qid);
          match res {
            None  => {
              // release on sem of query
              rp.queries.send(QueryMgmtMessage::NewReply(qid, (res,None)));
            },
            Some(node) => {
              let rpsp = rp.clone();
              // TODO switch thread spawn to continuation style(after ping we send on channel
              // only : a thread for that is to much (transform one result to allow
              // continuation style exec
              thread::spawn(move ||{
                let sync = Arc::new((Mutex::new(false),Condvar::new()));
                // spawn ping node first (= checking)
                debug!("start ping on store node reception");
                rpsp.peers.send(PeerMgmtMessage::PeerPing(node.clone(), Some(sync.clone())) ); // peer ping will run all needed control (accept, up, get prio) and update peer table
                let pingok =  match utils::clone_wait_one_result(sync){
                  None => {
                    error!("Condvar issue for ping of {:?} ", node); 
                    false
                  },// bad logic 
                  Some (r) => r,
                };
                if(pingok){
                  // then send reply
                  rpsp.queries.send(QueryMgmtMessage::NewReply(qid, (Some(node),None))); 
                } else {
                  // bad response consider not found -> this is discutable (bad node may kill
                  // query with this)
                  rpsp.queries.send(QueryMgmtMessage::NewReply(qid, (None,None))); 
                }
              });
            },
          };
        },
        None => {
          error!("receive store node for non stored query mode");
        },
      },
      ProtoMessage::STORE_VALUE_ATT(oqid, sre) => match oqid {
        Some(qid) => {
          let res = sre.map(|r|r.0);
          debug!("node store {:?} for {:?}", res, qid);
          match res {
            None  => (),
            Some(ref node) => {
              // Storage is done by query management
            },
          };
          rp.queries.send(QueryMgmtMessage::NewReply(qid, (None,res)));
        },
        None => {
          error!("receive store node for non stored query mode");
        },
      },
      ProtoMessage::STORE_VALUE(oqid, sre)=> match oqid {
        Some(qid) => {
          let res = match sre {
            Some(r) => {
              let mut res = r.0;
              // get attachment from transport response
              match oa {
                (None,&Some(ref at)) | (Some(ref at),&None) => {
                  if !res.set_attachment(at){
                    error!("error setting an attachment")
                  };
                },
                _ => {
                  error!("no attachment for store value att");
                },
              }
              Some(res)
            },
            None => {
              // TODO check unused attachment at least remove file
              None
            },
          };
          rp.queries.send(QueryMgmtMessage::NewReply(qid, (None,res)));
        },
        None => {
          error!("receive store node for non stored query mode");
        },
      },
      u => error!("Unmanaged query to serving process : {:?}", u),
    }
    if oneonly.is_some() {
      break;
    }
  }
}



