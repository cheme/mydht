//! Server service aka receiving process
use std::mem::replace;
use super::deflocal::{
  LocalDest,
  GlobalCommand,
};
use super::peermgmt::{
  PeerMgmtCommand,
};
use super::mainloop::{
  MainLoopCommand,
};
use super::client2::{
  WriteCommand,
};

use utils::{
  Proto,
  Ref,
  SRef,
  SToRef,
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
};
use msgenc::{
  ProtoMessage,
};
use super::{
  MyDHTConf,
  MCCommand,
  ShadowAuthType,
  GlobalHandleSend,
  ApiHandleSend,
  MainLoopSendIn,
  ApiWeakSend,
  ApiWeakHandle,
  GlobalWeakSend,
  GlobalWeakHandle,
  LocalHandle,
  LocalSendIn,
  WriteWeakSend,
  WriteWeakHandle,
  WriteHandleSend,
  ReaderBorrowable,
};
use peer::Peer;
use keyval::{
  KeyVal,
  SettableAttachment,
  SettableAttachments,
};
use service::{
  send_with_handle,
  Service,
  YieldReturn,
  Spawner,
  SpawnSend,
  SpawnSendWithHandle,
  SpawnHandle,
  SpawnChannel,
  SpawnerYield,
  ReadYield,
};
use mydhtresult::{
  Error,
  ErrorKind,
  Result,
};


// TODO put in its own module
pub struct ReadService<MC : MyDHTConf> {
  stream : Option<<MC::Transport as Transport>::ReadStream>,
  is_auth : bool,
  enc : MC::MsgEnc,
//  from : PeerRefSend<MC>,
  from : MC::PeerRef,
  //with : Option<PeerRefSend<MC>>,
  with : Option<MC::PeerRef>,
  peermgmt : MC::PeerMgmtMeths,
  token : usize,
  prio : Option<PeerPriority>,
  shad_msg : Option<<MC::Peer as Peer>::ShadowRMsg>,
  local_sp : Option<(LocalSendIn<MC>, LocalHandle<MC>)>,
  local_service_proto : MC::LocalServiceProto,
  local_spawner : MC::LocalServiceSpawn,
  local_channel_in : MC::LocalServiceChannelIn,
  read_dest_proto : ReadDest<MC>,
  api_dest_proto : Option<ApiHandleSend<MC>>,
//      dhtrules_proto : conf.init_dhtrules_proto()?,
}

/// Note that local handle is not send : cannot restart if state suspend
pub struct ReadServiceSend<MC : MyDHTConf> {
  stream : Option<<MC::Transport as Transport>::ReadStream>,
  is_auth : bool,
  enc : MC::MsgEnc,
  from : <MC::PeerRef as SRef>::Send,
  with : Option<<MC::PeerRef as SRef>::Send>,
  peermgmt : MC::PeerMgmtMeths,
  token : usize,
  prio : Option<PeerPriority>,
  shad_msg : Option<<MC::Peer as Peer>::ShadowRMsg>,
  local_spawner : MC::LocalServiceSpawn,
  local_service_proto : MC::LocalServiceProto,
  local_channel_in : MC::LocalServiceChannelIn,
  read_dest_proto : ReadDest<MC>,
  api_dest_proto : Option<ApiHandleSend<MC>>,
}

