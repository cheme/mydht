//! Client service
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
};
use super::{
  MyDHTConf,
  PeerRefSend,
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
  from : PeerRefSend<MC>,
  with : Option<PeerRefSend<MC>>,
}

impl<MC : MyDHTConf> WriteService<MC> {
  pub fn new( ws : <MC::Transport as Transport>::WriteStream, me : PeerRefSend<MC>, with : Option<PeerRefSend<MC>>) -> Self {
    WriteService {
      stream : ws,
      is_auth : false,
      from : me,
      with : with,
    }
  }
}

impl<MDC : MyDHTConf> Service for WriteService<MDC> {
  type CommandIn = WriteCommand<MDC>;
  type CommandOut = WriteReply<MDC>;

  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, async_yield : &mut S) -> Result<Self::CommandOut> {
    match req {
      WriteCommand::Write => {
        // for initial testing only TODO replace by deser
        let buf = &[1,2,3,4];
        let mut w = WriteYield(&mut self.stream, async_yield);
        w.write_all(buf).unwrap(); // unwrap for testing only (thread without error catching
      },
      WriteCommand::Pong(rp,chal,read_token) => panic!("TODO"),
    }
    Ok(WriteReply::Api(ApiReply::Done))
  }
}
/// command for readservice
pub enum WriteCommand<MC : MyDHTConf> {
  Write,
  /// pong a peer with challenge and read token
  Pong(MC::PeerRef, Vec<u8>, usize),
}

pub enum WriteCommandSend<MC : MyDHTConf> {
  Write,
  /// pong a peer with challenge and read token
  Pong(<MC::PeerRef as Ref<MC::Peer>>::Send, Vec<u8>, usize),
}

impl<MC : MyDHTConf> Clone for WriteCommand<MC> {
  fn clone(&self) -> Self {
    match *self {
      WriteCommand::Write => WriteCommand::Write,
      WriteCommand::Pong(ref pr,ref v,s) => WriteCommand::Pong(pr.clone(),v.clone(),s),
    }
  }
}
impl<MC : MyDHTConf> SRef for WriteCommand<MC> {
  type Send = WriteCommandSend<MC>;
  fn get_sendable(&self) -> Self::Send {
    match *self {
      WriteCommand::Write => WriteCommandSend::Write,
      WriteCommand::Pong(ref pr,ref v,s) => WriteCommandSend::Pong(pr.get_sendable(),v.clone(),s),
    }
  }
}
impl<MC : MyDHTConf> SToRef<WriteCommand<MC>> for WriteCommandSend<MC> {
  fn to_ref(self) -> WriteCommand<MC> {
    match self {
      WriteCommandSend::Write => WriteCommand::Write,
      WriteCommandSend::Pong(pr,v,s) => WriteCommand::Pong(pr.to_ref(),v,s),
    }
  }
}


#[derive(Clone)]
pub enum WriteReply<MDC : MyDHTConf> {
  Api(ApiReply<MDC>),
}


