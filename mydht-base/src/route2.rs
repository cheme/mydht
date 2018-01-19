use std::collections::VecDeque;
use mydhtresult::Result;

use peer::{
  Peer,
  PeerPriority,
};
use keyval::KeyVal;


use std::borrow::Borrow;
use kvcache::Cache;


/// tech trait to adapt to peercacheentry
/// return peer ref for internal route base implementation, and optional index of write
pub trait GetPeerRef<P,RP> {
  fn get_peer_ref(&self) -> (&P,&PeerPriority,Option<usize>);
}

pub trait RouteBaseMessage<P : Peer> {
  fn get_filter_mut(&mut self) -> Option<&mut VecDeque<<P as KeyVal>::Key>>;
  fn adjust_lastsent_next_hop(&mut self, nbquery : usize);
}
pub enum RouteMsgType {
  Local,
  Global,
  PeerStore,
}
pub trait RouteBase<P : Peer,RP : Borrow<P>,GP : GetPeerRef<P,RP>> : Cache<<P as KeyVal>::Key,GP> {
  fn route_base<MSG : RouteBaseMessage<P>>(&mut self, usize, MSG, RouteMsgType) -> Result<(MSG,Vec<usize>)>;
}

/// get index of write for a given index (currently use to get random destination)
pub trait IndexableWriteCache {
  fn get_at(&self, ix : usize) -> Option<usize>;
}


