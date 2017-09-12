//! Server service
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
pub struct ReadService<MC : MyDHTConf>(pub <MC::Transport as Transport>::ReadStream);

impl<MDC : MyDHTConf> Service for ReadService<MDC> {
  type CommandIn = ReadServiceCommand;
  type CommandOut = ();

  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, async_yield : &mut S) -> Result<Self::CommandOut> {
    match req {
      ReadServiceCommand::Run => {
        // for initial testing only TODO replace by deser
        let mut buf = vec![0;4];
        let mut r = ReadYield(&mut self.0, async_yield);
        r.read_exact(&mut buf).unwrap(); // unwrap for testring only
//        panic!("{:?}",&buf[..]);
        println!("{:?}",&buf[..]);
        assert!(&[1,2,3,4] == &buf[..]);
        buf[0]=9;
      },
    }
    Ok(())
  }
}
/// command for readservice
#[derive(Clone)]
pub enum ReadServiceCommand {
  Run,
}


