//! Client process : used to send message to distant peers, either one time (non connected
//! transport) or multiple time (connected transport and listening on channel (stored in
//! peermanager) for instructions.



use procs::mesgs::{self,PeerMgmtMessage,ClientMessage,ClientMessageIx};
use mydhtresult::Result as MDHTResult;
use peer::{PeerMgmtMeths,PeerStateChange};
use procs::{RunningProcesses,ArcRunningContext,RunningTypes};
use time::Duration;
use std::sync::{Arc};
use transport::Transport;
//use keyval::{Attachment,SettableAttachment};
use std::sync::mpsc::{Sender,Receiver};
//use query::{self,QueryMode,QueryChunk,QueryModeMsg};
use query::QueryChunk;
use peer::Peer;
use keyval::KeyVal;
use msgenc::{DistantEncAtt,DistantEnc};
use procs::server::start_listener;
use procs::server::{serverinfo_from_handle};
use route::{ClientSender};
use route::send_msg;
use msgenc::send_variant::ProtoMessage;
use std::borrow::Borrow;
use utils::Either;

pub fn start<RT : RunningTypes>
 (p : &Arc<RT::P>,
  rcl : Receiver<ClientMessageIx<RT::P,RT::V,<RT::T as Transport>::WriteStream>>,
  rc : ArcRunningContext<RT>,
  rp : RunningProcesses<RT>,
  mut osend : Option<ClientSender<<RT::T as Transport>::WriteStream,<RT::P as Peer>::Shadow>>,
  mult : bool,
  ) -> bool {
  match start_local(p,&rc,&rp,Either::Left((osend,rcl,mult))) {
    Ok(r) => r,
    Err(e) => {
      // peerremfromclient send from client_match directly (no need to try reconnect
      error!("client error : {}",e);
      false
    },
  }
}

pub struct CleanableReceiver<'a, RT : RunningTypes> (
  Receiver<ClientMessageIx<RT::P,RT::V,<RT::T as Transport>::WriteStream>>,
  &'a Sender<mesgs::PeerMgmtMessage<RT::P,RT::V,<RT::T as Transport>::ReadStream,<RT::T as Transport>::WriteStream>>,
  &'a Sender<mesgs::QueryMgmtMessage<RT::P,RT::V>>)
where RT::A : 'a, 
      RT::P : 'a,
      <RT::P as KeyVal>::Key : 'a,
      RT::V : 'a,
      <RT::V as KeyVal>::Key : 'a,
      <RT::T as Transport>::ReadStream : 'a,
      <RT::T as Transport>::WriteStream : 'a
;

impl<'a,RT : RunningTypes> Drop for CleanableReceiver<'a,RT> 
where RT::A : 'a, 
      RT::P : 'a,
      <RT::P as KeyVal>::Key : 'a,
      RT::V : 'a,
      <RT::V as KeyVal>::Key : 'a,
      <RT::T as Transport>::ReadStream : 'a,
      <RT::T as Transport>::WriteStream : 'a
{
  fn drop(&mut self) {
    debug!("Drop of client receiver, emptying channel");
    // empty channel
    loop {
      match self.0.try_recv() {
        Ok((ClientMessage::PeerFind(_,query_handle, _),_)) | Ok((ClientMessage::KVFind(_,query_handle, _),_)) => {
          debug!("- emptying channel found query");
          if query_handle.lessen_query(1, self.1, self.2) {
            query_handle.release_query(self.1, self.2)
          }
        },
        Err(e) => {
          debug!("end of emptying channel : {:?}",e);
          break;
        },
        _ => (),
      };
    };
  }
}

