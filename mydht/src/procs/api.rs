//! Api related command and utility
//!
//! Depending on crate feature api-direct-sender two kind of api send : direct or through mainloop
//! direct require internal sender to have same constraint as the mainloop sender.
//! Through mainloop only require the mainloop send constraint : on a single thread service
//! (service and mainloop on different thread) we could use Rc sender and do not require Arc
//! Sender, but all query goes through the mainloop.
//!
//! Direct sender skip mainloop usage, non direct sender on the other side allow to keep non Send
//! sender for instance if service spawn is local to mainloop and use a non Sync Rc using a direct
//! sender would need to switch to Arc usage.
use mydhtresult::{
  Result,
  Error,
};

use mydht_base::kvcache::{
  KVCache
};
use super::mainloop::MainLoopCommand;
use super::client2::WriteCommand;
use super::{
  MyDHTConf,
};
use transport::{
  Transport,
};
use super::deflocal::{
  GlobalCommand,
  GlobalCommandSend,
  GlobalReply,
};


use service::{
  HandleSend,
  Service,
  Spawner,
  Blocker,
  NoChannel,
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
use utils::{
  OneResult,
  ret_one_result,
};
use std::time::{
  Duration,
  Instant,
};
use std::marker::PhantomData;

/// local apiQuery
pub struct ApiQuery(ApiQueryId);

/// TODO add a query cache over cond var
/// implemetation of an api service using stored condvar for returning results
/// TODO refactor query cache to use it or replace it by Slab!!
pub struct Api<MC : MyDHTConf,QC : KVCache<ApiQueryId,(MC::ApiReturn,Instant)>>(pub QC,pub Duration,pub usize,pub PhantomData<MC>);

impl<MC : MyDHTConf,QC : KVCache<ApiQueryId,(MC::ApiReturn,Instant)>> Service for Api<MC,QC> {
  type CommandIn = ApiCommand<MC>;
  type CommandOut = ApiReply<MC>;

  fn call<S : SpawnerYield>(&mut self, req : Self::CommandIn, async_yield : &mut S) -> Result<Self::CommandOut> {
    Ok(match req {
      ApiCommand::Mainloop(mlc) => {
        ApiReply::ProxyMainloop(mlc)
      },
      ApiCommand::Failure(..) => {
        panic!("TODO implement : return error to oneresult");
      },
      ApiCommand::LocalServiceCommand(mut lsc,nb_for,ret) => {
        if lsc.is_api_reply() {
          self.2 += 1;
          let qid = ApiQueryId(self.2);
          lsc.set_api_reply(qid.clone());
          let end = Instant::now() + self.1;
          self.0.add_val_c(qid.clone(),(ret,end));
        }
        ApiReply::ProxyMainloop(MainLoopCommand::ForwardServiceLocal(lsc,nb_for))
      },
      ApiCommand::GlobalServiceCommand(mut gsc,ret) => {
        if gsc.is_api_reply() {
          self.2 += 1;
          let qid = ApiQueryId(self.2);
          gsc.set_api_reply(qid.clone());
          let end = Instant::now() + self.1;
          self.0.add_val_c(qid.clone(),(ret,end));
        }
        ApiReply::ProxyMainloop(MainLoopCommand::ProxyGlobal(gsc))
      },
      ApiCommand::LocalServiceReply(lsr) => {
        if let Some(qid) = lsr.get_api_reply() {
          if let Some(q) = self.0.remove_val_c(&qid) {
            //<MC::ApiService as ApiReturn<MC>>::api_return(q.0,ApiResult::LocalServiceReply(lsr))?;
            q.0.api_return(ApiResult::LocalServiceReply(lsr))?;
          }
        }
        ApiReply::Done
      },
      ApiCommand::GlobalServiceReply(gsr) => {
        if let Some(qid) = gsr.get_api_reply() {
          if let Some(q) = self.0.remove_val_c(&qid) {
//            <MC::ApiService as ApiReturn<MC>>::api_return(q.0,ApiResult::GlobalServiceReply(gsr))?;
            q.0.api_return(ApiResult::GlobalServiceReply(gsr))?;
          }
        }
        ApiReply::Done
      },

    })
  }

}

#[derive(Clone,PartialEq,Eq,Hash)]
pub struct ApiQueryId(pub usize);

pub enum ApiCommand<MC : MyDHTConf> {
  Mainloop(MainLoopCommand<MC>),
  Failure(Option<ApiQueryId>,Error),
  LocalServiceCommand(MC::LocalServiceCommand,usize,MC::ApiReturn),
  GlobalServiceCommand(GlobalCommand<MC>,MC::ApiReturn),
  LocalServiceReply(MC::LocalServiceReply),
  GlobalServiceReply(MC::GlobalServiceReply),
}


/// inner types of api command are not public,
/// so command need to be instantiate through methods
/// This also filters non public method for dest services
impl<MC : MyDHTConf> ApiCommand<MC> {
  pub fn try_connect(ad : <MC::Transport as Transport>::Address) -> ApiCommand<MC> {
    ApiCommand::Mainloop(MainLoopCommand::TryConnect(ad))
  }
  pub fn call_service_reply(mut c : MC::GlobalServiceCommand, ret : MC::ApiReturn) -> ApiCommand<MC> {
    ApiCommand::GlobalServiceCommand(GlobalCommand(None,c),ret)
  }
 
  pub fn call_service(mut c : MC::GlobalServiceCommand) -> ApiCommand<MC> {
    let cmd = MainLoopCommand::ProxyGlobal(GlobalCommand(None,c));
    ApiCommand::Mainloop(cmd)
  }
 
  pub fn call_service_local(mut c : MC::LocalServiceCommand, nb_for : usize) -> ApiCommand<MC> {
    let cmd = MainLoopCommand::ForwardServiceLocal(c,nb_for);
    ApiCommand::Mainloop(cmd)
  }
  pub fn call_service_local_reply(mut c : MC::LocalServiceCommand, nb_for : usize, ret : MC::ApiReturn) -> ApiCommand<MC> {
    ApiCommand::LocalServiceCommand(c,nb_for,ret)
  }

}

pub enum ApiReply<MC : MyDHTConf> {
  /// if no result expected
  Done,
  /// proxy
  ProxyMainloop(MainLoopCommand<MC>),
  LocalServiceReply(MC::LocalServiceReply),
  GlobalServiceReply(MC::GlobalServiceReply),
}

pub enum ApiResult<MC : MyDHTConf> {
  LocalServiceReply(MC::LocalServiceReply),
  GlobalServiceReply(MC::GlobalServiceReply),
  NoResult,
}

impl<MC : MyDHTConf> Clone for ApiResult<MC> 
where MC::LocalServiceReply : Clone,
      MC::GlobalServiceReply : Clone,
{
  fn clone(&self) -> Self {
    match *self {
      ApiResult::LocalServiceReply(ref lsr) => ApiResult::LocalServiceReply(lsr.clone()),
      ApiResult::GlobalServiceReply(ref lsr) => ApiResult::GlobalServiceReply(lsr.clone()),
      ApiResult::NoResult => ApiResult::NoResult,
    }
  }
}

pub struct ApiSendIn<MC : MyDHTConf> {
//    pub api_direct : Option<ApiHandleSend<MC>>,
    pub main_loop : MioSend<<MC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MC>>>::Send>,
}

pub struct ApiDest<MC : MyDHTConf> {
  // TODO direct send
    pub main_loop : MioSend<<MC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MC>>>::Send>,
}

impl<MC : MyDHTConf> SpawnSend<ApiReply<MC>> for ApiDest<MC> {
  const CAN_SEND : bool = true;
  fn send(&mut self, c : ApiReply<MC>) -> Result<()> {
    match c {
      ApiReply::Done => (),
//      ApiReply::Failure(wc) => self.main_loop.send(ic)?,
      ApiReply::ProxyMainloop(cmd) => self.main_loop.send(cmd)?,
      ApiReply::LocalServiceReply(..) => unreachable!(),
      ApiReply::GlobalServiceReply(..) => unreachable!(),
    };
    Ok(())
  }

}


pub trait ApiReturn<MC : MyDHTConf> {
  fn api_return(&self, ApiResult<MC>) -> Result<()>;
}

/// contains a Vec of result, and nb_result and nb_error, notify as complete when nb_result receive
/// or when nb_error or result receive.
impl<MC : MyDHTConf> ApiReturn<MC> for OneResult<(Vec<ApiResult<MC>>,usize,usize)>
where 
  MC::LocalServiceReply : Send,
  MC::GlobalServiceReply : Send,
{
  fn api_return(&self, rep : ApiResult<MC>) -> Result<()> {
    let no_res = if let ApiResult::NoResult = rep {true} else {false};
    if match self.0.lock() {
      Ok(mut res) => {
        if no_res {
          (res.0).2 -= 1;
        } else {
          (res.0).0.push(rep);
          (res.0).1 -= 1;
          (res.0).2 -= 1;
        }
        if (res.0).1 == 0 || (res.0).2 == 0 {
          res.1 = true;
          // avoid overflow and notify on each next result or error (allow stream)
          (res.0).1 = 1;
          (res.0).2 = 1;
          true
        } else {
          false
        }
      },
      Err(m) => {
        error!("poisoned mutex for api result : {:?}", m);
        false
      },
    } {
      self.1.notify_all();
    }


    // TODO refact oneresult
    Ok(())
  }
}

pub trait ApiQueriable {
  fn is_api_reply(&self) -> bool;
  fn set_api_reply(&mut self, ApiQueryId);
}
pub trait ApiRepliable {
  fn get_api_reply(&self) -> Option<ApiQueryId>;
}


impl<MC : MyDHTConf> SpawnSend<ApiCommand<MC>> for ApiSendIn<MC> {
  const CAN_SEND : bool = <MC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MC>>>::Send::CAN_SEND;
  fn send(&mut self, c : ApiCommand<MC>) -> Result<()> {
    match c {
      ApiCommand::Mainloop(ic) => self.main_loop.send(ic)?,
      ApiCommand::Failure(_,_) => unreachable!(),
      ApiCommand::LocalServiceCommand(cmd,nb_f,ret) => self.main_loop.send(MainLoopCommand::ForwardServiceApi(cmd,nb_f,ret))?,
//  ForwardServiceLocal(MC::LocalServiceCommand,MC::PeerRef),
      ApiCommand::GlobalServiceCommand(cmd,ret) => self.main_loop.send(MainLoopCommand::GlobalApi(cmd,ret))?,
      ApiCommand::LocalServiceReply(rep) => {
        let oqid = rep.get_api_reply();
        panic!("TODO get from cache an unlock cond var")
      },
      ApiCommand::GlobalServiceReply(rep) => {
        let oqid = rep.get_api_reply();
        panic!("TODO get from cache an unlock cond var")
      },
 
    };
    Ok(())
  }
}

