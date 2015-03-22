//! Client process : used to send message to distant peers, either one time (non connected
//! transport) or multiple time (connected transport and listening on channel (stored in
//! peermanager) for instructions.



use rustc_serialize::json;
use procs::mesgs::{self,PeerMgmtMessage,ClientMessage};
use msgenc::{ProtoMessage};
use std::fs::File;
use std::io::Result as IoResult;
use peer::{PeerMgmtRules, PeerPriority};
use std::str::from_utf8;
use procs::{peermanager,RunningContext,RunningProcesses};
use std::time::Duration;
use std::sync::{Arc,Semaphore,Condvar,Mutex};
use transport::TransportStream;
use transport::Transport;
use kvstore::Attachment;
use std::sync::mpsc::{Sender,Receiver};
use query::{self,QueryRules,QueryMode,QueryChunk,QueryConfMsg,QueryModeMsg};
use peer::Peer;
use kvstore::KeyVal;
use utils::{self,OneResult,receive_msg,send_msg,Either};
use std::net::{SocketAddr};
use msgenc::{MsgEnc,DistantEncAtt,DistantEnc};
use procs::server;

// ping a peer creating a thread with client when com ok, if thread exists (querying peermanager)
// use it directly to receive pong
// return nothing, send message to ppermgmt instead
// Method must run in its own thread (blocks on tcp exchange).
/// Start a client thread, either for one message (with or without ping) or for a connection loop
/// (connected transport). Always begin with an accept call (complex accept should only run in
/// connected mode).
pub fn start
 <P : Peer,
  V : KeyVal,
  R : PeerMgmtRules<P,V>, 
  Q : QueryRules, 
  E : MsgEnc,
  T : Transport,
 > 
 (p : Arc<P>, 
  tcl : Option<Sender<ClientMessage<P,V>>>, 
  orcl : Option<Receiver<ClientMessage<P,V>>>, 
  rc : RunningContext<P,V,R,Q,E,T>, 
  rp : RunningProcesses<P,V>, 
  withPing: bool, 
  withPingReply : Option<OneResult<bool>>,
  omessage : Option<ClientMessage<P,V>>,
  connected : bool,
  ) {
  let mut ok = false;
  match rc.1.accept(&(p),&rp,&rc) {
    None => {
      warn!("refused node {:?}",p);
      rp.0.send(PeerMgmtMessage::PeerUpdatePrio(p.clone(), PeerPriority::Blocked));
    },
    Some(pri)=> {
      // connect
      let mut sc : IoResult<T::Stream> = rc.4.connectwith(&p.to_address(), Duration::seconds(5));
      // TODO terrible imbricated matching see if some flatmap or helper function
      match sc {
        Err(e) => {
          info!("Cannot connect");
          rp.0.send(PeerMgmtMessage::PeerUpdatePrio(p.clone(), PeerPriority::Offline));
        },
        Ok(mut s1) => {
          // closure to send message with one reconnect try on error
          let r = if withPing {
            if !connected {
              error!("Starting ping unconnected {:?}, {:?}", p.get_key(), pri);
              // TODO duration same as client timeout
              let chal = rc.1.challenge(&(*rc.0));
              let sign = rc.1.signmsg(&(*rc.0), &chal);
              // TODO store challenge in a global map (a query in querymanager) : currently not done so no check of challenge when
              // running unconnected transport + store with ping result if present
              let mess : ProtoMessage<P,V>  = ProtoMessage::PING((*rc.0).clone(), chal.clone(), sign);
              send_msg(&mess,None,&mut s1,&rc.3);
              // we do not wait for a result
              true
            } else {
              debug!("pinging in start client process, {:?}, {:?}", p.get_key(), pri);
              let r =  ping::<_,V,_,_,_,_>(&(*p), rc.clone(), &mut s1);
              withPingReply.map(|ares|{
                utils::ret_one_result(ares, r)
              });
              if r {
                rp.0.send(PeerMgmtMessage::PeerUpdatePrio(p.clone(), pri));
                true
              } else {
                error!("Started unpingable client process");
                rp.0.send(PeerMgmtMessage::PeerUpdatePrio(p.clone(), PeerPriority::Offline));
                false
              }
            }
          } else {
            if connected {
              rp.0.send(PeerMgmtMessage::PeerUpdatePrio(p.clone(), pri));
            };
            true
          };
          if r {
          // loop on rcl
          match orcl {
            None => match omessage {
              None => if !withPing {
                error!("Unexpected client process without channel or message")
              },
              Some(m) => {
                recv_match(&p,&rc,&rp,m,false, &mut s1,pri);
              },
            },
            Some(ref r) => {
              loop{
                match r.recv() {
                  Err(_) => {
                    error!("Client channel issue");
                    break;
                  },
                  Ok(m) => {
                    let (okrecv, s) = recv_match(&p,&rc,&rp,m,true,&mut s1,pri); 
                     match s {
                       // update of stream is for possible reconnect
                       Some(s) => s1 = s,
                       None => (),
                    };
                    if !okrecv {
                      ok = false;
                      break;
                    };
                  },
                };
              };
            },
          };
          };
        },
      };
    },
  };
  if tcl.is_some() {
    // remove channel
    // TODO unsafe race condition here (a new one could have been open in between)
    debug!("Query for remove channnel");
    rp.0.send(PeerMgmtMessage::PeerRemChannel(p.clone()));
//    tcl.unwrap().drop();
  } 
  if !ok {
    match orcl {
      None => (),
      Some(ref rcl) => {
        // empty channel
        loop {
          match rcl.try_recv() {
            Ok(ClientMessage::PeerFind(_,Some(query), _)) | Ok(ClientMessage::KVFind(_,Some(query), _)) => {
              query.lessen_query(1, &rp.0);
            },
            Err(e) => {
              debug!("end of emptying channel : {:?}",e);
              break;
            },
            _ => (),
          };
        };
      },
    };
  };
  debug!("End cli process");
}