impl<MC : MyDHTConf> SRef for ReadService<MC> where
  MC::LocalServiceSpawn : Send,
  MC::LocalServiceChannelIn : Send,
  <MC::PeerMgmtChannelIn as SpawnChannel<PeerMgmtCommand<MC>>>::Send : Send,
  <MC::LocalServiceChannelIn as SpawnChannel<MC::LocalServiceCommand>>::Send : Send,
  MainLoopSendIn<MC> : Send,
  ApiWeakSend<MC> : Send,
  ApiWeakHandle<MC> : Send,
  GlobalWeakSend<MC> : Send,
  GlobalWeakHandle<MC> : Send,
  WriteWeakSend<MC> : Send,
  WriteWeakHandle<MC> : Send,
  {
  type Send = ReadServiceSend<MC>;
  #[inline]
  fn get_sendable(self) -> Self::Send {
    let ReadService {
      stream, is_auth, enc,
      from,
      with,
      peermgmt, token, prio, shad_msg, 
      local_sp, 
      local_spawner, local_channel_in, read_dest_proto, api_dest_proto,
      local_service_proto,
    } = self;
    ReadServiceSend {
      stream, is_auth, enc,
      from : from.get_sendable(),
      with : with.map(|w|w.get_sendable()),
      peermgmt, token, prio, shad_msg, 
      local_spawner, local_channel_in, read_dest_proto, api_dest_proto,
      local_service_proto,
    } 
  }
}

impl<MC : MyDHTConf> SToRef<ReadService<MC>> for ReadServiceSend<MC> where
  MC::LocalServiceSpawn : Send,
  MC::LocalServiceChannelIn : Send,
  <MC::PeerMgmtChannelIn as SpawnChannel<PeerMgmtCommand<MC>>>::Send : Send,
  <MC::LocalServiceChannelIn as SpawnChannel<MC::LocalServiceCommand>>::Send : Send,
  MainLoopSendIn<MC> : Send,
  ApiWeakSend<MC> : Send,
  ApiWeakHandle<MC> : Send,
  GlobalWeakSend<MC> : Send,
  GlobalWeakHandle<MC> : Send,
  WriteWeakSend<MC> : Send,
  WriteWeakHandle<MC> : Send,
  {
  #[inline]
  fn to_ref(self) -> ReadService<MC> {
    let ReadServiceSend {
      stream, is_auth, enc,
      from,
      with,
      peermgmt, token, prio, shad_msg, 
      local_spawner, local_channel_in, read_dest_proto, api_dest_proto,
      local_service_proto,
    } = self;
    ReadService {
      stream, is_auth, enc,
      from : from.to_ref(),
      with : with.map(|w|w.to_ref()),
      peermgmt, token, prio, shad_msg, 
      local_sp : None,
      local_spawner, local_channel_in, read_dest_proto, api_dest_proto,
      local_service_proto,
    } 
  }
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
    local_service_proto : MC::LocalServiceProto,
    ) -> Self {
  //pub fn new(token :usize, rs : <MC::Transport as Transport>::ReadStream, me : PeerRefSend<MC>, with : Option<PeerRefSend<MC>>, enc : MC::MsgEnc, peermgmt : MC::PeerMgmtMeths) -> Self {
    let is_auth = if MC::AUTH_MODE == ShadowAuthType::NoAuth {
      true
    } else {
      false
    };
    ReadService {
      stream : Some(rs),
      is_auth : is_auth,
      enc : enc,
      from : me,
      with,
      peermgmt,
      token,
      prio : None,
      shad_msg : None,
      local_sp : None,
      local_spawner : local_spawn,
      local_channel_in,
      read_dest_proto,
      api_dest_proto,
      local_service_proto,
    }
  }
}


