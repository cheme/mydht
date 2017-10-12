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
//! !!! TODO shutdown
//! !!! TODO adjustment and error counter must be refactor :
//!   - nb error change to nb fail forward or have rules to convert fail forward to nb_error
//!   - adjust on connect fail if a query id (a forward)
//!   - local query no reply : send api with an nb err (curently just none and get_api_qid ->
//!   get_nb_ok and get_nb_ko).
//!    - plus see comment in store prop : double cache query confusion  -> forward
//!    failure should be send to storekv back : it is not (need a generic mechanism which is not
//!    inplace : probably a callback fn in fwd message).
use mydhtresult::{
  Result,
};

use procs::storeprop::{
  KVStoreCommand,
  peer_ping,
};
use mydht_base::kvcache::{
  KVCache
};
use super::mainloop::{
  MainLoopCommand,
};
use super::{
  MyDHTConf,
  MCCommand,
  MCReply,
  FWConf,
  MainLoopSendIn,
};
use transport::{
  Transport,
};
use super::deflocal::{
  GlobalCommand,
};



use service::{
  Service,
  SpawnSend,
  SpawnChannel,
  SpawnerYield,
};
use utils::{
  OneResult,
};
use std::time::{
  Duration,
  Instant,
};
use std::marker::PhantomData;


/// An implementation of api service using a kvcache as storage and with a max duration
/// No clean cache is currently call : TODO merge with QueryCache (same thing)
pub struct Api<MC : MyDHTConf,QC : KVCache<ApiQueryId,(MC::ApiReturn,Instant)>>(pub QC,pub Duration,pub usize,pub PhantomData<MC>);

impl<MC : MyDHTConf,QC : KVCache<ApiQueryId,(MC::ApiReturn,Instant)>> Service for Api<MC,QC> {
  type CommandIn = ApiCommand<MC>;
  type CommandOut = ApiReply<MC>;

  fn call<S : SpawnerYield>(&mut self, req : Self::CommandIn, _async_yield : &mut S) -> Result<Self::CommandOut> {
    Ok(match req {
      ApiCommand::MainLoop(mlc) => {
        ApiReply::ProxyMainloop(mlc)
      },
      ApiCommand::Failure(qid) => {
        let rem = if let Some(q) = self.0.get_val_mut_c(&qid) {
          q.0.api_return(ApiResult::NoResult)?
        } else {
          false
        };
        if rem {
          self.0.remove_val_c(&qid);
        }
        ApiReply::Done
      },
      ApiCommand::Adjust(qid,nb) => {
        println!("nb : !!{}",nb);
        let rem = if let Some(q) = self.0.get_val_mut_c(&qid) {
          let mut r = false;
          for _ in 0..nb {
            r = q.0.api_return(ApiResult::NoResult)?;
          }
          r
        } else {
          false
        };
        if rem {
          self.0.remove_val_c(&qid);
        }
        ApiReply::Done
      }
      ApiCommand::ServiceCommand(mut lsc,nb_for,ret) => {
        if lsc.is_api_reply() {
          self.2 += 1;
          let qid = ApiQueryId(self.2);
          lsc.set_api_reply(qid.clone());
          let end = Instant::now() + self.1;
          self.0.add_val_c(qid.clone(),(ret,end));
        }
        match lsc {
          lsc @ MCCommand::Local(..) => {
            ApiReply::ProxyMainloop(MainLoopCommand::ForwardService(None,None,FWConf{ nb_for : nb_for, discover : false },lsc))
           // self.call_inner_loop(MainLoopCommand::ForwardService(None,None,nb_for,sc), async_yield)?;
          },
          MCCommand::Global(gsc) => {
//            self.call_inner_loop(MainLoopCommand::ProxyGlobal(GlobalCommand(None,sc), async_yield)?;
            ApiReply::ProxyMainloop(MainLoopCommand::ProxyGlobal(GlobalCommand(None,gsc)))
          },
          MCCommand::PeerStore(gsc) => {
            ApiReply::ProxyMainloop(MainLoopCommand::PeerStore(GlobalCommand(None,gsc)))
//            self.call_inner_loop(MainLoopCommand::PeerStore(sc), async_yield)?;
          },
          MCCommand::TryConnect(ad,oaid) => {
            ApiReply::ProxyMainloop(MainLoopCommand::TryConnect(ad,oaid))
//            self.call_inner_loop(MainLoopCommand::PeerStore(sc), async_yield)?;
          },

        }
      },
      ApiCommand::ServiceReply(lsr) => {
        if let Some(qid) = lsr.get_api_reply() {
          let rem = if let Some(q) = self.0.get_val_mut_c(&qid) {
            q.0.api_return(ApiResult::ServiceReply(lsr))?
          } else {
            false
          };
          if rem {
            self.0.remove_val_c(&qid);
          }
        }
        ApiReply::Done
      },
    })
  }

}