#[inline]
/// manage new client message
/// return a new transport if reconnected and a boolean if no more connectivity
pub fn recv_match
 <P : Peer,
  V : KeyVal,
  R : PeerMgmtRules<P,V>, 
  Q : QueryRules, 
  E : MsgEnc,
  T : Transport,
 > 
 (p : &Arc<P>, 
  rc : &RunningContext<P,V,R,Q,E,T>, 
  rp : &RunningProcesses<P,V>, 
  m : ClientMessage<P,V>,
  connected : bool,
  s : &mut T::Stream,
  pri : PeerPriority,
  ) -> (bool, Option<T::Stream>) {
  let mut ok = true;
  let mut newcon = None;
  macro_rules! sendorconnect(($mess:expr,$oa:expr) => (
  {
    ok = send_msg($mess,$oa,s,&rc.3);
    if !ok {
      debug!("trying reconnection");
      rc.4.connectwith(&p.to_address(), Duration::seconds(2)).map(|mut n|{
        debug!("reconnection in client process ok");
        ok = send_msg($mess, $oa,&mut n,&rc.3);
        newcon = Some(n);
      });
    };
    if !ok {
      rp.0.send(PeerMgmtMessage::PeerUpdatePrio(p.clone(), PeerPriority::Offline));
    };
  }
  ));

  match m {
    ClientMessage::ShutDown => {
      warn!("shuting client");
      ok = false;
    },
    ClientMessage::PeerPing(p) => {
      if !connected {
        debug!("Starting ping unconnected {:?}, {:?}", p.get_key(), pri);
        // TODO duration same as client timeout
        let chal = rc.1.challenge(&(*rc.0));
        let sign = rc.1.signmsg(&(*rc.0), &chal);
        // TODO store challenge in a global map : currently not done so no check of challenge when
        // running unconnected transport
        // we do not wait for a result
        let mess : ProtoMessage<P,V>  = ProtoMessage::PING((*rc.0).clone(), chal.clone(), sign);
        sendorconnect!(&mess,None);
        // TODO asynch wait for reply as new type of query
      } else {
        debug!("Pinging peer (tcp) : {:?}", p);
        // No reconnect here but may not be an issue
        if ping::<_,V,_,_,_,_>(&(*p), rc.clone(), s) {
          // Add peer as lower priority and pending message and add its socket!!!
           debug!("Pong reply ok, adding or updating peer {:?}, {:?}", p.get_key(), pri);
           // add node as ok (with previous priority)
           rp.0.send(PeerMgmtMessage::PeerUpdatePrio(p, pri));
        } else {
          debug!("User seem offline");
          rp.0.send(PeerMgmtMessage::PeerUpdatePrio(p, PeerPriority::Offline));
          ok = false;
        }
      };
    },
    ClientMessage::PeerFind(nid,oquery, mut queryconf) => match queryconf.0 {
      QueryModeMsg::Proxy => {
        let query = oquery.unwrap(); // unsafe code : may panic but it is a design problem : a proxy mode query should allways have a query, otherwhise it is a bug
        // send query with hop + 1
        server::update_query_conf(&mut queryconf, &rc.2);
        query::dec_nbhop(&mut queryconf, &rc.2);
        let mess : ProtoMessage<P,V> = ProtoMessage::FIND_NODE(queryconf, nid.clone());
        sendorconnect!(&mess,None);
        // add a query to peer
        rp.0.send(PeerMgmtMessage::PeerQueryPlus(p.clone()));
        // receive (first something or last nothing reply in repc plus rules to
        let r : Option<(ProtoMessage<P,V>, Option<Attachment>)> = receive_msg(s, &rc.3); 
        // store if new (with checking before (ping)))
        let success = match r {
          None => {
            warn!("Waiting peer query timeout or Invalid find peer reply");
            false
          },
          Some((dmess,oa))  => {
            match dmess {
              ProtoMessage::STORE_NODE(rconf, None) => {
                false // it is a success (there is a reply) but we still need to wait for other query so its consider a failure
              },
              ProtoMessage::STORE_NODE(rconf, Some(DistantEnc(snod))) => {
                let node = Arc::new(snod);
                // check the returned node is not the connected node (should
                // not happen but if it happen we will deadlock this thread
                if node.get_key() != p.get_key() {
                  // Adding is done by simple PeerPing
                  let sync = Arc::new((Mutex::new(false),Condvar::new()));
                  rp.0.send(PeerMgmtMessage::PeerPing(node.clone(), Some(sync.clone())));
                  let pingok = match utils::clone_wait_one_result(sync){
                    None => {
                      error!("Condvar issue for ping of {:?} ", node); 
                      false // bad logic 
                    },
                    Some (r) => r,
                  };
                  if pingok{
                    query.set_query_result(Either::Left(Some(node)),&rp.2)
                  } else {
                    false
                  }
                } else {
                  error!("Proxy store node is the first hop which we already know");
                  false
                }
              },
              _ => {
                error!("Invalid message waiting for store_node");
                false
              },
            }
          },
        };

        let squ : Sender<PeerMgmtMessage<P,V>>  = rp.0.clone();
        let typedquery : & query::Query<P,V> = & query;
        if success {
          typedquery.release_query(&squ);
        } else {
          // release on semaphore
          typedquery.lessen_query(1,&squ);
        }
        // remove a proxyied query from peer
        rp.0.send(PeerMgmtMessage::PeerQueryMinus(p.clone()));
        if !ok {
          // lessen TODO asynch is pow...
          query.lessen_query(1, &rp.0);
        }

      },
      _  => {
        // note that new queryid and recnode has been set in server
        // and query count in server -> amix and aproxy became as simple send
        // send query with hop + 1
        // local queryid set in server here update dest
        query::dec_nbhop(&mut queryconf, &rc.2);
        let mess  : ProtoMessage<P,V> = ProtoMessage::FIND_NODE(queryconf, nid);
        sendorconnect!(&mess,None);
        if !ok {
          // lessen TODO asynch is pow...
          oquery.map(|query|query.lessen_query(1, &rp.0));
        }
      },
    },
    ClientMessage::KVFind(nid, oquery, mut queryconf) => match queryconf.0 {
      QueryModeMsg::Proxy => {
        let query = oquery.unwrap(); // unsafe code : may panic but it is a design problem : a proxy mode query should allways have a query, otherwhise it is a bug
        // send query with hop + 1
        query::dec_nbhop(&mut queryconf, &rc.2);
        let mess : ProtoMessage<P,V> = ProtoMessage::FIND_VALUE(queryconf, nid.clone());
        sendorconnect!(&mess,None);
        // add a query to peer
        rp.0.send(PeerMgmtMessage::PeerQueryPlus(p.clone()));
        // receive (first something or last nothing reply in repc plus rules to
        let r  : Option<(ProtoMessage<P,V>,Option<Attachment>)> = receive_msg (s, &rc.3); 
        // store if new (with checking before (ping)))
        let success = match r {
          None => {
            warn!("Waiting peer query timeout or invalid find peer reply");
            false
          },
          Some((dmess,oa))  => {
            match dmess {
              ProtoMessage::STORE_VALUE_ATT(rconf, None) | ProtoMessage::STORE_VALUE(rconf, None) => {
                false // it is a success (there is a reply) but we still need to wait for other query so its consider a failure
              },
              ProtoMessage::STORE_VALUE_ATT(rconf, Some(DistantEncAtt(node))) => {
                query.set_query_result(Either::Right(Some(node)),&rp.2)
              },
              ProtoMessage::STORE_VALUE(rconf, Some(DistantEnc(mut node))) => {
                match oa {
                  Some(ref at) => {
                    if !node.set_attachment(at) {
                      error!("error setting an attachment")
                      };
                  },
                  _ => {
                    error!("no attachment for store value att");
                  },
                }
                query.set_query_result(Either::Right(Some(node)),&rp.2)
              },
              _ => {
                error!("wrong message waiting for store_value");
                false
              },
            }
          },
        };
        let typedquery : & query::Query<P,V> = & query;
        if success {
          typedquery.release_query(&rp.0);
        } else  {
          // release on semaphore
          typedquery.lessen_query(1,&rp.0);
        };
        // remove a proxyied query from peer
        rp.0.send(PeerMgmtMessage::PeerQueryMinus(p.clone()));
        if !ok {
          // lessen TODO asynch is pow...
          query.lessen_query(1, &rp.0);
        }
      },
      _ => {
        // no adding query counter for peer
        // send query with hop + 1
        query::dec_nbhop(&mut queryconf, &rc.2);
        let mess  : ProtoMessage<P,V> = ProtoMessage::FIND_VALUE(queryconf, nid);
        sendorconnect!(&mess,None);
        if !ok {
          // lessen TODO asynch is pow...
          oquery.map(|query|query.lessen_query(1, &rp.0));
        }
      },
    },
    ClientMessage::StoreNode(queryconf, r) => {
      let mess  : ProtoMessage<P,V> = ProtoMessage::STORE_NODE(queryconf.0.get_qid(), r.cloned().map(|v|DistantEnc(v)));
      sendorconnect!(&mess,None);
    },
    ClientMessage::StoreKV(queryconf, r) => {
      match (queryconf.1){
        QueryChunk::Attachment => {
          let att = r.as_ref().and_then(|kv|kv.get_attachment().map(|p|p.clone()));
          let mess  : ProtoMessage<P,V> = ProtoMessage::STORE_VALUE(queryconf.0.get_qid(), r.clone().map(|v|DistantEnc(v)));
          sendorconnect!(&mess,att.as_ref());
        },
        _                     =>  {
          let mess  : ProtoMessage<P,V> = ProtoMessage::STORE_VALUE_ATT(queryconf.0.get_qid(), r.clone().map(|v|DistantEncAtt(v)));
          sendorconnect!(&mess,None);
        },
      };
    },
  };
  (ok, newcon)
}


