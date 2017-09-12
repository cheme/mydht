//! Client service
use transport::{
  Transport,
  Address,
  Address as TransportAddress,
  SlabEntry,
  SlabEntryState,
  Registerable,
};
use super::{
  MyDHTConf,
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


// TODO put in its own module
pub struct WriteService<MC : MyDHTConf>(pub <MC::Transport as Transport>::WriteStream);

impl<MDC : MyDHTConf> Service for WriteService<MDC> {
  type CommandIn = WriteServiceCommand;
  type CommandOut = WriteServiceReply;

  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, async_yield : &mut S) -> Result<Self::CommandOut> {
    match req {
      WriteServiceCommand::Write => {
        // for initial testing only TODO replace by deser
        let buf = &[1,2,3,4];
        let mut w = WriteYield(&mut self.0, async_yield);
        w.write_all(buf).unwrap(); // unwrap for testing only (thread without error catching
      },
    }
    Ok(WriteServiceReply::Done)
  }
}
/// command for readservice
#[derive(Clone)]
pub enum WriteServiceCommand {
  Write,
}

#[derive(Clone)]
pub enum WriteServiceReply {
  /// if no result expected
  Done,
  /// service failure (could be from spawner)
  Failure(WriteServiceCommand),
}


