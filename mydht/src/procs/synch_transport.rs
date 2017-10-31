//! service to manage synch transport : 
//!  - listen for incomming connection
//!  - try connect in background
use mydhtresult::Result;
use service::{
  Service,
  SpawnSend,
  SpawnerYield,
};
use procs::{
  MyDHTConf,
};
use std::sync::{
  Arc,
};
use transport::Transport;
use super::mainloop::{
  MainLoopCommand,
};
use super::{
  MainLoopSendIn,
};
use utils::{
  Proto,
};

//------------sync listenner

#[derive(Clone)]
pub struct SynchConnListenerCommandIn;

impl Proto for SynchConnListenerCommandIn {
  #[inline]
  fn get_new(&self) -> Self {
    self.clone()
  }
}
pub enum SynchConnListenerCommandOut<T : Transport> {
  Skip,
  Connected(<T as Transport>::ReadStream, Option<<T as Transport>::WriteStream>),
}
pub struct SynchConnListenerCommandDest<MC : MyDHTConf>(pub MainLoopSendIn<MC>);
pub struct SynchConnListener<T : Transport> (pub Arc<T>);
impl<T : Transport> Service for SynchConnListener<T> {
  type CommandIn = SynchConnListenerCommandIn;
  type CommandOut = SynchConnListenerCommandOut<T>;

  fn call<S : SpawnerYield>(&mut self, _req: Self::CommandIn, _async_yield : &mut S) -> Result<Self::CommandOut> {
    match self.0.accept() {
      Ok((rs,ows)) => {
        return Ok(SynchConnListenerCommandOut::Connected(rs,ows));
      },
      // ignore error
      Err(e) => error!("Transport accept error : {}",e),
    }
    Ok(SynchConnListenerCommandOut::Skip)
  }
}

impl<MC : MyDHTConf> SpawnSend<SynchConnListenerCommandOut<MC::Transport>> for SynchConnListenerCommandDest<MC> {
  const CAN_SEND : bool = true;
  fn send(&mut self, r : SynchConnListenerCommandOut<MC::Transport>) -> Result<()> {
    match r {
      SynchConnListenerCommandOut::Connected(rs,ows) => {
        self.0.send(MainLoopCommand::ConnectedR(rs,ows))?;
      },
      SynchConnListenerCommandOut::Skip => (),
    };
    Ok(())
  }
}


//------------connect service

#[derive(Clone)]
pub struct SynchConnectCommandIn<T : Transport>(pub usize,pub <T as Transport>::Address);
pub enum SynchConnectCommandOut<T : Transport> {
  /// contain slab index to clean
  Failure(usize),
  /// contain slab index and streams
  Connected(usize, <T as Transport>::WriteStream, Option<<T as Transport>::ReadStream>),
}
pub struct SynchConnectDest<MC : MyDHTConf>(pub MainLoopSendIn<MC>);
pub struct SynchConnect<T : Transport> (pub Arc<T>);

impl<T : Transport> Service for SynchConnect<T> {
  type CommandIn = SynchConnectCommandIn<T>;
  type CommandOut = SynchConnectCommandOut<T>;

  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, _async_yield : &mut S) -> Result<Self::CommandOut> {
    let SynchConnectCommandIn(slab_ix,add) = req;
    Ok(match self.0.connectwith(&add) {
      Ok((ws,ors)) => SynchConnectCommandOut::Connected(slab_ix,ws,ors),
      Err(e) => {
        debug!("Could not connect: {}",e);
        SynchConnectCommandOut::Failure(slab_ix)
      },
    })
  }
}

impl<MC : MyDHTConf> SpawnSend<SynchConnectCommandOut<MC::Transport>> for SynchConnectDest<MC> {
  const CAN_SEND : bool = true;
  fn send(&mut self, r : SynchConnectCommandOut<MC::Transport>) -> Result<()> {
    match r {
      SynchConnectCommandOut::Connected(slx, ws,ors) => {
        self.0.send(MainLoopCommand::ConnectedW(slx,ws,ors))?;
      },
      SynchConnectCommandOut::Failure(slx) => {
        self.0.send(MainLoopCommand::FailConnect(slx))?;
      }
    };
    Ok(())
  }
}


