//! peer management service : store peer as backend (kvstore) and additional functionality (long accept for instance)
use super::{
  MyDHTConf,
};
use peer::{
  PeerMgmtMeths,
  PeerPriority,
};

pub enum PeerMgmtCommand<MC : MyDHTConf> {
  /// peer to check
  Accept(MC::PeerRef),
  /// new peer with its prio
  NewPrio(MC::PeerRef, PeerPriority),

}
