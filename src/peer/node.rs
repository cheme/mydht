use std::net::{SocketAddr};
use rustc_serialize::{Encoder,Encodable,Decoder};
use peer::Peer;
use peer::NoShadow;
use std::string::String;
use keyval::{KeyVal};
use keyval::{Attachment,SettableAttachment};
use utils::SocketAddrExt;

#[derive(RustcDecodable,RustcEncodable,Debug,PartialEq,Eq,Clone)]
pub struct Node {
  pub nodeid  : NodeID,
  pub address : SocketAddrExt,
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
  type Address = SocketAddr;
  fn to_address(&self) -> SocketAddr {
    self.address.0
  }
  noshadow!();
}

