//! Default proxy local service implementation
use super::{
  FWConf,
};
use utils::{
  SRef,
  SToRef,
};
use mydhtresult::{
  Result,
};
use super::mainloop::{
  MainLoopCommand,
  MainLoopSubCommand,
};
use super::api::{
  ApiCommand,
  ApiQueryId,
  ApiQueriable,
  ApiRepliable,
};
use service::{
  Service,
  SpawnSend,
  SpawnSendWithHandle,
  SpawnerYield,
};
use super::server2::{
  ReadDest,
};
use super::{
  MyDHTConf,
  ApiHandleSend,
  MCCommand,
  MCReply,
  MainLoopSendIn,
  ApiWeakSend,
  ApiWeakHandle,
};
use super::storeprop::{
  KVStoreCommand,
  KVStoreReply,
};
use super::server2::{
  ReadReply,
};
use keyval::KeyVal;
use peer::Peer;

//pub struct GlobalCommand<MC : MyDHTConf>(pub Option<MC::PeerRef>, pub MC::GlobalServiceCommand);
#[derive(Clone)]
pub struct GlobalCommand<PR,GSC>(pub Option<PR>, pub GSC);

// TODO replace by standard GlobalCommand??
pub struct GlobalCommandSend<PRS,GSCS>(pub Option<PRS>, pub GSCS);
//pub struct GlobalCommandSend<MC : MyDHTConf>(pub Option<<MC::PeerRef as Ref<MC::Peer>>::Send>, <MC::GlobalServiceCommand as SRef>::Send)
//  where MC::GlobalServiceCommand : SRef;

impl<PR : SRef, GSC : SRef> SRef for GlobalCommand<PR,GSC> 
   {
  type Send = GlobalCommandSend<<PR as SRef>::Send, <GSC as SRef>::Send>;//: SToRef<Self>;
  fn get_sendable(self) -> Self::Send {
    let GlobalCommand(opr,gsc) = self;
    GlobalCommandSend(opr.map(|pr|pr.get_sendable()), gsc.get_sendable())
  }
}

impl<PR : SRef, GSC : SRef> SToRef<GlobalCommand<PR,GSC>> for GlobalCommandSend<<PR as SRef>::Send, <GSC as SRef>::Send> {
  fn to_ref(self) -> GlobalCommand<PR,GSC> {
    let GlobalCommandSend(opr,gsc) = self;
    GlobalCommand(opr.map(|pr|pr.to_ref()), gsc.to_ref())
  }
}

/*
pub enum GlobalReply<MC : MyDHTConf> {
  /// forward command to list of peers or/and to nb peers from route
  Forward(Option<Vec<MC::PeerRef>>,Option<Vec<(<MC::Peer as KeyVal>::Key,<MC::Peer as Peer>::Address)>>,usize,MC::GlobalServiceCommand),
  /// reply to api
  Api(MC::GlobalServiceReply),
  /// no rep
  NoRep,
  Mult(Vec<GlobalReply<MC>>),
}*/
pub enum GlobalReply<P : Peer,PR,GSC,GSR> {
  /// forward command to list of peers or/and to nb peers from route
  Forward(Option<Vec<PR>>,Option<Vec<(<P as KeyVal>::Key,<P as Peer>::Address)>>,FWConf,GSC),
  PeerForward(Option<Vec<PR>>,Option<Vec<(<P as KeyVal>::Key,<P as Peer>::Address)>>,FWConf,KVStoreCommand<P,PR,P,PR>),
  /// reply to api
  Api(GSR),
  PeerApi(KVStoreReply<PR>),
  MainLoop(MainLoopSubCommand<P>),
  /// no rep
  NoRep,
  Mult(Vec<GlobalReply<P,PR,GSC,GSR>>),
}

