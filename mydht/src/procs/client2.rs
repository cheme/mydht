//! Client service
use std::borrow::Borrow;
use peer::Peer;
use peer::{
  PeerMgmtMeths,
  PeerPriority,
};
use keyval::{
  KeyVal,
  SettableAttachment,
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
  ToRef,
  SRef,
  SToRef,
  shad_write_header,
  shad_write_end,
  send_msg,
  send_att,
};
use super::{
  MyDHTConf,
  PeerRefSend,
  ShadowAuthType,
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
  from : PeerRefSend<MC>,
  with : Option<PeerRefSend<MC>>,
  peermgmt : MC::PeerMgmtMeths,
  token : usize,
}

impl<MC : MyDHTConf> WriteService<MC> {
  pub fn new(token : usize, ws : <MC::Transport as Transport>::WriteStream, me : PeerRefSend<MC>, with : Option<PeerRefSend<MC>>, enc : MC::MsgEnc, peermgmt : MC::PeerMgmtMeths) -> Self {
    WriteService {
      stream : ws,
      is_auth : false,
      enc : enc,
      from : me,
      with : with,
      peermgmt : peermgmt,
      token : token,
    }
  }
}


pub fn get_shad_auth<MDC : MyDHTConf>(from : &PeerRefSend<MDC>,with : &Option<PeerRefSend<MDC>>) -> <MDC::Peer as Peer>::ShadowWAuth {
  match MDC::AUTH_MODE {
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

impl<MDC : MyDHTConf> Service for WriteService<MDC> {
  type CommandIn = WriteCommand<MDC>;
  type CommandOut = WriteReply<MDC>;

  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, async_yield : &mut S) -> Result<Self::CommandOut> {
    let mut stream = WriteYield(&mut self.stream, async_yield);
    match req {
      WriteCommand::Write => {
        // for initial testing only TODO replace by deser
        let buf = &[1,2,3,4];
//        let mut w = WriteYield(&mut self.stream, async_yield);
        stream.write_all(buf).unwrap(); // unwrap for testing only (thread without error catching
      },
      WriteCommand::Pong(rp,chal,read_token,option_chal2) => {
        // update dest
        self.with = Some(rp.get_sendable());
        let sig = self.peermgmt.signmsg(self.from.borrow(), &chal[..]);
        let pmess : ProtoMessage<MDC::Peer,MDC::KeyVal> = ProtoMessage::PONG(self.from.borrow(),chal,sig,option_chal2);
        // once shadower
        let mut shad = get_shad_auth::<MDC>(&self.from,&self.with);
        shad_write_header(&mut shad, &mut stream)?;

        send_msg(&pmess, &mut stream, &self.enc, &mut shad)?;
        if let Some(ref att) = self.from.borrow().get_attachment() {
          send_att(att, &mut stream, &self.enc, &mut shad)?;
        }

        shad_write_end(&mut shad, &mut stream)?;
        stream.flush()?;
      },
      WriteCommand::Ping(chal) => {
 //       let chal = self.peermgmt.challenge(self.from.borrow());
        let sign = self.peermgmt.signmsg(self.from.borrow(), &chal);
        // we do not wait for a result
        let pmess : ProtoMessage<MDC::Peer,MDC::KeyVal> = ProtoMessage::PING(self.from.borrow(), chal.clone(), sign);
        let mut shad = get_shad_auth::<MDC>(&self.from,&self.with);
        shad_write_header(&mut shad, &mut stream)?;

        send_msg(&pmess, &mut stream, &self.enc, &mut shad)?;
        if let Some(ref att) = self.from.borrow().get_attachment() {
          send_att(att, &mut stream, &self.enc, &mut shad)?;
        }

        shad_write_end(&mut shad, &mut stream)?;
        stream.flush()?;
//        return Ok(WriteReply::MainLoop(MainLoopCommand::NewChallenge(self.token,chal)));

      },
    }
    // default to no rep
    Ok(WriteReply::NoRep)
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
}

pub enum WriteCommandSend<MC : MyDHTConf> {
  Write,
  /// Vec<u8> being chalenge store in mainloop process
  Ping(Vec<u8>),
  /// pong a peer with challenge and read token
  Pong(<MC::PeerRef as Ref<MC::Peer>>::Send, Vec<u8>, usize, Option<Vec<u8>>),
}

impl<MC : MyDHTConf> Clone for WriteCommand<MC> {
  fn clone(&self) -> Self {
    match *self {
      WriteCommand::Write => WriteCommand::Write,
      WriteCommand::Ping(ref chal) => WriteCommand::Ping(chal.clone()),
      WriteCommand::Pong(ref pr,ref v,s,ref v2) => WriteCommand::Pong(pr.clone(),v.clone(),s,v2.clone()),
    }
  }
}
impl<MC : MyDHTConf> SRef for WriteCommand<MC> {
  type Send = WriteCommandSend<MC>;
  fn get_sendable(&self) -> Self::Send {
    match *self {
      WriteCommand::Write => WriteCommandSend::Write,
      WriteCommand::Ping(ref chal) => WriteCommandSend::Ping(chal.clone()),
      WriteCommand::Pong(ref pr,ref v,s,ref v2) => WriteCommandSend::Pong(pr.get_sendable(),v.clone(),s,v2.clone()),
    }
  }
}
impl<MC : MyDHTConf> SToRef<WriteCommand<MC>> for WriteCommandSend<MC> {
  fn to_ref(self) -> WriteCommand<MC> {
    match self {
      WriteCommandSend::Write => WriteCommand::Write,
      WriteCommandSend::Ping(chal) => WriteCommand::Ping(chal),
      WriteCommandSend::Pong(pr,v,s,v2) => WriteCommand::Pong(pr.to_ref(),v,s,v2),
    }
  }
}


#[derive(Clone)]
pub enum WriteReply<MDC : MyDHTConf> {
  NoRep,
  Api(ApiReply<MDC>),
}


