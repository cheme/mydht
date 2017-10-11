//! types to disable service
use mydht_base::route2::RouteBaseMessage;
use super::api::{
  ApiQueryId,
  ApiQueriable,
  ApiRepliable,
};
use peer::{
  Peer,
};
use keyval::{
  KeyVal,
};
use service::{
  Service,
  SpawnerYield,
};
use mydhtresult::{
  Result,
};
use std::collections::VecDeque;

#[derive(Debug,Clone)]
pub struct NoCommandReply;

impl ApiQueriable for NoCommandReply {
  #[inline]
  fn is_api_reply(&self) -> bool {
    false
  }
  #[inline]
  fn set_api_reply(&mut self, _ : ApiQueryId) {
  }
  #[inline]
  fn get_api_reply(&self) -> Option<ApiQueryId> {
    None
  }
}
impl ApiRepliable for NoCommandReply {
  #[inline]
  fn get_api_reply(&self) -> Option<ApiQueryId> {
    None
  }
}


impl<P : Peer> RouteBaseMessage<P> for NoCommandReply {
  #[inline]
  fn get_filter_mut(&mut self) -> Option<&mut VecDeque<<P as KeyVal>::Key>> {
    None
  }
  fn adjust_lastsent_next_hop(&mut self,_ : usize) {}
}