#[derive(Clone,PartialEq,Eq,Hash,Debug)]
pub struct ApiQueryId(pub usize);

pub enum ApiCommand<MC : MyDHTConf> {
  MainLoop(MainLoopCommand<MC>),
  Failure(ApiQueryId),
  /// usize is nb local service call if local service TODO consider removing local service from
  /// api??
  ServiceCommand(MCCommand<MC>,usize,MC::ApiReturn),
  //GlobalServiceCommand(GlobalCommand<MC::PeerRef,MC::GlobalServiceCommand>,MC::ApiReturn),
  ServiceReply(MCReply<MC>),
//  GlobalServiceReply(MC::GlobalServiceReply),
  /// remove some expected result (consider as failure)
  Adjust(ApiQueryId,usize),
}


/// inner types of api command are not public,
/// so command need to be instantiate through methods
/// This also filters non public method for dest services
impl<MC : MyDHTConf> ApiCommand<MC> {
  pub fn try_connect(ad : <MC::Transport as Transport>::Address) -> ApiCommand<MC> {
    ApiCommand::MainLoop(MainLoopCommand::TryConnect(ad,None))
  }
  pub fn try_connect_reply(ad : <MC::Transport as Transport>::Address, ret : MC::ApiReturn) -> ApiCommand<MC> {
    ApiCommand::ServiceCommand(MCCommand::TryConnect(ad,None),0,ret)
  }
  pub fn call_shutdown_reply(_ret : MC::ApiReturn) -> ApiCommand<MC> {
    unimplemented!()
    // TODO change TryConnect MCCommand to generic MDHT enum command
    //ApiCommand::ServiceCommand(MCCommand::Shutdown,0,ret) -> could be MainLoopSubCommand !!!!
  }
 

  pub fn refresh_peer(nb : usize) -> ApiCommand<MC> {
    let kvscom = GlobalCommand(None,KVStoreCommand::Subset(nb, peer_ping));
    ApiCommand::MainLoop(MainLoopCommand::PeerStore(kvscom))
  }
 
  pub fn call_peer_reply(c : KVStoreCommand<MC::Peer,MC::PeerRef,MC::Peer,MC::PeerRef>, ret : MC::ApiReturn) -> ApiCommand<MC> {
    // fw conf to def val, is replaced by kvstore service anyway.
    ApiCommand::ServiceCommand(MCCommand::PeerStore(c),0,ret)
  }

  pub fn call_service_reply(c : MC::GlobalServiceCommand, ret : MC::ApiReturn) -> ApiCommand<MC> {
    ApiCommand::ServiceCommand(MCCommand::Global(c),0,ret)
  }
 
  pub fn call_service(c : MC::GlobalServiceCommand) -> ApiCommand<MC> {
    let cmd = MainLoopCommand::ProxyGlobal(GlobalCommand(None,c));
    ApiCommand::MainLoop(cmd)
  }
 
