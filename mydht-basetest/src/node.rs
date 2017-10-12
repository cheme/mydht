use peer::Peer;
use peer::{
  NoShadow,
};


use std::string::String;
use keyval::{KeyVal};
use keyval::{Attachment,SettableAttachment};
use mydht_base::transport::SerSocketAddr;




#[derive(Deserialize,Serialize,Debug,PartialEq,Eq,Clone)]
pub struct Node {
  pub nodeid  : NodeID,
  pub address : SerSocketAddr,
}

pub type NodeID = String;

impl KeyVal for Node {
  type Key = NodeID;
  #[inline]
  fn get_key(& self) -> NodeID {
    self.nodeid.clone()
  }
  #[inline]
  fn get_key_ref(&self) -> &NodeID {
    &self.nodeid
  }
  noattachment!();
}

impl SettableAttachment for Node { }

impl Peer for Node {
  type Address = SerSocketAddr;
  fn get_address(&self) -> &SerSocketAddr {
    &self.address
  }
  noshadow_auth!();
  noshadow_msg!();
}