pub fn ping
 <P : Peer,
  V : KeyVal,
  R : PeerMgmtRules<P,V>, 
  Q : QueryRules, 
  E : MsgEnc,
  T : Transport> 
 (p : &P, 
  rc : RunningContext<P,V,R,Q,E,T>, 
  s : &mut T::Stream) 
 -> bool {
  debug!("ping fn from {:?} to {:?}", rc.0.get_key(), p.get_key());
  let chal = rc.1.challenge(&(*rc.0));
  let sign = rc.1.signmsg(&(*rc.0), &chal);
  // TODO for unconnected protomessage ping is a findnode with unmanaged query (QReply none) over the node itself and
  // we do not wait for a result (the idea is getting a reply to put our peer in online state
  let mess : ProtoMessage<P,V>  = ProtoMessage::PING((*rc.0).clone(), chal.clone(), sign);
  send_msg(&mess,None,s,&rc.3);
  // wait for a reply message (useless currently as socket open but more info can be send
  // (Node private key))
  let r1 : Option<(ProtoMessage<P,V>,Option<Attachment>)> = receive_msg (s, &rc.3); 
  match r1 {
    None => {
      warn!("Waiting pong timeout or invalid pong");
      false
    }, // TODO remove socket from peers??
    Some((r,_))  => {
      match r {
        ProtoMessage::PONG(ps) => {
          if rc.1.checkmsg(&(*p),&chal,&ps) {
            debug!("received valid Pong with signature {:?} ", ps);
            // add user
            true
          }else{
            warn!("Forged pong reply received");
            false
          }
        },
        _ => {
          warn!("wrong message waiting for ping");
          false
        },
      }
    },
  }
}

