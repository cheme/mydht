//! Server service aka receiving process
use std::mem::replace;
use super::storeprop::{
  KVStoreCommand,
};
use super::deflocal::{
  LocalDest,
  GlobalCommand,
};
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
  receive_msg,
  receive_msg_msg,
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
  MCCommand,
  PeerRefSend,
  ShadowAuthType,
  GlobalHandleSend,
  ApiHandleSend,
  OptInto,
};
use peer::Peer;
use keyval::{
  KeyVal,
  SettableAttachment,
  SettableAttachments,
};
use service::{
  send_with_handle,
  HandleSend,
  Service,
  Spawner,
  SpawnSend,
  SpawnSendWithHandle,
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
//  from : PeerRefSend<MC>,
  from : MC::PeerRef,
  //with : Option<PeerRefSend<MC>>,
  with : Option<MC::PeerRef>,
  shad : Option<<MC::Peer as Peer>::ShadowRMsg>,
  peermgmt : MC::PeerMgmtMeths,
  token : usize,
  prio : Option<PeerPriority>,
  shad_msg : Option<<MC::Peer as Peer>::ShadowRMsg>,
  local_sp : Option<(
    <MC::LocalServiceChannelIn as SpawnChannel<MC::LocalServiceCommand>>::Send, 
    <MC::LocalServiceSpawn as Spawner<
      MC::LocalService,
      LocalDest<MC>,
      <MC::LocalServiceChannelIn as SpawnChannel<MC::LocalServiceCommand>>::Recv
    >>::Handle)>,
  local_spawner : MC::LocalServiceSpawn,
  local_channel_in : MC::LocalServiceChannelIn,
  read_dest_proto : ReadDest<MC>,
  api_dest_proto : Option<ApiHandleSend<MC>>,
//      dhtrules_proto : conf.init_dhtrules_proto()?,
}

impl<MC : MyDHTConf> ReadService<MC> {
  pub fn new(
    token :usize, 
    rs : <MC::Transport as Transport>::ReadStream, 
    me : MC::PeerRef, with : Option<MC::PeerRef>, 
    enc : MC::MsgEnc, 
    peermgmt : MC::PeerMgmtMeths, 
    local_spawn : MC::LocalServiceSpawn, 
    local_channel_in : MC::LocalServiceChannelIn,
    read_dest_proto : ReadDest<MC>,
    api_dest_proto : Option<ApiHandleSend<MC>>,
    ) -> Self {
  //pub fn new(token :usize, rs : <MC::Transport as Transport>::ReadStream, me : PeerRefSend<MC>, with : Option<PeerRefSend<MC>>, enc : MC::MsgEnc, peermgmt : MC::PeerMgmtMeths) -> Self {
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
      shad_msg : None,
      local_sp : None,
      local_spawner : local_spawn,
      local_channel_in : local_channel_in,
      read_dest_proto : read_dest_proto,
      api_dest_proto : api_dest_proto,
    }
  }
}


