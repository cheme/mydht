//! Client service
use procs::OptInto;
use std::borrow::Borrow;
use peer::Peer;
use peer::{
  PeerMgmtMeths,
  PeerPriority,
};
use super::api::{
  ApiQueryId,
  ApiQueriable,
  ApiRepliable,
};
use keyval::{
  KeyVal,
  SettableAttachment,
  SettableAttachments,
  GettableAttachments,
};
use transport::{
  Transport,
  Address,
  Address as TransportAddress,
  SlabEntry,
  SlabEntryState,
  Registerable,
};
use utils::{
  Ref,
  SRef,
  SToRef,
  shad_write_header,
  shad_write_end,
  send_msg,
  send_msg_msg,
  send_att,
};
use super::{
  MyDHTConf,
  PeerRefSend,
  ShadowAuthType,
  MCCommand,
};
use msgenc::{
  MsgEnc,
};
use msgenc::send_variant::{
  ProtoMessage,
};


use service::{
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
use super::api::ApiReply;

// TODO put in its own module
pub struct WriteService<MC : MyDHTConf> {
  stream : <MC::Transport as Transport>::WriteStream,
  is_auth : bool,
  enc : MC::MsgEnc,
  from : MC::PeerRef,
//  from : PeerRefSend<MC>,
  //with : Option<PeerRefSend<MC>>,
  with : Option<MC::PeerRef>,
  peermgmt : MC::PeerMgmtMeths,
  token : usize,
  shad_msg : Option<<MC::Peer as Peer>::ShadowWMsg>,
}

impl<MC : MyDHTConf> WriteService<MC> {
  //pub fn new(token : usize, ws : <MC::Transport as Transport>::WriteStream, me : PeerRefSend<MC>, with : Option<PeerRefSend<MC>>, enc : MC::MsgEnc, peermgmt : MC::PeerMgmtMeths) -> Self {
  pub fn new(token : usize, ws : <MC::Transport as Transport>::WriteStream, me : MC::PeerRef, with : Option<MC::PeerRef>, enc : MC::MsgEnc, peermgmt : MC::PeerMgmtMeths) -> Self {
    WriteService {
      stream : ws,
      is_auth : false,
      enc : enc,
      from : me,
      with : with,
      peermgmt : peermgmt,
      token : token,
      shad_msg : None,
    }
  }
}


pub fn get_shad_auth<MC : MyDHTConf>(from : &MC::PeerRef,with : &Option<MC::PeerRef>) -> <MC::Peer as Peer>::ShadowWAuth {
//pub fn get_shad_auth<MC : MyDHTConf>(from : &PeerRefSend<MC>,with : &Option<PeerRefSend<MC>>) -> <MC::Peer as Peer>::ShadowWAuth {
  match MC::AUTH_MODE {
    ShadowAuthType::NoAuth => {
      /*self.is_auth = true;
      return self.call(req,async_yield);*/
      unreachable!()
    },
    ShadowAuthType::Public => from.borrow().get_shadower_w_auth(),
    ShadowAuthType::Private => {
      match with {
        &Some (ref w) => w.borrow().get_shadower_w_auth(),
        &None => unreachable!(), // See previous setting of self.with 
          // return Err(Error("No dest in with for private network, could not allow pong".to_string(),ErrorKind::Bug,None)),
      }
    },
  }
}

impl<MC : MyDHTConf> Service for WriteService<MC> {
  type CommandIn = WriteCommand<MC>;
  type CommandOut = WriteReply<MC>;

  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, async_yield : &mut S) -> Result<Self::CommandOut> {
    match req {
      WriteCommand::Write => {
        let mut stream = WriteYield(&mut self.stream, async_yield);
        // for initial testing only TODO replace by deser
        let buf = &[1,2,3,4];
//        let mut w = WriteYield(&mut self.stream, async_yield);
        stream.write_all(buf).unwrap(); // unwrap for testing only (thread without error catching
      },
      WriteCommand::Pong(rp,chal,read_token,option_chal2) => {
        let mut stream = WriteYield(&mut self.stream, async_yield);
        // update dest : this ensure that on after auth with is initialized! Remember that with may
        // contain initializing shared secret for message shadow.
        self.with = Some(rp);
        let sig = self.peermgmt.signmsg(self.from.borrow(), &chal[..]);
        let pmess : ProtoMessage<MC::Peer> = ProtoMessage::PONG(self.from.borrow(),chal,sig,option_chal2);
        // once shadower
        let mut shad = get_shad_auth::<MC>(&self.from,&self.with);
        shad_write_header(&mut shad, &mut stream)?;

        send_msg(&pmess, &mut stream, &self.enc, &mut shad)?;
        if let Some(ref att) = self.from.borrow().get_attachment() {
          send_att(att, &mut stream, &self.enc, &mut shad)?;
        }

        shad_write_end(&mut shad, &mut stream)?;
        stream.flush()?;
      },
      WriteCommand::Ping(chal) => {
        let mut stream = WriteYield(&mut self.stream, async_yield);
 //       let chal = self.peermgmt.challenge(self.from.borrow());
        let sign = self.peermgmt.signmsg(self.from.borrow(), &chal);
        // we do not wait for a result
        let pmess : ProtoMessage<MC::Peer> = ProtoMessage::PING(self.from.borrow(), chal.clone(), sign);
        let mut shad = get_shad_auth::<MC>(&self.from,&self.with);
        shad_write_header(&mut shad, &mut stream)?;

        send_msg(&pmess, &mut stream, &self.enc, &mut shad)?;
        if let Some(ref att) = self.from.borrow().get_attachment() {
          send_att(att, &mut stream, &self.enc, &mut shad)?;
        }

        shad_write_end(&mut shad, &mut stream)?;
        stream.flush()?;
//        return Ok(WriteReply::MainLoop(MainLoopCommand::NewChallenge(self.token,chal)));

      },
      WriteCommand::Service(command) => {
        self.forward_proto(command,async_yield)?;
      },
/*      WriteCommand::GlobalService(command) => {
        self.forward_proto(MCCommand::Global(command),async_yield)?;
      },*/
    }
    // default to no rep
    Ok(WriteReply::NoRep)
  }
}