/*
impl<A,B> Clone for GlobalCommand<MC> where MC::GlobalServiceCommand : Clone {
  fn clone(&self) -> Self {
    let &GlobalCommand(ref oref,ref lsc) = self;
    GlobalCommand(oref.clone(),lsc.clone())
  }
}*/
/// TODO derivec clone should be fine here
impl<P : Peer,PR : Clone,GSC : Clone,GSR : Clone> Clone for GlobalReply<P,PR,GSC,GSR> {
  fn clone(&self) -> Self {
    match *self {
      GlobalReply::Forward(ref odests,ref okadests, ref nb_for, ref gsc) => GlobalReply::Forward(odests.clone(),okadests.clone(),nb_for.clone(),gsc.clone()),
      GlobalReply::PeerForward(ref odests,ref okadests,ref nb_for, ref gsc) => GlobalReply::PeerForward(odests.clone(),okadests.clone(),nb_for.clone(),gsc.clone()),
      GlobalReply::Api(ref gsr) => GlobalReply::Api(gsr.clone()),
      GlobalReply::PeerApi(ref gsr) => GlobalReply::PeerApi(gsr.clone()),
      GlobalReply::MainLoop(ref mlsc) => GlobalReply::MainLoop(mlsc.clone()),
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

impl<A,B : ApiQueriable> ApiQueriable for GlobalCommand<A,B> {
  #[inline]
  fn is_api_reply(&self) -> bool {
    self.1.is_api_reply()
  }
  #[inline]
  fn set_api_reply(&mut self, aid : ApiQueryId) {
    self.1.set_api_reply(aid)
  }

  #[inline]
  fn get_api_reply(&self) -> Option<ApiQueryId> {
    self.1.get_api_reply()
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
 // where 
//   MC::GlobalServiceChannelIn: SpawnChannel<GlobalCommand<MC>>,
//   MC::GlobalServiceSpawn: Spawner<MC::GlobalService, GlobalDest<MC>, <MC::GlobalServiceChannelIn as SpawnChannel<GlobalCommand<MC>>>::Recv>
{
  type CommandIn = MC::GlobalServiceCommand;
  type CommandOut = LocalReply<MC>;
  #[inline]
  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, _ : &mut S) -> Result<Self::CommandOut> {
    Ok(LocalReply::Read(ReadReply::Global(GlobalCommand(self.with.clone(),req))))
  }
}

pub struct LocalDest<MC : MyDHTConf> {
  pub api : Option<ApiHandleSend<MC>>,
  pub read : ReadDest<MC>,
}

pub struct GlobalDest<MC : MyDHTConf> {
  pub mainloop : MainLoopSendIn<MC>,
  pub api : Option<ApiHandleSend<MC>>,
}
impl<MC : MyDHTConf> Clone for LocalDest<MC> {
  fn clone(&self) -> Self {
    LocalDest{
      read : self.read.clone(),
      api : self.api.clone(),
    }
  }
}
/*
impl<MC : MyDHTConf> SRef for  GlobalDest<MC> {
  type Send = Self;
  #[inline]
  fn get_sendable(self) -> Self::Send { self }
}
impl<MC : MyDHTConf> SToRef<GlobalDest<MC>> for  GlobalDest<MC> {
  fn to_ref(self) -> GlobalDest<MC> { self }
}
*/
//sref_self_mc!(GlobalDest);



impl<MC : MyDHTConf> SRef for GlobalDest<MC> where
  MainLoopSendIn<MC> : Send,
  ApiWeakSend<MC> : Send,
  ApiWeakHandle<MC> : Send,
  {
  type Send = GlobalDest<MC>;
  #[inline]
  fn get_sendable(self) -> Self::Send {
    self
  }
}

impl<MC : MyDHTConf> SToRef<GlobalDest<MC>> for GlobalDest<MC> where
  MainLoopSendIn<MC> : Send,
  ApiWeakSend<MC> : Send,
  ApiWeakHandle<MC> : Send,
  {
  #[inline]
  fn to_ref(self) -> GlobalDest<MC> {
    self
  }
}





/*
  Forward(Option<Vec<MC::PeerRef>>,usize,MC::GlobalServiceCommand),
  /// reply to api
  Api(MC::GlobalServiceReply),
  /// no rep
  NoRep,
}*/

impl<MC : MyDHTConf> SpawnSend<GlobalReply<MC::Peer,MC::PeerRef,MC::GlobalServiceCommand,MC::GlobalServiceReply>> for GlobalDest<MC> {
  const CAN_SEND : bool = true;
  fn send(&mut self, r : GlobalReply<MC::Peer,MC::PeerRef,MC::GlobalServiceCommand,MC::GlobalServiceReply>) -> Result<()> {
    match r {
      GlobalReply::Mult(cmds) => {
        for cmd in cmds.into_iter() {
          self.send(cmd)?;
        }
      },
      GlobalReply::MainLoop(mlc) => {
        self.mainloop.send(MainLoopCommand::SubCommand(mlc))?;
      },
      GlobalReply::PeerApi(c) => {
        if c.get_api_reply().is_some() {
          let cml =  match self.api {
            Some(ref mut api_weak) => {
              api_weak.send_with_handle(ApiCommand::ServiceReply(MCReply::PeerStore(c)))?.map(|c|
                  if let ApiCommand::ServiceReply(MCReply::PeerStore(c)) = c {c} else {unreachable!()})
            },
            None => {
              Some(c)
            },
          };
          if let Some(c) = cml {
            self.api = None;
            self.mainloop.send(MainLoopCommand::ProxyApiReply(MCReply::PeerStore(c)))?;
          }
        }
      },
      GlobalReply::Api(c) => {
        if c.get_api_reply().is_some() {
          let cml =  match self.api {
            Some(ref mut api_weak) => {
              api_weak.send_with_handle(ApiCommand::ServiceReply(MCReply::Global(c)))?.map(|c|
                  if let ApiCommand::ServiceReply(MCReply::Global(c)) = c {c} else {unreachable!()})
            },
            None => {
              Some(c)
            },
          };
          if let Some(c) = cml {
            self.api = None;
            self.mainloop.send(MainLoopCommand::ProxyApiReply(MCReply::Global(c)))?;
          }
        }
      },
      GlobalReply::PeerForward(opr,okad,nb_for,gsc) => {
        self.mainloop.send(MainLoopCommand::ForwardService(opr,okad,nb_for,MCCommand::PeerStore(gsc)))?;
      },
      GlobalReply::Forward(opr,okad,nb_for,gsc) => {
        self.mainloop.send(MainLoopCommand::ForwardService(opr,okad,nb_for,MCCommand::Global(gsc)))?;
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
      LocalReply::Api(c) => {
        let cml =  match self.api {
          Some(ref mut api_weak) => {
           api_weak.send_with_handle(ApiCommand::ServiceReply(MCReply::Local(c)))?.map(|c|
               if let ApiCommand::ServiceReply(MCReply::Local(c)) = c {c} else {unreachable!()})
          },
          None => {
            Some(c)
          },
        };
        if let Some(c) = cml {
          self.api = None;
          self.read.send(ReadReply::MainLoop(MainLoopCommand::ProxyApiReply(MCReply::Local(c))))?;
        }
      },
    }
    Ok(())
  }
}