impl<MC : MyDHTConf> Service for ReadService<MC> {
  type CommandIn = ReadCommand<MC>;
  type CommandOut = ReadReply<MC>;

  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, async_yield : &mut S) -> Result<Self::CommandOut> {
    if let ReadCommand::ReadBorrowReturn(st,osh) = req {
      self.stream = Some(st);
      self.shad_msg = osh;
      return Ok(ReadReply::NoReply)
    }
    // TODO if command giving back a read stream : here
    if self.stream.is_none() {
      // borrowed stream
      match async_yield.spawn_yield() {
        YieldReturn::Return => return Err(Error("Would block".to_string(), ErrorKind::ExpectedError,None)),
        YieldReturn::Loop => (), 
      }
      // go back to command call for a possible ReadBorrowReturn
      return Ok(ReadReply::NoReply)
    }
    let opmess = {
      let ust = self.stream.as_mut().unwrap();
      let mut stream = ReadYield(ust, async_yield);
      match req {
        ReadCommand::ReadBorrowReturn(..) => unreachable!(),// done at call start
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
          let msg : ProtoMessage<MC::Peer> = receive_msg(&mut stream, &mut self.enc, &mut shad)?;

          match msg {
            ProtoMessage::PING(mut p, chal, sig) => {
              // attachment probably useless but if it is possible...
              let atsize = p.attachment_expected_size();
              if atsize > 0 {
                let att = receive_att(&mut stream, &mut self.enc, &mut shad, atsize)?;
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
                    return Ok(ReadReply::PeerMgmt(PeerMgmtCommand::Accept(pref.clone(),MainLoopCommand::NewPeerChallenge(pref,self.token,chal))));
                  } else {
                    // send RefPeer to peermgmt with new priority
//                    return Ok(ReadReply::PeerMgmt(PeerMgmtCommand::NewPrio(MC::PeerRef::new(p),peer_prio)))
//                    return Ok(ReadReply::NewPeer(MC::PeerRef::new(p),peer_prio,self.token,chal))
                    return Ok(ReadReply::MainLoop(MainLoopCommand::NewPeerChallenge(MC::PeerRef::new(p),self.token,chal)));
                  }
                } else {
                  // send refuse peer with token to mainloop 
                  return Ok(ReadReply::MainLoop(MainLoopCommand::RejectPeer(p.get_key(),None,Some(self.token))));
                }
              }

            },
            ProtoMessage::PONG(mut withpeer,initial_chal, sig, next_chal) => {
              debug!("a pong received from {:?}", withpeer.get_key_ref());

              let atsize = withpeer.attachment_expected_size();
              if atsize > 0 {
                let att = receive_att(&mut stream, &mut self.enc, &mut shad, atsize)?;
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
                debug!("server service switch to authenticated");
                self.is_auth = true;
                let pref = MC::PeerRef::new(withpeer);
                //self.with = Some(pref.get_sendable());
                self.with = Some(pref.clone());
                return Ok(ReadReply::MainLoop(MainLoopCommand::NewPeerUncheckedChallenge(pref,prio,self.token,initial_chal,next_chal)));
              }

            },
          }
          None
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
          let mut pmess : MC::ProtoMsg = receive_msg_msg(&mut stream, &mut self.enc, shad)?;
          let atts_s = pmess.attachment_expected_sizes();
          if atts_s.len() > 0 {
            let mut atts = Vec::with_capacity(atts_s.len());
            for atsize in atts_s {
              let att = receive_att(&mut stream, &mut self.enc, shad, atsize)?;
              atts.push(att);
            }
            pmess.set_attachments(&atts[..]);
          }
          Some(pmess)
        }
        }
      }
    };
    if let Some(pmess) = opmess {
      match pmess.into() {
        MCCommand::TryConnect(_,_) => {
          panic!(r#"Tryconnect command between peer required peer key (optional) and cache of under connection address,
          plus a method to block/allow it, none of it is done at this time
          //return Ok(ReadReply::MainLoop(MainLoopCommand::TryConnect(ad,None)));
          "#);
        },
        MCCommand::PeerStore(mess) => {
          return Ok(ReadReply::MainLoop(MainLoopCommand::PeerStore(GlobalCommand::Distant(self.with.clone(),mess))));
        },
        MCCommand::Global(mess) => {
          return Ok(ReadReply::Global(GlobalCommand::Distant(self.with.clone(),mess)))
        },
        MCCommand::Local(mut mess) => {

          let bre = mess.is_borrow_read_end();
          if mess.is_borrow_read() {
            // put read in msg plus slab ix
            let shad = replace(&mut self.shad_msg,None).unwrap();
            let stream = replace(&mut self.stream,None).unwrap();
            mess.put_read(stream,shad,self.token,&mut self.enc);
          }
          let mess = mess;
          let s_replace = if let Some((ref mut send, ref mut local_handle)) = self.local_sp {
            // try send in
            send_with_handle(send, local_handle, mess)?
          } else {
            let service = MC::init_local_service(self.local_service_proto.clone(),self.from.clone(),self.with.clone(),self.token)?;
            let (send,recv) = self.local_channel_in.new()?;
            let sender = LocalDest {
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
                let service = MC::init_local_service(self.local_service_proto.clone(),self.from.clone(),self.with.clone(),self.token)?;
                // TODO reinit channel and sender plus empty receiver in sender seems way better!!!
                self.local_spawner.spawn(service, sender, Some(mess), receiver, MC::LOCAL_SERVICE_NB_ITER)?
              } else {
                // restart
                self.local_spawner.spawn(service, sender, Some(mess), receiver, MC::LOCAL_SERVICE_NB_ITER)?
              };
              replace(&mut self.local_sp, Some((send,nlocal_handle)));
            }
          };
          if bre {
            // do not keep read open : end it (with mainloop msg to rebind read slab ix to dest
            // : a racy behavior here if read from local is already send and listen on and
            // not rewired in main process : when rewiring in main process it must unyield dest
            // !!)
            // for now end with error
            return Err(Error("End as borrow is not expected to be back".to_string(), ErrorKind::EndService,None));
          }
        },
      }

    }
        // for initial testing only TODO replace by deser
    /*    let mut buf = vec![0;4];
        let mut r = ReadYield(&mut self.stream, async_yield);
        r.read_exact(&mut buf).unwrap(); // unwrap for testring only
//        panic!("{:?}",&buf[..]);
        println!("{:?}",&buf[..]);
        assert!(&[1,2,3,4] == &buf[..]);
        buf[0]=9;*/
    Ok(ReadReply::NoReply)
  }
}
/// command for readservice
/// TODO a command to close read service cleanly (on peer refused for instance)
/// TODO a command to send weakchannel of write : probably some issue with threading here as a once
/// op : use of Box seems fine -> might be studier to end and restart with same state
/// Currently the channel in is not even use (Run send as first command)
pub enum ReadCommand<MC : MyDHTConf> {
  Run,
  ReadBorrowReturn(<MC::Transport as Transport>::ReadStream, Option<<MC::Peer as Peer>::ShadowRMsg>),
}