impl<MC : MyDHTConf> Service for ReadService<MC> {
  type CommandIn = ReadCommand;
  type CommandOut = ReadReply<MC>;

  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, async_yield : &mut S) -> Result<Self::CommandOut> {
    let mut stream = ReadYield(&mut self.stream, async_yield);
    match req {
      ReadCommand::Run => {
        if !self.is_auth {
          let mut shad = match MC::AUTH_MODE {
            ShadowAuthType::NoAuth => {
              /*self.is_auth = true;
              return self.call(req,async_yield);*/
              unreachable!()
            },
            ShadowAuthType::Public | ShadowAuthType::Private => self.from.borrow().get_shadower_r_auth(),
/*            ShadowAuthType::Private => {
              match self.with {
                Some (ref w) => w.borrow().get_shadower_r_auth(),
                None => return Err(Error("No dest in read for private network, could not allow receive".to_string(),ErrorKind::Bug,None)),
              }
            },*/
          };

          shad_read_header(&mut shad, &mut stream)?;
          // read in single pass
          // TODO specialize ping pong messages with MaxSize. - 
          let msg : ProtoMessage<MC::Peer> = receive_msg(&mut stream, &self.enc, &mut shad)?;

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
                    let pref = MC::PeerRef::new(p);
                    return Ok(ReadReply::PeerMgmt(PeerMgmtCommand::Accept(pref.clone(),MainLoopCommand::NewPeerChallenge(pref,peer_prio,self.token,chal))));
                  } else {
                    // send RefPeer to peermgmt with new priority
//                    return Ok(ReadReply::PeerMgmt(PeerMgmtCommand::NewPrio(MC::PeerRef::new(p),peer_prio)))
//                    return Ok(ReadReply::NewPeer(MC::PeerRef::new(p),peer_prio,self.token,chal))
                    return Ok(ReadReply::MainLoop(MainLoopCommand::NewPeerChallenge(MC::PeerRef::new(p),peer_prio,self.token,chal)));
                  }
                } else {
                  // send refuse peer with token to mainloop 
                  return Ok(ReadReply::MainLoop(MainLoopCommand::RejectPeer(p.get_key(),None,Some(self.token))));
                }
              }

            },
            ProtoMessage::PONG(mut withpeer,initial_chal, sig, next_chal) => {
              println!("a pong received");

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
                          let pref = MC::PeerRef::new(withpeer);
                          // not really auth actually : could still be refused, but message
                          // received next will not be auth message (service drop on failure so if
                          // new connect it is on new service)
                          self.is_auth = true;
                          // must update in case of shared secret update for msg shadower
                          //self.with = Some(pref.get_sendable());
                          self.with = Some(pref.clone());
                          return Ok(ReadReply::PeerMgmt(PeerMgmtCommand::Accept(pref.clone(),MainLoopCommand::NewPeerUncheckedChallenge(pref,peer_prio,self.token,initial_chal,next_chal))));
                      } else {
                        peer_prio
                      }
                    } else {
                      return Ok(ReadReply::MainLoop(MainLoopCommand::RejectPeer(withpeer.get_key(),None,Some(self.token))));
                    }
                  },
                };
                println!("is auth set to thrue");
                self.is_auth = true;
                let pref = MC::PeerRef::new(withpeer);
                //self.with = Some(pref.get_sendable());
                self.with = Some(pref.clone());
                return Ok(ReadReply::MainLoop(MainLoopCommand::NewPeerUncheckedChallenge(pref,prio,self.token,initial_chal,next_chal)));
              }

            },
            _ => return Err(Error("wrong state for peer not authenticated yet".to_string(),ErrorKind::PingError,None)),
          }
