//! peer management service : store peer as backend (kvstore) and additional functionality (long accept for instance)
//! TODO unimplemented : failure for asynch peer management implementation
use super::{
  MyDHTConf,
  MainLoopCommand,
};
use peer::{
  PeerMgmtMeths,
  PeerPriority,
};

pub enum PeerMgmtCommand<MC : MyDHTConf> {
  /// peer to check and command to proxy TODO the main loop command must implement a set peerprio
  /// trait !!!!
  Accept(MC::PeerRef, MainLoopCommand<MC>),
  /// new peer with its prio
  NewPrio(MC::PeerRef, PeerPriority),

}