impl<MC : MyDHTConf> Proto for ReadCommand<MC> {
  fn get_new(&self) -> Self {
    ReadCommand::Run
  }
}

impl<MC : MyDHTConf> SRef for ReadCommand<MC> {
  type Send = ReadCommand<MC>;
  fn get_sendable(self) -> Self::Send {
    self
  }
}

impl<MC : MyDHTConf> SToRef<ReadCommand<MC>> for ReadCommand<MC> {
  fn to_ref(self) -> ReadCommand<MC> {
      self
  }
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
  pub mainloop : MainLoopSendIn<MC>,
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

impl<MC : MyDHTConf> SRef for ReadDest<MC> where
  <MC::PeerMgmtChannelIn as SpawnChannel<PeerMgmtCommand<MC>>>::Send : Send,
  MainLoopSendIn<MC> : Send,
  GlobalWeakSend<MC> : Send,
  GlobalWeakHandle<MC> : Send,
  WriteWeakSend<MC> : Send,
  WriteWeakHandle<MC> : Send,
  {
  type Send = ReadDest<MC>;
  fn get_sendable(self) -> Self::Send {
    self
  }
}

impl<MC : MyDHTConf> SToRef<ReadDest<MC>> for ReadDest<MC> where
  <MC::PeerMgmtChannelIn as SpawnChannel<PeerMgmtCommand<MC>>>::Send : Send,
  MainLoopSendIn<MC> : Send,
  GlobalWeakSend<MC> : Send,
  GlobalWeakHandle<MC> : Send,
  WriteWeakSend<MC> : Send,
  WriteWeakHandle<MC> : Send,
  {
  fn to_ref(self) -> ReadDest<MC> {
    self
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
      ReadReply::NewPeer(_pr,_pp,_tok,_chal) => {
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
