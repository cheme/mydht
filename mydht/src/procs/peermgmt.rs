//! peer management service : store peer as backend (kvstore) and additional functionality (long accept for instance)
//! TODO unimplemented : failure for asynch peer management implementation
use super::{
  MyDHTConf,
  PeerRefSend,
};
use peer::{
  PeerPriority,
};
use utils::{
  SRef,
  SToRef,
};
use super::mainloop::{
  MainLoopCommand,
  MainLoopCommandSend,
};

pub enum PeerMgmtCommand<MC : MyDHTConf> {
  /// peer to check and command to proxy TODO the main loop command must implement a set peerprio
  /// trait !!!!
  Accept(MC::PeerRef, MainLoopCommand<MC>),
  /// new peer with its prio
  NewPrio(MC::PeerRef, PeerPriority),
}

impl<MC : MyDHTConf> Clone for PeerMgmtCommand<MC> 
  where MC::GlobalServiceCommand : Clone,
        MC::LocalServiceCommand : Clone,
        MC::GlobalServiceReply : Clone,
        MC::LocalServiceReply : Clone {
  fn clone(&self) -> Self {
    match *self {
      PeerMgmtCommand::Accept(ref pr,ref com) => 
        PeerMgmtCommand::Accept(pr.clone(),com.clone()),
      PeerMgmtCommand::NewPrio(ref pr,ref pp) => 
        PeerMgmtCommand::NewPrio(pr.clone(),pp.clone()),
    }
  }
}

impl<MC : MyDHTConf> SRef for PeerMgmtCommand<MC> 
  where MC::GlobalServiceCommand : SRef,
        MC::LocalServiceCommand : SRef,
        MC::GlobalServiceReply : SRef,
        MC::LocalServiceReply : SRef {
  type Send = PeerMgmtCommandSend<MC>;
  fn get_sendable(self) -> Self::Send {
    match self {
      PeerMgmtCommand::Accept(pr,com) => 
        PeerMgmtCommandSend::Accept(pr.get_sendable(),com.get_sendable()),
      PeerMgmtCommand::NewPrio(pr,pp) => 
        PeerMgmtCommandSend::NewPrio(pr.get_sendable(),pp),
    }
  }
}

pub enum PeerMgmtCommandSend<MC : MyDHTConf> 
  where MC::GlobalServiceCommand : SRef,
        MC::LocalServiceCommand : SRef,
        MC::GlobalServiceReply : SRef,
        MC::LocalServiceReply : SRef {
  Accept(PeerRefSend<MC>, MainLoopCommandSend<MC>),
  NewPrio(PeerRefSend<MC>, PeerPriority),
}

impl<MC : MyDHTConf> SToRef<PeerMgmtCommand<MC>> for PeerMgmtCommandSend<MC> 
  where MC::GlobalServiceCommand : SRef,
        MC::LocalServiceCommand : SRef,
        MC::GlobalServiceReply : SRef,
        MC::LocalServiceReply : SRef {
  fn to_ref(self) -> PeerMgmtCommand<MC> {
    match self {
      PeerMgmtCommandSend::Accept(pr,com) => 
        PeerMgmtCommand::Accept(pr.to_ref(),com.to_ref()),
      PeerMgmtCommandSend::NewPrio(pr,pp) => 
        PeerMgmtCommand::NewPrio(pr.to_ref(),pp),
    }
  }
}