// ping a peer creating a thread with client when com ok, if thread exists (querying peermanager)
// use it directly to receive pong
// return nothing, send message to ppermgmt instead
// Method must run in its own thread (blocks on tcp exchange).
/// Start a client thread, either for one message (with or without ping) or for a connection loop
/// (managed Client thread).
pub fn start_local <RT : RunningTypes>
 (p : &Arc<RT::P>,
  rc : &ArcRunningContext<RT>,
  rp : &RunningProcesses<RT>,
  params : Either<
    (Option<ClientSender<<RT::T as Transport>::WriteStream,<RT::P as Peer>::Shadow>>,
     Receiver<ClientMessageIx<RT::P,RT::V,<RT::T as Transport>::WriteStream>>,
     bool),
    (Option<&mut ClientSender<<RT::T as Transport>::WriteStream,<RT::P as Peer>::Shadow>>,
     ClientMessage<RT::P,RT::V,<RT::T as Transport>::WriteStream>)
  >,
  ) -> MDHTResult<bool> {
  let mut ok = true;
  // connect
  let mut onewsend : Option<ClientSender<<RT::T as Transport>::WriteStream,<RT::P as Peer>::Shadow>> = 
    if params.left_ref().map_or(false, |l|l.0.is_none()) 
    || params.right_ref().map_or(false,|r|r.0.is_none()) {
    // TODO duration from rules!!!
    mult_client_connect(p, rc, rp, None).ok()
  } else {
    None
  };
  // TODO if needed start receive and send handle to peermgmt!!!
  // closure to send message with one reconnect try on error
  // loop on rcl
  match params { // TODO orcl depend on presence of message (plus either of sender : do Either<(rcl, sender),(&mut sender, msg)> for message
      Either::Right((osend, m)) => {
          let sc0 : &mut ClientSender<<RT::T as Transport>::WriteStream,<RT::P as Peer>::Shadow> = match osend {
            None => match &mut onewsend {
              &mut None => {
                debug!("cannot connect with peer returning false");
                info!("Cannot connect");
                // send no connection possible : 
                return Ok(false);
              },
              &mut Some(ref mut ns) => ns,
            },
            Some(mut cs) => cs,
          };
          let (okrecv, _) = try!(client_match(p,&rc,&rp,m,false,sc0));
          ok = okrecv;
      },
      Either::Left((osend, r, mult)) => {
        let mut sc = Vec::new();
        let sc0 : ClientSender<<RT::T as Transport>::WriteStream,<RT::P as Peer>::Shadow> = match osend {
          None => match onewsend {
            None => {
              debug!("cannot connect with peer returning false");
              info!("Cannot connect");
              // send no connection possible : 
              return Ok(false);
            },
            Some(ns) => ns,
          },
          Some(cs) => cs,
        };
        sc.push(Some(sc0));

        let cr : CleanableReceiver<RT> = CleanableReceiver(r,&rp.peers,&rp.queries);
        loop {
          match cr.0.recv() {
            Err(_) => {
              error!("Client channel issue");
              break;
            },
            Ok((m,ix)) => {
              if ix > sc.len() {
                if mult {
                  if let ClientMessage::EndMult = m {
                    ok = false;
                    break;
                  };
                  panic!("BUG receive message for wrongly indexed client sender");
                } else {
                  panic!("BUG receive message for wrongly indexed client sender");
                };
              } else {
                let (okrecv, s) = {
                  match sc.get_mut(ix) {
                    Some(&mut Some(ref mut six)) => {
                      try!(client_match(p,rc,rp,m,true,six))
                    },
                    Some(&mut None) | None => {
                      if let ClientMessage::PeerAdd(p,ows) = m {
                        let newcon = mult_client_connect(&p,rc,rp,ows).map_err(|e|warn!("peer add failure : {:?}",e));
                        (newcon.is_ok(), newcon.ok())
                      } else {
                        (false, None)
                      }
                    },
                  }
                };
                match s {
                  // update of stream is for possible reconnect
                  // return &'a mut on reconnect
                  Some(s) => {
                    if ix == sc.len() {
                      sc.push(Some(s));
                    } else if ix < sc.len() {
                      sc.get_mut(ix).map(|mut sc|{
                        if sc.is_none() {
                        };
                        *sc = Some(s)
                      });
                    } else {
                      panic!("inconsistent client connection address, this is a bug");
                    }
                  },
                  None => (),
                };
                if !okrecv {
                  sc.get_mut(ix).map(|mut sc|{
                    if sc.is_some() {
                    };
                    *sc = None
                  });
                  if !mult {
                    ok = false;
                    break
                  };
                };
              };
            },
          };
        };
      },
  };
  Ok(ok)
}

