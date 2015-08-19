//! Client process : used to send message to distant peers, either one time (non connected
//! transport) or multiple time (connected transport and listening on channel (stored in
//! peermanager) for instructions.



use procs::mesgs::{self,PeerMgmtMessage,ClientMessage,ClientMessageIx};
use mydhtresult::Result as MydhtResult;
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
use msgenc::{MsgEnc,DistantEncAtt,DistantEnc};
use procs::server::start_listener;
use procs::server::{serverinfo_from_handle};
use route::{ClientSender};
use route::send_msg;
use msgenc::send_variant::ProtoMessage;
use std::borrow::Borrow;

pub fn start<RT : RunningTypes>
 (p : &Arc<RT::P>,
  orcl : Option<Receiver<ClientMessageIx<RT::P,RT::V>>>,
  rc : ArcRunningContext<RT>,
  rp : RunningProcesses<RT>,
  omessage : Option<ClientMessage<RT::P,RT::V>>,
  mut osend : Option<ClientSender<<RT::T as Transport>::WriteStream>>,
  managed : bool,
  ) -> bool {
  match start_local(p,orcl,&rc,&rp,omessage,osend.as_mut(),managed) {
    Ok(r) => r,
    Err(e) => {
      // peerremfromclient send from client_match directly (no need to try reconnect
      error!("client error : {}",e);
      false
    },
  }
}
 
pub struct CleanableReceiver<'a, RT : RunningTypes>(Receiver<ClientMessageIx<RT::P,RT::V>>, &'a Sender<mesgs::PeerMgmtMessage<RT::P,RT::V,<RT::T as Transport>::ReadStream,<RT::T as Transport>::WriteStream>>)
where RT::A : 'a, 
      RT::P : 'a,
      <RT::P as KeyVal>::Key : 'a,
      RT::V : 'a,
      <RT::V as KeyVal>::Key : 'a,
      <RT::T as Transport>::ReadStream : 'a,
      <RT::T as Transport>::WriteStream : 'a
;

impl<'a,RT : RunningTypes>  Drop for CleanableReceiver<'a,RT> 
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
        Ok((ClientMessage::PeerFind(_,Some(query), _),ix)) | Ok((ClientMessage::KVFind(_,Some(query), _),ix)) => {
          debug!("- emptying channel found query");
          query.lessen_query(1, self.1);
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
  orcl : Option<Receiver<ClientMessageIx<RT::P,RT::V>>>,
  rc : &ArcRunningContext<RT>,
  rp : &RunningProcesses<RT>,
  omessage : Option<ClientMessage<RT::P,RT::V>>,
  osend : Option<&mut ClientSender<<RT::T as Transport>::WriteStream>>,
  managed : bool,
  ) -> MydhtResult<bool> {
  let mut ok = true;
  // connect
  let mut onewsend : Option<ClientSender<<RT::T as Transport>::WriteStream>> = if osend.is_none() {
    // TODO duration from rules!!!
    match rc.transport.connectwith(&p.to_address(), Duration::seconds(5)) {
      Ok((ws, ors)) => {
        // Send back read stream an connected status to peermanager
        match ors {
          None => (),
          Some(rs) => {
            let sh = try!(start_listener(rs,&rc,&rp));
            // send si to peermanager
            rp.peers.send(PeerMgmtMessage::ServerInfoFromClient(p.clone(), serverinfo_from_handle(&sh)));
          },
        };
        Some(ClientSender::Threaded(ws))
      },
      Err(e) => {
        debug!("connection with client failed in client process {}",e);
        None
      },
    }
  } else {
    None
  };
  let sc : &mut ClientSender<<RT::T as Transport>::WriteStream> = match osend {
    None => match &mut onewsend {
      &mut None => {
        debug!("cannot connect with peer returning false");
        info!("Cannot connect");
        rp.peers.send(PeerMgmtMessage::PeerRemFromClient(p.clone(), PeerStateChange::Offline));
        // send no connection possible : 
        return Ok(false);
      },
      &mut Some(ref mut ns) => ns,
    },
    Some(mut cs) => cs,
  };
  // TODO if needed start receive and send handle to peermgmt!!!
  // closure to send message with one reconnect try on error
  if managed {
    // loop on rcl
    match orcl {
      None => match omessage {
        None =>  {
          error!("Unexpected client process without channel or message")
        },
        Some(m) => {
          let (okrecv, _) = try!(client_match(p,&rc,&rp,m,false, sc));
        },
      },
      Some(r) => {
        let cr : CleanableReceiver<RT> = CleanableReceiver(r,&rp.peers);
        loop {
          match cr.0.recv() {
            Err(_) => {
              error!("Client channel issue");
              break;
            },
            Ok((m,ix)) => {
              let (okrecv, s) = try!(client_match(p,&rc,&rp,m,true, sc)); 
              match s {
                // update of stream is for possible reconnect
                // return &'a mut on reconnect
                Some(s) => *sc = s,
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
  Ok(ok)
}



#[inline]
/// manage new client message
/// return a new transport if reconnected and a boolean if no more connectivity : 
/// all other issue than offline are managed by returning errors
pub fn client_match <RT : RunningTypes>
 (p : &Arc<RT::P>,
  rc : &ArcRunningContext<RT>,
  rp : &RunningProcesses<RT>,
  m : ClientMessage<RT::P,RT::V>,
  managed : bool,
  s : &mut ClientSender<<RT::T as Transport>::WriteStream>,
  ) -> MydhtResult<(bool, Option<ClientSender<<RT::T as Transport>::WriteStream>>)> {
  let mut ok = true;
  let mut newcon = None;
  macro_rules! sendorconnect(($mess:expr,$oa:expr) => (
  {
    ok = send_msg($mess,$oa,s,&rc.msgenc);
    if !ok && managed {
      debug!("trying reconnection");
      rc.transport.connectwith(&p.to_address(), Duration::seconds(2)).map(|(mut n,_)|{
        // TODO if receive stream restart its server and shut xistring
        debug!("reconnection in client process ok");
        // for now reconnect is only for threaded : see start_local TODO local spawn could use this
        let mut cn = ClientSender::Threaded(n);
        // TODO send to peermgmet optional writeTransportStream
        ok = send_msg($mess, $oa,&mut cn,&rc.msgenc);
        if ok {
          newcon = Some(cn);
        } else {
          rp.peers.send(PeerMgmtMessage::PeerRemFromClient(p.clone(), PeerStateChange::Offline));
        };
      });
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
    ClientMessage::PeerFind(nid, oquery, mut queryconf) =>  {
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
      let mess  : ProtoMessage<RT::P,RT::V> = ProtoMessage::STORE_NODE(oqid, r.as_ref().map(|v|DistantEnc(v.borrow())));
      sendorconnect!(&mess,None);
    },
    ClientMessage::StoreKV(oqid, chunk, r) => {
      match chunk {
        QueryChunk::Attachment => {
          let att = r.as_ref().and_then(|kv|kv.get_attachment().map(|p|p.clone()));
          let mess  : ProtoMessage<RT::P,RT::V> = ProtoMessage::STORE_VALUE(oqid, r.as_ref().map(|v|DistantEnc(v)));
          sendorconnect!(&mess,att.as_ref());
        },
        _                     =>  {
          let mess  : ProtoMessage<RT::P,RT::V> = ProtoMessage::STORE_VALUE_ATT(oqid, r.as_ref().map(|v|DistantEncAtt(v)));
          sendorconnect!(&mess,None);
        },
      };
    },
  };
  Ok((ok, newcon))
}


