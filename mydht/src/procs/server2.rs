//! Server service aka receiving process
use super::peermgmt::{
  PeerMgmtCommand,
};
use super::mainloop::{
  MainLoopCommand,
  WriteHandleSend,
};
use super::client2::{
  WriteCommand,
  WriteService,
};

use utils::{
  Ref,
  ToRef,
  receive_msg,
  receive_att,
  shad_read_header,
  shad_read_end,
};
use peer::{
  PeerMgmtMeths,
  PeerPriority,
};
use std::borrow::Borrow;
use transport::{
  Transport,
  Address,
  Address as TransportAddress,
  SlabEntry,
  SlabEntryState,
  Registerable,
};
use msgenc::{
  MsgEnc,
  ProtoMessage,
};
use super::{
  MyDHTConf,
  PeerRefSend,
  ShadowAuthType,
};
use peer::Peer;
use keyval::{
  KeyVal,
  SettableAttachment,
};
use service::{
  HandleSend,
  Service,
  Spawner,
  SpawnSend,
  SpawnRecv,
  SpawnHandle,
  SpawnChannel,
  MioChannel,
  MioSend,
  MioRecv,
  NoYield,
  YieldReturn,
  SpawnerYield,
  SpawnUnyield,
  WriteYield,
  ReadYield,
  DefaultRecv,
  DefaultRecvChannel,
  NoRecv,
  NoSend,
};
use mydhtresult::{
  Result,
  Error,
  ErrorKind,
  ErrorLevel as MdhtErrorLevel,
};
use std::io::{
  Write,
  Read,
};


// TODO put in its own module
pub struct ReadService<MC : MyDHTConf> {
  stream : <MC::Transport as Transport>::ReadStream,
  is_auth : bool,
  enc : MC::MsgEnc,
  from : PeerRefSend<MC>,
  with : Option<PeerRefSend<MC>>,
  shad : Option<<MC::Peer as Peer>::ShadowRMsg>,
  peermgmt : MC::PeerMgmtMeths,
  token : usize,
  prio : Option<PeerPriority>,
//      dhtrules_proto : conf.init_dhtrules_proto()?,
}

impl<MC : MyDHTConf> ReadService<MC> {
  pub fn new(token :usize, rs : <MC::Transport as Transport>::ReadStream, me : PeerRefSend<MC>, with : Option<PeerRefSend<MC>>, enc : MC::MsgEnc, peermgmt : MC::PeerMgmtMeths) -> Self {
    let is_auth = if MC::AUTH_MODE == ShadowAuthType::NoAuth {
      true
    } else {
      false
    };
    ReadService {
      stream : rs,
      is_auth : is_auth,
      enc : enc,
      from : me,
      with : with,
      shad : None,
      peermgmt : peermgmt,
      token : token,
      prio : None,
    }
  }
}


