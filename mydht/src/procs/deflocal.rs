//! Default proxy local service implementation

use mydhtresult::{
  Result,
};
use super::mainloop::{
  MainLoopCommand,
};
 
use service::{
  Service,
  Spawner,
  SpawnUnyield,
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
use super::server2::{
  ReadDest,
};
use super::{
  MyDHTConf,
  GetOrigin,
  GlobalHandleSend,
};
use super::server2::{
  ReadReply,
};
use std::marker::PhantomData;

pub struct GlobalCommand<MDC : MyDHTConf>(pub Option<MDC::PeerRef>, pub MDC::LocalServiceCommand);

impl<MDC : MyDHTConf> GetOrigin<MDC> for GlobalCommand<MDC> {
  fn get_origin(&self) -> Option<&MDC::PeerRef> {
    self.0.as_ref()
  }
}

pub struct GlobalReply<MDC : MyDHTConf>(pub PhantomData<MDC>);

//pub struct LocalCommand<MDC : MyDHTConf>(pub MDC::ProtoMsg);

pub enum LocalReply<MDC : MyDHTConf> {
  Global(MDC::GlobalServiceCommand),
  Read(ReadReply<MDC>), 
}

pub struct DefLocalService<MDC : MyDHTConf> {
  pub from : MDC::PeerRef,
  /// optional as in non auth app no peer ref, otherwhise allways a value which will be use to
  /// build GlobalCommand in adapter
  pub with : Option<MDC::PeerRef>,
}


impl<MDC : MyDHTConf<GlobalServiceCommand = GlobalCommand<MDC>>> Service for DefLocalService<MDC> 
  where 
   MDC::GlobalServiceChannelIn: SpawnChannel<GlobalCommand<MDC>>,
   MDC::GlobalServiceSpawn: Spawner<MDC::GlobalService, MDC::GlobalDest, <MDC::GlobalServiceChannelIn as SpawnChannel<GlobalCommand<MDC>>>::Recv>
{
  type CommandIn = MDC::LocalServiceCommand;
  type CommandOut = LocalReply<MDC>;
  #[inline]
  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, _ : &mut S) -> Result<Self::CommandOut> {
    Ok(LocalReply::Global(GlobalCommand(self.with.clone(),req)))
  }
}

pub struct LocalDest<MDC : MyDHTConf> {
  pub global : Option<GlobalHandleSend<MDC>>,
  pub read : ReadDest<MDC>,
}

impl<MDC : MyDHTConf> Clone for LocalDest<MDC> {
    fn clone(&self) -> Self {
      LocalDest{
        read : self.read.clone(),
        global : self.global.clone(),
      }
    }
}
impl<MDC : MyDHTConf> SpawnSend<LocalReply<MDC>> for LocalDest<MDC> {
  const CAN_SEND : bool = true;
  fn send(&mut self, r : LocalReply<MDC>) -> Result<()> {
    match r {
      LocalReply::Read(mlc) => {
        self.read.send(mlc)?;
      },
      LocalReply::Global(mlc) => {
        let cml = match self.global {
          Some(ref mut w) => {
            if w.1.is_finished() {
              Some(mlc)
            } else {
              w.send(mlc)?;
              None
            }
          },
          None => {
            Some(mlc)
          },
        };
        if let Some(c) = cml {
          self.global = None;
          self.read.send(ReadReply::MainLoop(MainLoopCommand::ProxyGlobal(c)))?;
        }
      },
    }
    Ok(())
  }
}


