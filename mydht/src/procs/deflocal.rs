//! Default proxy local service implementation
use utils::{
  Ref,
  ToRef,
  SRef,
  SToRef,
};
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
use keyval::KeyVal;
use peer::Peer;
use std::marker::PhantomData;

pub struct GlobalCommand<MC : MyDHTConf>(pub Option<MC::PeerRef>, pub MC::GlobalServiceCommand);
//pub struct GlobalCommand<PR,GSC>(pub Option<PR>, pub GSC);

pub struct GlobalCommandSend<MC : MyDHTConf>(pub Option<<MC::PeerRef as Ref<MC::Peer>>::Send>, <MC::GlobalServiceCommand as SRef>::Send)
  where MC::GlobalServiceCommand : SRef;

impl<MC : MyDHTConf> SRef for GlobalCommand<MC> 
  where MC::GlobalServiceCommand : SRef {
  type Send = GlobalCommandSend<MC>;//: SToRef<Self>;
  fn get_sendable(&self) -> Self::Send {
    let GlobalCommand(ref opr,ref gsc) = *self;
    GlobalCommandSend(opr.as_ref().map(|pr|pr.get_sendable()), gsc.get_sendable())
  }
}

impl<MC : MyDHTConf> SToRef<GlobalCommand<MC>> for GlobalCommandSend<MC> 
  where MC::GlobalServiceCommand : SRef {
  fn to_ref(self) -> GlobalCommand<MC> {
    let GlobalCommandSend(opr,gsc) = self;
    GlobalCommand(opr.map(|pr|pr.to_ref()), gsc.to_ref())
  }
}


pub enum GlobalReply<MC : MyDHTConf> {
  /// forward command to list of peers or/and to nb peers from route
  Forward(Option<Vec<MC::PeerRef>>,Option<Vec<(<MC::Peer as KeyVal>::Key,<MC::Peer as Peer>::Address)>>,usize,MC::GlobalServiceCommand),
  /// reply to api
  Api(MC::GlobalServiceReply),
  /// no rep
  NoRep,
  Mult(Vec<GlobalReply<MC>>),
}
/*pub enum GlobalReply<P : KeyVal,PR,GSC,GSR> {
  /// forward command to list of peers or/and to nb peers from route
  Forward(Option<Vec<PR>>,Option<Vec<(<P as KeyVal>::Key,<P as Peer>::Address)>>,usize,GSC),
  /// reply to api
  Api(GSR),
  /// no rep
  NoRep,
  Mult(Vec<GlobalReply<P,PR,GSC,GSR>>),
}*/


impl<MC : MyDHTConf> Clone for GlobalCommand<MC> where MC::GlobalServiceCommand : Clone {
  fn clone(&self) -> Self {
    let &GlobalCommand(ref oref,ref lsc) = self;
    GlobalCommand(oref.clone(),lsc.clone())
  }
}
impl<MC : MyDHTConf> Clone for GlobalReply<MC> where MC::GlobalServiceReply : Clone {
  fn clone(&self) -> Self {
    match *self {
      GlobalReply::Forward(ref odests,ref okadests, nb_for, ref gsc) => GlobalReply::Forward(odests.clone(),okadests.clone(),nb_for,gsc.clone()),
      GlobalReply::Api(ref gsr) => GlobalReply::Api(gsr.clone()),
      GlobalReply::NoRep => GlobalReply::NoRep,
      GlobalReply::Mult(ref grs) => GlobalReply::Mult(grs.clone()),
    }
  }
}

/*
impl<MC : MyDHTConf> GetOrigin<MC> for GlobalCommand<MC> {
  fn get_origin(&self) -> Option<&MC::PeerRef> {
    self.0.as_ref()
  }
}*/
/*
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
*/
//pub struct LocalCommand<MC : MyDHTConf>(pub MC::ProtoMsg);

pub enum LocalReply<MC : MyDHTConf> {
  /// transfer to global service
  Global(GlobalCommand<MC>),
  /// same capability as read dest, awkward as it targets internal call
  Read(ReadReply<MC>),
  /// reply to api
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
/*
impl<MC : MyDHTConf> ApiRepliable for GlobalReply<MC> {
  #[inline]
  fn get_api_reply(&self) -> Option<ApiQueryId> {
    self.0.get_api_reply()
  }
}*/


impl<MC : MyDHTConf> Service for DefLocalService<MC> 
  where 
   MC::GlobalServiceChannelIn: SpawnChannel<GlobalCommand<MC>>,
   MC::GlobalServiceSpawn: Spawner<MC::GlobalService, GlobalDest<MC>, <MC::GlobalServiceChannelIn as SpawnChannel<GlobalCommand<MC>>>::Recv>
{
  type CommandIn = MC::GlobalServiceCommand;
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

pub struct GlobalDest<MC : MyDHTConf> {
  pub mainloop : MioSend<<MC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MC>>>::Send>,
  pub api : Option<ApiHandleSend<MC>>,
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
/*
  Forward(Option<Vec<MC::PeerRef>>,usize,MC::GlobalServiceCommand),
  /// reply to api
  Api(MC::GlobalServiceReply),
  /// no rep
  NoRep,
}*/

impl<MC : MyDHTConf> SpawnSend<GlobalReply<MC>> for GlobalDest<MC> {
  const CAN_SEND : bool = true;
  fn send(&mut self, r : GlobalReply<MC>) -> Result<()> {
    match r {
      GlobalReply::Mult(cmds) => {
        for cmd in cmds.into_iter() {
          self.send(cmd)?;
        }
      },
      GlobalReply::Api(c) => {
        if c.get_api_reply().is_some() {
          let cml =  match self.api {
            Some(ref mut api_weak) => {
              if api_weak.1.is_finished() {
                Some(c)
              } else {
                api_weak.send(ApiCommand::GlobalServiceReply(c))?;
                None
              }
            },
            None => {
              Some(c)
            },
          };
          if let Some(c) = cml {
            self.api = None;
            self.mainloop.send(MainLoopCommand::ProxyApiGlobalReply(c))?;
          }
        }
      },
      GlobalReply::Forward(opr,okad,nb_for,gsc) => {
        self.mainloop.send(MainLoopCommand::ForwardServiceGlobal(opr,okad,nb_for,gsc))?;
      },
      GlobalReply::NoRep => (),
    }
    Ok(())
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