const CONNECT_DURATION : i64 = 4; // TODO in params

#[inline]
/// manage new client message
/// return a new transport if reconnected and a boolean if no more connectivity : 
/// all other issue than offline are managed by returning errors
pub fn client_match <RT : RunningTypes>
 (p : &Arc<RT::P>,
  rc : &ArcRunningContext<RT>,
  rp : &RunningProcesses<RT>,
  m : ClientMessage<RT::P,RT::V,<RT::T as Transport>::WriteStream>,
  managed : bool,
  s : &mut ClientSender<<RT::T as Transport>::WriteStream,<RT::P as Peer>::Shadow>,
  ) -> MDHTResult<(bool, Option<ClientSender<<RT::T as Transport>::WriteStream,<RT::P as Peer>::Shadow>>)> {
  let mut ok;
  let mut newcon = None;
  macro_rules! sendorconnect(($mess:expr,$oa:expr) => (
  {
    ok = send_msg($mess,$oa,s,&rc.msgenc,rc.peerrules.get_shadower(&(*p),&$mess));
    if !ok && managed {
      debug!("trying reconnection");
      match rc.transport.connectwith(&p.to_address(), Duration::seconds(CONNECT_DURATION)) {
        Ok((n,_)) => {
          // TODO if receive stream restart its server and shut xisting !!!
          debug!("reconnection in client process ok");
          // for now reconnect is only for threaded : see start_local TODO local spawn could use this
          // use new shadower (using existing conn would be unsafe)
          let mut cn = ClientSender::Threaded(n,p.get_shadower(true));
          // TODO send to peermgmet optional readTransportStream
          ok = send_msg($mess, $oa,&mut cn,&rc.msgenc,rc.peerrules.get_shadower(&(*p),&$mess));
          if ok {
            newcon = Some(cn);
          } else {
            try!(rp.peers.send(PeerMgmtMessage::PeerRemFromClient(p.clone(), PeerStateChange::Offline)));
          };
        },
        Err(e) => {
          warn!("Reconnect attempt failure {}",e);
        },
      };
    };
  }
  ));

  match m {
    ClientMessage::PeerPong(chal) => {
      let sign = rc.peerrules.signmsg(&(*rc.me), &chal);
      let mess : ProtoMessage<RT::P,RT::V> = ProtoMessage::PONG(rc.me.borrow(),sign);
      // simply send
      sendorconnect!(&mess,None);
    },
    ClientMessage::ShutDown => {
      warn!("shuting client");
      ok = false;
    },
    ClientMessage::PeerPing(p,chal) => {
      debug!("Sending ping {:?}", p.get_key());
      let sign = rc.peerrules.signmsg(&(*rc.me), &chal);
      // we do not wait for a result
      let mess : ProtoMessage<RT::P,RT::V> = ProtoMessage::PING(rc.me.borrow(), chal.clone(), sign);
      sendorconnect!(&mess,None);
    },
    // TODO oquery should be mutex handler over dist., + TODO send message of lessen to queryman when no query
    ClientMessage::PeerFind(nid, queryh, mut queryconf) => {
      // note that new queryid and recnode has been set in server
      // and query count in server -> amix and aproxy became as simple send
      // send query with hop + 1
      // local queryid set in server here update dest
      queryconf.dec_nbhop(&rc.rules);
      let mess  : ProtoMessage<RT::P,RT::V> = ProtoMessage::FINDNODE(queryconf, nid);
      sendorconnect!(&mess,None);
      if !ok {
        if queryh.lessen_query(1, &rp.peers, &rp.queries) {
          queryh.release_query(&rp.peers, &rp.queries)
        }
      }
    },
    ClientMessage::KVFind(nid, queryh, mut queryconf) => {
      // no adding query counter for peer
      // send query with hop + 1
      queryconf.dec_nbhop(&rc.rules);
      let mess  : ProtoMessage<RT::P,RT::V> = ProtoMessage::FINDVALUE(queryconf, nid);
      sendorconnect!(&mess,None);
      if !ok {
        // lessen TODO asynch is pow... cf lessen of peermanager
        if queryh.lessen_query(1, &rp.peers, &rp.queries) {
            queryh.release_query(&rp.peers, &rp.queries)
        }
      }
    },
    ClientMessage::StoreNode(oqid, r) => {
      let mess  : ProtoMessage<RT::P,RT::V> = ProtoMessage::STORENODE(oqid, r.as_ref().map(|v|DistantEnc(v.borrow())));
      sendorconnect!(&mess,None);
    },
    ClientMessage::StoreKV(oqid, chunk, r) => {
      match chunk {
        QueryChunk::Attachment => {
          let att = r.as_ref().and_then(|kv|kv.get_attachment().map(|p|p.clone()));
          let mess  : ProtoMessage<RT::P,RT::V> = ProtoMessage::STOREVALUE(oqid, r.as_ref().map(|v|DistantEnc(v)));
          sendorconnect!(&mess,att.as_ref());
        },
        _                     =>  {
          let mess  : ProtoMessage<RT::P,RT::V> = ProtoMessage::STOREVALUEATT(oqid, r.as_ref().map(|v|DistantEncAtt(v)));
          sendorconnect!(&mess,None);
        },
      };
    },
    ClientMessage::PeerAdd(..) => {
      error!("This should not be done in this function");
      ok = false;
    },
    ClientMessage::EndMult => {
      error!("This should not be done in this function");
      ok = false;
    },
  };
  Ok((ok, newcon))
}

