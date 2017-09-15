use std::net::{SocketAddr};
use serde::{Serializer,Serialize,Deserializer};
use peer::Peer;
use peer::{
  NoShadow,
};
use readwrite_comp::{
  ExtRead,
  ExtWrite,
};


use std::string::String;
use keyval::{KeyVal};
use keyval::{Attachment,SettableAttachment};
use mydht_base::transport::SerSocketAddr;
use mydht_base::route::byte_rep::DHTElemBytes;




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
/* 
  #[inline]
  fn get_key_ref<'a>(&'a self) -> &'a NodeID {
    &self.nodeid
  }*/
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

  impl<'a> DHTElemBytes<'a> for Node {
  // return ing Vec<u8> is stupid but it is for testing
  type Bytes = Vec<u8>;
  fn bytes_ref_keb (&'a self) -> Self::Bytes {
    self.nodeid.bytes_ref_keb()
  }
  fn kelem_eq_keb(&self, other : &Self) -> bool {
    self == other
  }
}

