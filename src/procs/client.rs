//! Client process : used to send message to distant peers, either one time (non connected
//! transport) or multiple time (connected transport and listening on channel (stored in
//! peermanager) for instructions.



use rustc_serialize::json;
use procs::mesgs::{self,PeerMgmtMessage,ClientMessage,ClientMessageIx};
use msgenc::{ProtoMessage};
use std::fs::File;
use std::io::Result as IoResult;
use peer::{PeerMgmtMeths, PeerPriority,PeerStateChange,PeerState};
use std::str::from_utf8;
use procs::{peermanager,RunningContext,RunningProcesses,ArcRunningContext,RunningTypes};
use time::Duration;
use std::sync::{Arc,Semaphore,Condvar,Mutex};
use transport::TransportStream;
use transport::WriteTransportStream;
use transport::Transport;
use keyval::{Attachment,SettableAttachment};
use std::sync::mpsc::{Sender,Receiver};
use query::{self,QueryMode,QueryChunk,QueryModeMsg};
use peer::Peer;
use keyval::KeyVal;
use utils::{self,OneResult,receive_msg,send_msg,Either};
use std::net::{SocketAddr};
use msgenc::{MsgEnc,DistantEncAtt,DistantEnc};
use procs::server;
use std::thread::Thread;

/// TODO replace client channel by that
/// Reference used to send Client Message.
/// This could be used by any process to keep trace of a client process : 
/// for instance from server client direct client query (without peermgmt)
pub enum ClientHandle<P : Peer, V : KeyVal> {

  /// Uninitialised client or no handle from peermgmt
  /// First message is querying a handle from peermgmt (through OneResult), when in server.
  /// When in query no new handle are needed but next query created from server will certainly use
  /// a faster handle.
  /// - optional WriteStream, this is only in server context when transport has instantiate write
  /// stream on connect
  /// Result in communication with PeerMgmt Process
  NoHandle,

  /// no new thread, the function is called, for DHT where there is not much client treatment
  /// (from peermanager thread). TODO plus all fields
  /// Result in communication with peermgmtprocess
  Local,


  /// thread do multiplexe some client : usize is client ix
  //ThreadedMult(Sender<ClientMessageIx<P,V>>, usize),

  /// WriteTransportStream is runing (loop) in its own thread
  Threaded(Sender<ClientMessageIx<P,V>>),
}

impl<P : Peer, V : KeyVal> ClientHandle<P,V> {
  
  /// either 
  pub fn send_msg(&self) {}

  pub fn send_pong(&self,repsig : String) {
    // TODO !!
  }


  /// for mode threaded, will spawn the client thread
  pub fn start() {}
  //fn start() -> ClientHandle<RT> {}
}