  pub fn call_service_local(c : MC::LocalServiceCommand, nb_for : usize ) -> ApiCommand<MC> {
    let cmd = MainLoopCommand::ForwardService(None,None,FWConf{ nb_for : nb_for, discover : false },MCCommand::Local(c));
    ApiCommand::MainLoop(cmd)
  }
  pub fn call_service_local_reply(c : MC::LocalServiceCommand, nb_for : usize, ret : MC::ApiReturn) -> ApiCommand<MC> {
    ApiCommand::ServiceCommand(MCCommand::Local(c),nb_for,ret)
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
  ServiceReply(MCReply<MC>),
//  LocalServiceReply(MC::LocalServiceReply),
//  GlobalServiceReply(MC::GlobalServiceReply),
  NoResult,
}

impl<MC : MyDHTConf> Clone for ApiResult<MC> 
where MC::LocalServiceReply : Clone,
      MC::GlobalServiceReply : Clone,
{
  fn clone(&self) -> Self {
    match *self {
//      ApiResult::LocalServiceReply(ref lsr) => ApiResult::LocalServiceReply(lsr.clone()),
      ApiResult::ServiceReply(ref lsr) => ApiResult::ServiceReply(lsr.clone()),
      ApiResult::NoResult => ApiResult::NoResult,
    }
  }
}

/// send input for api : use mainloop TODO option weak handle to api
pub struct ApiSendIn<MC : MyDHTConf> {
//    pub api_direct : Option<ApiHandleSend<MC>>,
    pub main_loop : MainLoopSendIn<MC>,
}

/// api dest to mainloop
pub struct ApiDest<MC : MyDHTConf> {
  // TODO direct send
    pub main_loop : MainLoopSendIn<MC>,
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
  /// if return true, this could/should be drop
  fn api_return(&self, ApiResult<MC>) -> Result<bool>;
}

/// contains a Vec of result, and nb_result and nb_error, notify as complete when nb_result receive
/// or when nb_error or result receive.
impl<MC : MyDHTConf> ApiReturn<MC> for OneResult<(Vec<ApiResult<MC>>,usize,usize)>
where 
  MC::LocalServiceReply : Send,
  MC::GlobalServiceReply : Send,
{
  fn api_return(&self, rep : ApiResult<MC>) -> Result<bool> {
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
      return Ok(true)
    }

    Ok(false)
  }
}

pub trait ApiQueriable {
  fn is_api_reply(&self) -> bool;
  fn set_api_reply(&mut self, ApiQueryId);
  fn get_api_reply(&self) -> Option<ApiQueryId>;
}
/// TODO check if trait is used and remove if not
pub trait ApiRepliable {
  fn get_api_reply(&self) -> Option<ApiQueryId>;
}


impl<MC : MyDHTConf> SpawnSend<ApiCommand<MC>> for ApiSendIn<MC> {
  const CAN_SEND : bool = <MC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MC>>>::Send::CAN_SEND;
  fn send(&mut self, c : ApiCommand<MC>) -> Result<()> {
    match c {
      ApiCommand::MainLoop(ic) => self.main_loop.send(ic)?,
      ApiCommand::Failure(..) => unreachable!(),
      ApiCommand::Adjust(..) => unreachable!(),
      ApiCommand::ServiceCommand(cmd,nb_f,ret) => self.main_loop.send(MainLoopCommand::ForwardApi(cmd,nb_f,ret))?,
//  ForwardServiceLocal(MC::LocalServiceCommand,MC::PeerRef),
//      ApiCommand::ServiceCommand(MCCommand::Global(cmd),ret) => self.main_loop.send(MainLoopCommand::GlobalApi(cmd,ret))?,
      ApiCommand::ServiceReply(..) => {
        unreachable!()
//        let oqid = rep.get_api_reply();
 //       panic!("TODO get from cache an unlock cond var")
      },
/*      ApiCommand::GlobalServiceReply(rep) => {
        unreachable!()
        //let oqid = rep.get_api_reply();
        //panic!("TODO get from cache an unlock cond var")
      },*/
 
    };
    Ok(())
  }
}