impl<MC : MyDHTConf> WriteService<MC> {

  fn forward_proto<S : SpawnerYield, R : OptInto<MC::ProtoMsg>>(&mut self, command: R, async_yield : &mut S) -> Result<()> {
    if command.can_into() {
      let mut stream = WriteYield(&mut self.stream, async_yield);
      if self.shad_msg.is_none() {
        let mut shad = match MC::AUTH_MODE {
          ShadowAuthType::NoAuth => {
            self.from.borrow().get_shadower_w_msg()
          },
          ShadowAuthType::Public | ShadowAuthType::Private => {
            match self.with {
              Some(ref w) => w.borrow().get_shadower_w_msg(),
              None => {return Err(Error("route return slab may contain write ref of non initialized (connected), a route impl issue".to_string(), ErrorKind::Bug,None));},
            }
          },
        };
        // write head before storing
        shad_write_header(&mut shad, &mut stream)?;
        self.shad_msg = Some(shad);
      }

      let mut shad = self.shad_msg.as_mut().unwrap();
      let pmess = command.opt_into().unwrap();
      send_msg_msg(&pmess, &mut stream, &self.enc, &mut shad)?;
      for att in pmess.get_attachments() {
        send_att(att, &mut stream, &self.enc, &mut shad)?;
      }
    }

    Ok(())
  }
}
/// command for readservice
pub enum WriteCommand<MC : MyDHTConf> {
  Write,
  /// ping TODO chal as Ref<Vec<u8>>
  Ping(Vec<u8>),
  /// pong a peer with challenge and read token last is second challenge if needed (needed when
  /// replying to a ping not a pong)
  Pong(MC::PeerRef, Vec<u8>, usize, Option<Vec<u8>>),
  Service(MCCommand<MC>),
  //GlobalService(MC::GlobalServiceCommand),
}

impl<MC : MyDHTConf> WriteCommand<MC> {
  pub fn get_api_reply(&self) -> Option<ApiQueryId> {
    match *self {
      WriteCommand::Service(ref lsc) => lsc.get_api_reply(),
      _ => None,
    }
  }
}


pub enum WriteCommandSend<MC : MyDHTConf> 
  where MC::LocalServiceCommand : SRef,
        MC::GlobalServiceCommand : SRef,
  {
  Write,
  /// Vec<u8> being chalenge store in mainloop process
  Ping(Vec<u8>),
  /// pong a peer with challenge and read token
  Pong(<MC::PeerRef as SRef>::Send, Vec<u8>, usize, Option<Vec<u8>>),
  Service(<MCCommand<MC> as SRef>::Send),
  //GlobalService(<MC::GlobalServiceCommand as SRef>::Send),
}

impl<MC : MyDHTConf> Clone for WriteCommand<MC> {
  fn clone(&self) -> Self {
    match *self {
      WriteCommand::Write => WriteCommand::Write,
      WriteCommand::Ping(ref chal) => WriteCommand::Ping(chal.clone()),
      WriteCommand::Pong(ref pr,ref v,s,ref v2) => WriteCommand::Pong(pr.clone(),v.clone(),s,v2.clone()),
      WriteCommand::Service(ref p) => WriteCommand::Service(p.clone()),
      //WriteCommand::GlobalService(ref p) => WriteCommand::GlobalService(p.clone()),
    }
  }
}
impl<MC : MyDHTConf> SRef for WriteCommand<MC>
  where MC::LocalServiceCommand : SRef,
        MC::GlobalServiceCommand : SRef,
  {
  type Send = WriteCommandSend<MC>;
  fn get_sendable(&self) -> Self::Send {
    match *self {
      WriteCommand::Write => WriteCommandSend::Write,
      WriteCommand::Ping(ref chal) => WriteCommandSend::Ping(chal.clone()),
      WriteCommand::Pong(ref pr,ref v,s,ref v2) => WriteCommandSend::Pong(pr.get_sendable(),v.clone(),s,v2.clone()),
      WriteCommand::Service(ref p) => WriteCommandSend::Service(p.get_sendable()),
//      WriteCommand::GlobalService(ref p) => WriteCommandSend::GlobalService(p.get_sendable()),
    }
  }
}
impl<MC : MyDHTConf> SToRef<WriteCommand<MC>> for WriteCommandSend<MC>
  where MC::LocalServiceCommand : SRef,
        MC::GlobalServiceCommand : SRef,
  {
  fn to_ref(self) -> WriteCommand<MC> {
    match self {
      WriteCommandSend::Write => WriteCommand::Write,
      WriteCommandSend::Ping(chal) => WriteCommand::Ping(chal),
      WriteCommandSend::Pong(pr,v,s,v2) => WriteCommand::Pong(pr.to_ref(),v,s,v2),
      WriteCommandSend::Service(p) => WriteCommand::Service(p.to_ref()),
      //WriteCommandSend::GlobalService(p) => WriteCommand::GlobalService(p.to_ref()),
    }
  }
}

/// TODO Api is not use : remove it and default to NoSend 
pub enum WriteReply<MC : MyDHTConf> {
  NoRep,
  Api(ApiReply<MC>),
}


