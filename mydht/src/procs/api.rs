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
};

use super::mainloop::MainLoopCommand;
use super::client2::WriteCommand;
use super::{
  MyDHTConf,
};
use transport::{
  Transport,
};
use service::{
  SpawnChannel,
  SpawnSend,
  MioSend,
};


/// local apiQuery
pub struct ApiQuery(ApiQueryId);

#[derive(Clone)]
pub struct ApiQueryId(usize);

pub enum ApiCommand<MC : MyDHTConf> {
  Mainloop(MainLoopCommand<MC>),
}

/// inner types of api command are not public,
/// so command need to be instantiate through methods
/// This also filters non public method for dest services
impl<MC : MyDHTConf> ApiCommand<MC> {
  pub fn try_connect(ad : <MC::Transport as Transport>::Address) -> ApiCommand<MC> {
    ApiCommand::Mainloop(MainLoopCommand::TryConnect(ad))
  }
  pub fn call_service(c : MC::LocalServiceCommand) -> ApiCommand<MC> {
    ApiCommand::Mainloop(MainLoopCommand::ForwardService(c))
  }
  pub fn local_service(c : MC::GlobalServiceCommand) -> ApiCommand<MC> {
    ApiCommand::Mainloop(MainLoopCommand::ProxyGlobal(c))
  }
}

#[derive(Clone)]
pub enum ApiReply<MDC : MyDHTConf> {
  /// if no result expected
  Done,
  /// service failure (could be from spawner)
  Failure(WriteCommand<MDC>),
}

#[cfg(feature="api-direct-sender")]
pub struct ApiSendIn<MC : MyDHTConf> {
    pub main_loop : MioSend<<MC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MC>>>::Send>,
}
#[cfg(not(feature="api-direct-sender"))]
pub struct ApiSendIn<MC : MyDHTConf> {
    pub main_loop : MioSend<<MC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MC>>>::Send>, 
}

impl<MC : MyDHTConf> SpawnSend<ApiCommand<MC>> for ApiSendIn<MC> {
  const CAN_SEND : bool = <MC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MC>>>::Send::CAN_SEND;
  fn send(&mut self, c : ApiCommand<MC>) -> Result<()> {
    match c {
      ApiCommand::Mainloop(ic) => self.main_loop.send(ic)?,
    };
    Ok(())
  }
}

