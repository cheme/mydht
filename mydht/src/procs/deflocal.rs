//! Default proxy local service implementation

use mydhtresult::{
  Result,
};
use super::mainloop::{
  MainLoopCommand,
};
use super::api::{
  ApiCommand,
  ApiQueryId,
  ApiQueriable,
  ApiRepliable,
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
  ApiHandleSend,
  OptInto,
};
use super::server2::{
  ReadReply,
};
use std::marker::PhantomData;

pub struct GlobalCommand<MC : MyDHTConf>(pub Option<MC::PeerRef>, pub MC::LocalServiceCommand);
pub struct GlobalReply<MC : MyDHTConf>(pub MC::LocalServiceReply);

impl<MC : MyDHTConf> GetOrigin<MC> for GlobalCommand<MC> {
  fn get_origin(&self) -> Option<&MC::PeerRef> {
    self.0.as_ref()
  }
}
impl<MC : MyDHTConf> OptInto<MC::ProtoMsg> for GlobalReply<MC> {
  #[inline]
  fn can_into(&self) -> bool {
    self.0.can_into()
  }
  #[inline]
  fn opt_into(self) -> Option<MC::ProtoMsg> {
    self.0.opt_into()
  }
}

//pub struct LocalCommand<MC : MyDHTConf>(pub MC::ProtoMsg);

pub enum LocalReply<MC : MyDHTConf> {
  Global(MC::GlobalServiceCommand),
  Read(ReadReply<MC>), 
  Api(MC::LocalServiceReply),
}

pub struct DefLocalService<MC : MyDHTConf> {
  pub from : MC::PeerRef,
  /// optional as in non auth app no peer ref, otherwhise allways a value which will be use to
  /// build GlobalCommand in adapter
  pub with : Option<MC::PeerRef>,
}

impl<MC : MyDHTConf> ApiQueriable for GlobalCommand<MC> {
  #[inline]
  fn is_api_reply(&self) -> bool {
    self.1.is_api_reply()
  }
  #[inline]
  fn set_api_reply(&mut self, aid : ApiQueryId) {
    self.1.set_api_reply(aid)
  }
}

impl<MC : MyDHTConf> ApiRepliable for GlobalReply<MC> {
  #[inline]
  fn get_api_reply(&self) -> Option<ApiQueryId> {
    self.0.get_api_reply()
  }
}


impl<MC : MyDHTConf<GlobalServiceCommand = GlobalCommand<MC>>> Service for DefLocalService<MC> 
  where 
   MC::GlobalServiceChannelIn: SpawnChannel<GlobalCommand<MC>>,
   MC::GlobalServiceSpawn: Spawner<MC::GlobalService, MC::GlobalDest, <MC::GlobalServiceChannelIn as SpawnChannel<GlobalCommand<MC>>>::Recv>
{
  type CommandIn = MC::LocalServiceCommand;
  type CommandOut = LocalReply<MC>;
  #[inline]
  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, _ : &mut S) -> Result<Self::CommandOut> {
    Ok(LocalReply::Global(GlobalCommand(self.with.clone(),req)))
  }
}

pub struct LocalDest<MC : MyDHTConf> {
  pub global : Option<GlobalHandleSend<MC>>,
  pub api : Option<ApiHandleSend<MC>>,
  pub read : ReadDest<MC>,
}

impl<MC : MyDHTConf> Clone for LocalDest<MC> {
    fn clone(&self) -> Self {
      LocalDest{
        read : self.read.clone(),
        api : self.api.clone(),
        global : self.global.clone(),
      }
    }
}
impl<MC : MyDHTConf> SpawnSend<LocalReply<MC>> for LocalDest<MC> {
  const CAN_SEND : bool = true;
  fn send(&mut self, r : LocalReply<MC>) -> Result<()> {
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
      LocalReply::Api(c) => {
        let cml =  match self.api {
          Some(ref mut api_weak) => {
            if api_weak.1.is_finished() {
              Some(c)
            } else {
              api_weak.send(ApiCommand::LocalServiceReply(c))?;
              None
            }
          },
          None => {
            Some(c)
          },
        };
        if let Some(c) = cml {
          self.api = None;
          self.read.send(ReadReply::MainLoop(MainLoopCommand::ProxyApiLocalReply(c)))?;
        }
      },
    }
    Ok(())
  }
}


