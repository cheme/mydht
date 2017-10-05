use std::collections::VecDeque;
use transport::{
  Transport,
  Address,
};
use mydhtresult::Result;

use peer::{Peer,PeerState,PeerStateChange};
use keyval::KeyVal;

use std::sync::Arc;
use std::marker::PhantomData;

use std::borrow::Borrow;
use kvcache::Cache;


/// tech trait to adapt to peercacheentry
/// return peer ref for internal route base implementation, and optional index of write
pub trait GetPeerRef<P,RP> {
  fn get_peer_ref(&self) -> (&P,Option<usize>);
}
pub trait RouteBaseMessage<P : Peer> {
  fn get_filter(&self) -> Option<&VecDeque<<P as KeyVal>::Key>>;
}
pub enum RouteMsgType {
  Local,
  Global,
  PeerStore,
}
pub trait RouteBase<P : Peer,RP : Borrow<P>,GP : GetPeerRef<P,RP>> : Cache<<P as KeyVal>::Key,GP> {
  fn route_base<MSG : RouteBaseMessage<P>>(&mut self, usize, MSG, RouteMsgType) -> Result<(MSG,Vec<usize>)>;
}