//pub fn receive_msg<P : Peer, V : KeyVal, T : Read, E : MsgEnc, S : ExtRead>(t : &mut T, e : &E, s : &mut S) -> MDHTResult<(ProtoMessage<P,V>, Option<Attachment>)> {
        } else {
          // init shad if needed
          if self.shad_msg.is_none() {
            let mut shad = 
                self.from.borrow().get_shadower_r_msg();
/*match MC::AUTH_MODE {
              ShadowAuthType::NoAuth => {
                self.from.borrow().get_shadower_r_msg()
              },
              ShadowAuthType::Public | ShadowAuthType::Private => {
                match self.with {
                  Some(ref w) => w.borrow().get_shadower_r_msg(),
                  None => {return Err(Error("reader set as auth but no with peer and not NoAuth auth".to_string(), ErrorKind::Bug,None));},
                }
              },
            };*/
            shad_read_header(&mut shad, &mut stream)?;

            self.shad_msg = Some(shad);
          }
          let shad = self.shad_msg.as_mut().unwrap();
          let mut pmess : MC::ProtoMsg = receive_msg_msg(&mut stream, &self.enc, shad)?;
          let atts_s = pmess.attachment_expected_sizes();
          if atts_s.len() > 0 {
            let mut atts = Vec::with_capacity(atts_s.len());
            for atsize in atts_s {
              let att = receive_att(&mut stream, &self.enc, shad, atsize)?;
              atts.push(att);
            }
            pmess.set_attachments(&atts[..]);
          }

          match pmess.into() {
            MCCommand::TryConnect(_,_) => {
              panic!(r#"Tryconnect command between peer required peer key (optional) and cache of under connection address,
              plus a method to block/allow it, none of it is done at this time
              //return Ok(ReadReply::MainLoop(MainLoopCommand::TryConnect(ad,None)));
              "#);
            },
            MCCommand::PeerStore(mess) => {
              return Ok(ReadReply::MainLoop(MainLoopCommand::PeerStore(GlobalCommand(self.with.clone(),mess))));
            },
            MCCommand::Global(mess) => {
              return Ok(ReadReply::Global(GlobalCommand(self.with.clone(),mess)))
            },
            MCCommand::Local(mess) => {

              let s_replace = if let Some((ref mut send, ref mut local_handle)) = self.local_sp {
                // try send in
                send_with_handle(send, local_handle, mess)?
              } else {
                let service = MC::init_local_service(self.from.clone(),self.with.clone())?;
                let (send,recv) = self.local_channel_in.new()?;
                let sender = LocalDest{
                  read : self.read_dest_proto.clone(),
                  api : self.api_dest_proto.clone(),
                };
                let local_handle = self.local_spawner.spawn(service, sender, Some(mess), recv, MC::LOCAL_SERVICE_NB_ITER)?;
                self.local_sp = Some((send,local_handle));
                None
              };
              if let Some(mess) = s_replace {
                let lh = replace(&mut self.local_sp, None);
                if let Some((send, local_handle)) = lh {
                  let (service, sender, receiver, res) = local_handle.unwrap_state()?;
                  let nlocal_handle = if res.is_err() {
                    // TODO log try restart ???
                    // reinit service, reuse receiver as may not be empty (do not change our send)
                    let service = MC::init_local_service(self.from.clone(),self.with.clone())?;
                    // TODO reinit channel and sender plus empty receiver in sender seems way better!!!
                    self.local_spawner.spawn(service, sender, Some(mess), receiver, MC::LOCAL_SERVICE_NB_ITER)?
                  } else {
                    // restart
                    self.local_spawner.spawn(service, sender, Some(mess), receiver, MC::LOCAL_SERVICE_NB_ITER)?
                  };
                  replace(&mut self.local_sp, Some((send,nlocal_handle)));
                }
              }
            },
          }

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
/// TODO a command to close read service cleanly (on peer refused for instance)
/// TODO a command to send weakchannel of write : probably some issue with threading here as a once
/// op : use of Box seems fine -> might be studier to end and restart with same state
/// Currently the channel in is not even use (Run send as first command)
#[derive(Clone)]
pub enum ReadCommand {
  Run,
}


pub enum ReadReply<MC : MyDHTConf> {
  Global(GlobalCommand<MC::PeerRef,MC::GlobalServiceCommand>),
  MainLoop(MainLoopCommand<MC>),
  PeerMgmt(PeerMgmtCommand<MC>),
  Write(WriteCommand<MC>),
  /// proxy to mainloop new peer (which proxy to peermgmt new peer prio update), and sender pong
  /// (through mainloop or direct WriteCommand). TODO remove as use case requires a store of
  /// chalenge THEN a pong
  NewPeer(MC::PeerRef,PeerPriority,usize,Vec<u8>), 
  NoReply,
}

pub struct ReadDest<MC : MyDHTConf> {
  pub mainloop : MioSend<<MC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MC>>>::Send>,
  // TODO switch to optionnal handle send similar to write
  pub peermgmt : <MC::PeerMgmtChannelIn as SpawnChannel<PeerMgmtCommand<MC>>>::Send,
  pub global : Option<GlobalHandleSend<MC>>,
  pub write : Option<WriteHandleSend<MC>>,
  pub read_token : usize,
}

impl<MC : MyDHTConf> Clone for ReadDest<MC> {
    fn clone(&self) -> Self {
      ReadDest{
        mainloop : self.mainloop.clone(),
        peermgmt : self.peermgmt.clone(),
        global : self.global.clone(),
        write : self.write.clone(),
        read_token : self.read_token,
      }
    }
}
impl<MC : MyDHTConf> SpawnSend<ReadReply<MC>> for ReadDest<MC> {
  const CAN_SEND : bool = true;
  fn send(&mut self, r : ReadReply<MC>) -> Result<()> {
    match r {
      ReadReply::MainLoop(mlc) => {
        self.mainloop.send(mlc)?
      },
      ReadReply::Global(mlc) => {
        let cml = match self.global {
          Some(ref mut w) => {
            w.send_with_handle(mlc)?
          },
          None => {
            Some(mlc)
          },
        };
        if let Some(c) = cml {
          self.global = None;
          self.mainloop.send(MainLoopCommand::ProxyGlobal(c))?;
        }
      },
      ReadReply::PeerMgmt(pmc) => self.peermgmt.send(pmc)?,
      ReadReply::Write(wc) => {
        let cwrite = match self.write {
          Some(ref mut w) => {
            w.send_with_handle(wc)?
          },
          None => {
            self.mainloop.send(MainLoopCommand::ProxyWrite(self.read_token,wc))?;
            None
          },
        };
        if let Some(wc) = cwrite {
          self.write = None;
          self.mainloop.send(MainLoopCommand::ProxyWrite(self.read_token,wc))?;
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