// ping a peer creating a thread with client when com ok, if thread exists (querying peermanager)
// use it directly to receive pong
// return nothing, send message to ppermgmt instead
// Method must run in its own thread (blocks on tcp exchange).
/// Start a client thread, either for one message (with or without ping) or for a connection loop
/// (connected transport). Always begin with an accept call (complex accept should only run in
/// connected mode).
/// TODO add param saying if we start receive trait!!!!
/// TODO add current prio to avoid needless call to accept!!!!
pub fn start <RT : RunningTypes>
 (p : Arc<RT::P>, 
  tcl : Option<Sender<ClientMessage<RT::P,RT::V>>>, // TODO remove it
  orcl : Option<Receiver<ClientMessageIx<RT::P,RT::V>>>, 
  rc : ArcRunningContext<RT>, 
  rp : RunningProcesses<RT>,
  withPing: bool, 
  withPingReply : Option<OneResult<bool>>,
  omessage : Option<ClientMessage<RT::P,RT::V>>,
  connected : bool,
  ) {
  let mut ok = false;
  match rc.peerrules.accept(&(*p),&rp,&rc) {
    None => {
      warn!("refused node {:?}",p);
      rp.peers.send(PeerMgmtMessage::PeerUpdatePrio(p.clone(), PeerState::Refused));
    },
    Some(pri)=> {
      // connect
      let mut sc = rc.transport.connectwith(&p.to_address(), Duration::seconds(5));
      // TODO terrible imbricated matching see if some flatmap or helper function
      match sc {
        Err(e) => {
          info!("Cannot connect");
          rp.peers.send(PeerMgmtMessage::PeerUpdatePrio(p.clone(), PeerState::Offline(pri)));
        },
        Ok((mut s1,_)) => {
          // TODO if needed start receive and send handle to peermgmt!!!
          // closure to send message with one reconnect try on error
          let r = if withPing {
            if !connected {
              error!("Starting ping unconnected {:?}, {:?}", p.get_key(), pri);
              // TODO duration same as client timeout
              let chal = rc.peerrules.challenge(&(*rc.me));
              let sign = rc.peerrules.signmsg(&(*rc.me), &chal);
              // TODO store challenge in a global map (a query in querymanager) : currently not done so no check of challenge when
              // running unconnected transport + store with ping result if present
              let mess : ProtoMessage<RT::P,RT::V>  = ProtoMessage::PING((*rc.me).clone(), chal.clone(), sign);
              send_msg(&mess,None,&mut s1,&rc.msgenc);
              // we do not wait for a result
              true
            } else {
              debug!("pinging in start client process, {:?}, {:?}", p.get_key(), pri);
              let r =  ping::<RT>(&(*p), rc.clone(), &mut s1);
              withPingReply.map(|ares|{
                utils::ret_one_result(&ares, r)
              });
              if r {
                rp.peers.send(PeerMgmtMessage::PeerUpdatePrio(p.clone(), PeerState::Online(pri.clone())));
                true
              } else {
                error!("Started unpingable client process");
                rp.peers.send(PeerMgmtMessage::PeerUpdatePrio(p.clone(), PeerState::Offline(pri.clone())));
                false
              }
            }
          } else {
            if connected {
              rp.peers.send(PeerMgmtMessage::PeerUpdatePrio(p.clone(), PeerState::Online(pri.clone())));
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
                recv_match(&p,&rc,&rp,m,false, &mut s1,&pri);
              },
            },
            Some(ref r) => {
              loop{
                match r.recv() {
                  Err(_) => {
                    error!("Client channel issue");
                    break;
                  },
                  Ok((m,ix)) => {
                    let (okrecv, s) = recv_match(&p,&rc,&rp,m,true,&mut s1,&pri); 
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
    // set to offline
    rp.peers.send(PeerMgmtMessage::PeerRemFromClient(p.clone(), PeerStateChange::Offline));
//    tcl.unwrap().drop();
  } 
  if !ok {
    match orcl {
      None => (),
      Some(ref rcl) => {
        // empty channel
        loop {
          match rcl.try_recv() {
            Ok((ClientMessage::PeerFind(_,Some(query), _),ix)) | Ok((ClientMessage::KVFind(_,Some(query), _),ix)) => {
              query.lessen_query(1, &rp.peers);
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
pub fn recv_match <RT : RunningTypes>
 (p : &Arc<RT::P>, 
  rc : &ArcRunningContext<RT>, 
  rp : &RunningProcesses<RT>,
  m : ClientMessage<RT::P,RT::V>,
  connected : bool,
  s : &mut <RT::T as Transport>::WriteStream,
  pri : &PeerPriority,
  ) -> (bool, Option<<RT::T as Transport>::WriteStream>) {
  let mut ok = true;
  let mut newcon = None;
  macro_rules! sendorconnect(($mess:expr,$oa:expr) => (
  {
    ok = send_msg($mess,$oa,s,&rc.msgenc);
    if !ok {
      debug!("trying reconnection");
      rc.transport.connectwith(&p.to_address(), Duration::seconds(2)).map(|(mut n,_)|{
        debug!("reconnection in client process ok");
        // TODO send to peermgmet optional writeTransportStream
        ok = send_msg($mess, $oa,&mut n,&rc.msgenc);
        newcon = Some(n);
      });
    };
    if !ok {
      rp.peers.send(PeerMgmtMessage::PeerChangeState(p.clone(), PeerStateChange::Offline));
    };
  }
  ));

  match m {
    ClientMessage::PeerPong(_) => {
      // TODO !!!!
    },
    ClientMessage::ShutDown => {
      warn!("shuting client");
      ok = false;
    },
    ClientMessage::PeerPing(p) => {
      if !connected {
        debug!("Starting ping unconnected {:?}, {:?}", p.get_key(), pri);
        // TODO duration same as client timeout
        let chal = rc.peerrules.challenge(&(*rc.me));
        let sign = rc.peerrules.signmsg(&(*rc.me), &chal);
        // TODO store challenge in a global map : currently not done so no check of challenge when
        // running unconnected transport
        // we do not wait for a result
        let mess : ProtoMessage<RT::P,RT::V>  = ProtoMessage::PING((*rc.me).clone(), chal.clone(), sign);
        sendorconnect!(&mess,None);
        // TODO asynch wait for reply as new type of query
      } else {
        debug!("Pinging peer (tcp) : {:?}", p);
        // No reconnect here but may not be an issue
        if ping::<RT>(&(*p), rc.clone(), s) {
          // Add peer as lower priority and pending message and add its socket!!!
           debug!("Pong reply ok, adding or updating peer {:?}, {:?}", p.get_key(), pri);
           // add node as ok (with previous priority)
           rp.peers.send(PeerMgmtMessage::PeerChangeState(p, PeerStateChange::Online));
        } else {
          debug!("User seem offline");
          rp.peers.send(PeerMgmtMessage::PeerChangeState(p, PeerStateChange::Offline));
          ok = false;
        }
      };
    },
    ClientMessage::PeerFind(nid,oquery, mut queryconf) =>  {
      // note that new queryid and recnode has been set in server
      // and query count in server -> amix and aproxy became as simple send
      // send query with hop + 1
      // local queryid set in server here update dest
      queryconf.dec_nbhop(&rc.rules);
      let mess  : ProtoMessage<RT::P,RT::V> = ProtoMessage::FIND_NODE(queryconf, nid);
      sendorconnect!(&mess,None);
      if !ok {
        // lessen TODO asynch is pow...
        oquery.map(|query|query.lessen_query(1, &rp.peers));
      }
    },
    ClientMessage::KVFind(nid, oquery, mut queryconf) => {
      // no adding query counter for peer
      // send query with hop + 1
      queryconf.dec_nbhop(&rc.rules);
      let mess  : ProtoMessage<RT::P,RT::V> = ProtoMessage::FIND_VALUE(queryconf, nid);
      sendorconnect!(&mess,None);
      if !ok {
        // lessen TODO asynch is pow...
        oquery.map(|query|query.lessen_query(1, &rp.peers));
      }
    },
    ClientMessage::StoreNode(oqid, r) => {
      let mess  : ProtoMessage<RT::P,RT::V> = ProtoMessage::STORE_NODE(oqid, r.map(|v|DistantEnc((*v).clone())));
      sendorconnect!(&mess,None);
    },
    ClientMessage::StoreKV(oqid, chunk, r) => {
      match chunk {
        QueryChunk::Attachment => {
          let att = r.as_ref().and_then(|kv|kv.get_attachment().map(|p|p.clone()));
          let mess  : ProtoMessage<RT::P,RT::V> = ProtoMessage::STORE_VALUE(oqid, r.clone().map(|v|DistantEnc(v)));
          sendorconnect!(&mess,att.as_ref());
        },
        _                     =>  {
          let mess  : ProtoMessage<RT::P,RT::V> = ProtoMessage::STORE_VALUE_ATT(oqid, r.clone().map(|v|DistantEncAtt(v)));
          sendorconnect!(&mess,None);
        },
      };
    },
  };
  (ok, newcon)
}

// TODO TODO TODO TODO remove : ping is an asynch action and not an addable action
pub fn ping <RT : RunningTypes>
 (p : &RT::P, 
  rc : ArcRunningContext<RT>, 
  s : &mut <RT::T as Transport>::WriteStream) 
 -> bool {
  debug!("ping fn from {:?} to {:?}", rc.me.get_key(), p.get_key());
  let chal = rc.peerrules.challenge(&(*rc.me));
  let sign = rc.peerrules.signmsg(&(*rc.me), &chal);
  // TODO for unconnected protomessage ping is a findnode with unmanaged query (QReply none) over the node itself and
  // we do not wait for a result (the idea is getting a reply to put our peer in online state
  let mess : ProtoMessage<RT::P,RT::V>  = ProtoMessage::PING((*rc.me).clone(), chal.clone(), sign);
  send_msg(&mess,None,s,&rc.msgenc);
  true
  // wait for a reply message (useless currently as socket open but more info can be send
  // (Node private key))
  /* TODO here synch wait for reply : remove all ping wait no more synch ping
  let r1 : Option<(ProtoMessage<RT::P,RT::V>,Option<Attachment>)> = receive_msg (s, &rc.msgenc); 
  match r1 {
    None => {
      warn!("Waiting pong timeout or invalid pong");
      false
    }, // TODO remove socket from peers??
    Some((r,_))  => {
      match r {
        ProtoMessage::PONG(ps) => {
          if rc.peerrules.checkmsg(&(*p),&chal,&ps) {
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
  */
}