// TODO use in start_local also
pub fn mult_client_connect <RT : RunningTypes>
 (p : &Arc<RT::P>,
  rc : &ArcRunningContext<RT>,
  rp : &RunningProcesses<RT>,
  ows : Option<<RT::T as Transport>::WriteStream>,
  ) -> MDHTResult<ClientSender<<RT::T as Transport>::WriteStream,<RT::P as Peer>::Shadow>> {
  match ows {
    Some(ws) => {
      let mut cn = ClientSender::Threaded(ws,p.get_shadower(true));
      Ok(cn)
    },
    None => {
      match rc.transport.connectwith(&p.to_address(), Duration::seconds(CONNECT_DURATION)) {
        Ok((n,ors)) => {
          debug!("connection of new mult client ok");
          // Send back read stream an connected status to peermanager
          match ors {
            None => (),
            Some(rs) => {
              let sh = try!(start_listener(rs,&rc,&rp)); // note that we instanciate two different shadower : shadower are not shared : even if the same transport is used to send and receive, two shadower are used (so two keys if sim key in shadower), a get_shadower_pair with communication between thread could be use to use the same key but it will require new method from shadow to get pair of shadower and care on having key set by only one side of comm (client but seems racy at least).
              // send si to peermanager
              try!(rp.peers.send(PeerMgmtMessage::ServerInfoFromClient(p.clone(), serverinfo_from_handle(&sh))));
            },
          };
 
          let mut cn = ClientSender::Threaded(n,p.get_shadower(true));
          Ok(cn)
        },
        Err(e) => {
          warn!("initial connection mult failure {}",e);
          try!(rp.peers.send(PeerMgmtMessage::PeerRemFromClient(p.clone(), PeerStateChange::Offline)));
          Err(e.into())
        },
      }
    },
  }
}