impl<MDC : MyDHTConf> Service for ReadService<MDC> {
  type CommandIn = ReadCommand;
  type CommandOut = ReadReply<MDC>;

  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, async_yield : &mut S) -> Result<Self::CommandOut> {
    let mut stream = ReadYield(&mut self.stream, async_yield);
    match req {
      ReadCommand::Run => {
        let msg = if !self.is_auth {
          let mut shad = match MDC::AUTH_MODE {
            ShadowAuthType::NoAuth => {
              /*self.is_auth = true;
              return self.call(req,async_yield);*/
              unreachable!()
            },
            ShadowAuthType::Public => self.from.borrow().get_shadower_r_auth(),
            ShadowAuthType::Private => {
              match self.with {
                Some (ref w) => w.borrow().get_shadower_r_auth(),
                None => return Err(Error("No dest in read for private network, could not allow receive".to_string(),ErrorKind::Bug,None)),
              }
            },
          };

          shad_read_header(&mut shad, &mut stream)?;
          // read in single pass
          // TODO specialize ping pong messages with MaxSize. - 
          let msg : ProtoMessage<MDC::Peer,MDC::KeyVal> = receive_msg(&mut stream, &self.enc, &mut shad)?;

          match msg {
            ProtoMessage::PING(mut p, chal, sig) => {
              // attachment probably useless but if it is possible...
              let atsize = p.attachment_expected_size();
              if atsize > 0 {
                let att = receive_att(&mut stream, &self.enc, &mut shad, atsize)?;
                p.set_attachment(&att);
              }
              shad_read_end(&mut shad, &mut stream)?;
              // check sig
              if !self.peermgmt.checkmsg(&p,&chal[..],&sig[..]) {
                // send refuse peer with token to mainloop 
                return Ok(ReadReply::MainLoop(MainLoopCommand::RejectReadSpawn(self.token)));
              } else {
                if let Some(peer_prio) = self.peermgmt.accept(&p) {
                  self.prio = Some(peer_prio.clone());
                  if peer_prio == PeerPriority::Unchecked {
                    // send accept query to peermgmt service : it will update cache
                    let pref = MDC::PeerRef::new(p);
                    return Ok(ReadReply::PeerMgmt(PeerMgmtCommand::Accept(pref.clone(),MainLoopCommand::NewPeerChallenge(pref,peer_prio,self.token,chal))));
                  } else {
                    // send RefPeer to peermgmt with new priority
//                    return Ok(ReadReply::PeerMgmt(PeerMgmtCommand::NewPrio(MDC::PeerRef::new(p),peer_prio)))
//                    return Ok(ReadReply::NewPeer(MDC::PeerRef::new(p),peer_prio,self.token,chal))
                    return Ok(ReadReply::MainLoop(MainLoopCommand::NewPeerChallenge(MDC::PeerRef::new(p),peer_prio,self.token,chal)));
                  }
                } else {
                  // send refuse peer with token to mainloop 
                  return Ok(ReadReply::MainLoop(MainLoopCommand::RejectPeer(p.get_key(),None,Some(self.token))));
                }
              }

            },
            ProtoMessage::PONG(mut withpeer,initial_chal, sig, next_chal) => {
              let atsize = withpeer.attachment_expected_size();
              if atsize > 0 {
                let att = receive_att(&mut stream, &self.enc, &mut shad, atsize)?;
                withpeer.set_attachment(&att);
              }
              shad_read_end(&mut shad, &mut stream)?;
              // check sig
              if !self.peermgmt.checkmsg(&withpeer,&initial_chal[..],&sig[..]) {
                // send refuse peer with token to mainloop 
                return Ok(ReadReply::MainLoop(MainLoopCommand::RejectReadSpawn(self.token)));
              } else {
                let prio = match self.prio {
                  Some(ref prio) => prio.clone(),
                  None => {
                    if let Some(peer_prio) = self.peermgmt.accept(&withpeer) {
                      self.prio = Some(peer_prio.clone());
                      if peer_prio == PeerPriority::Unchecked {
                          // send accept query to peermgmt service : it will update cache
                          let pref = MDC::PeerRef::new(withpeer);
                          return Ok(ReadReply::PeerMgmt(PeerMgmtCommand::Accept(pref.clone(),MainLoopCommand::NewPeerUncheckedChallenge(pref.clone(),peer_prio,self.token,initial_chal,next_chal))));
                      } else {
                        peer_prio
                      }
                    } else {
                      return Ok(ReadReply::MainLoop(MainLoopCommand::RejectPeer(withpeer.get_key(),None,Some(self.token))));
                    }
                  },
                };
                return Ok(ReadReply::MainLoop(MainLoopCommand::NewPeerUncheckedChallenge(MDC::PeerRef::new(withpeer),prio,self.token,initial_chal,next_chal)));
              }

            },
            _ => return Err(Error("wrong state for peer not authenticated yet".to_string(),ErrorKind::PingError,None)),
          }
//pub fn receive_msg<P : Peer, V : KeyVal, T : Read, E : MsgEnc, S : ExtRead>(t : &mut T, e : &E, s : &mut S) -> MDHTResult<(ProtoMessage<P,V>, Option<Attachment>)> {
        } else {
        };
        // for initial testing only TODO replace by deser
    /*    let mut buf = vec![0;4];
        let mut r = ReadYield(&mut self.stream, async_yield);
        r.read_exact(&mut buf).unwrap(); // unwrap for testring only
//        panic!("{:?}",&buf[..]);
        println!("{:?}",&buf[..]);
        assert!(&[1,2,3,4] == &buf[..]);
        buf[0]=9;*/
      },
    }
    Ok(ReadReply::NoReply)
  }
}
/// command for readservice
#[derive(Clone)]
pub enum ReadCommand {
  Run,
}


pub enum ReadReply<MC : MyDHTConf> {
  MainLoop(MainLoopCommand<MC>),
  PeerMgmt(PeerMgmtCommand<MC>),
  Write(WriteCommand<MC>),
  /// proxy to mainloop new peer (which proxy to peermgmt new peer prio update), and sender pong
  /// (through mainloop or direct WriteCommand). TODO remove as use case requires a store of
  /// chalenge THEN a pong
  NewPeer(MC::PeerRef,PeerPriority,usize,Vec<u8>), 
  NoReply,
}

pub struct ReadDest<MDC : MyDHTConf> {
  pub mainloop : MioSend<<MDC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MDC>>>::Send>,
  // TODO switch to optionnal handle send similar to write
  pub peermgmt : <MDC::PeerMgmtChannelIn as SpawnChannel<PeerMgmtCommand<MDC>>>::Send,
  pub write : Option<WriteHandleSend<MDC>>,
}

impl<MDC : MyDHTConf> Clone for ReadDest<MDC> {
    fn clone(&self) -> Self {
      ReadDest{
        mainloop : self.mainloop.clone(),
        peermgmt : self.peermgmt.clone(),
        write : self.write.clone(),
      }
    }
}
impl<MDC : MyDHTConf> SpawnSend<ReadReply<MDC>> for ReadDest<MDC> {
  const CAN_SEND : bool = true;
  fn send(&mut self, r : ReadReply<MDC>) -> Result<()> {
    match r {
      ReadReply::MainLoop(mlc) => {
        self.mainloop.send(mlc)?
      },
      ReadReply::PeerMgmt(pmc) => self.peermgmt.send(pmc)?,
      ReadReply::Write(wc) => {
        let cwrite = match self.write {
          Some(ref mut w) => {
            if w.1.is_finished() {
              Some(wc)
            } else {
              w.send(wc)?;
              None
            }
          },
          None => {
            self.mainloop.send(MainLoopCommand::ProxyWrite(wc))?;
            None
          },
        };
        if let Some(wc) = cwrite {
          self.write = None;
          self.mainloop.send(MainLoopCommand::ProxyWrite(wc))?;
        }
      },
      ReadReply::NewPeer(pr,pp,tok,chal) => {
        panic!("unused TODO remove??");
//        self.send(ReadReply::MainLoop(MainLoopCommand::NewPeer(pr.clone(),pp,Some(tok))))?;
 //       self.send(ReadReply::Write(WriteCommand::Pong(pr,chal,rtok)))?;
      },
      ReadReply::NoReply => (),
    }
    Ok(())
  }
}

// sender need :
// mainloop -> refuse peer with token (drop ws peer should not be remove from peer mgmt)
// peermgmt -> update priority
//             query priority (asynch accept)
// client2 -> send pong
