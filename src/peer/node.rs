use std::net::{SocketAddr};
use std::io::Result as IoResult;
use std::sync::Arc;
use std::sync::mpsc::{Sender,Receiver};
use std::net::{ToSocketAddrs};
use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};
use procs::mesgs::{PeerMgmtMessage,KVStoreMgmtMessage,QueryMgmtMessage};
use std::string::String;
use std::str::FromStr;
use peer::Peer;
use kvstore::{KeyVal};
use kvstore::Attachment;
use utils::SocketAddrExt;

#[derive(RustcDecodable,RustcEncodable,Debug,PartialEq,Eq,Clone)]
pub struct Node {
  pub nodeid  : NodeID,
  pub address : SocketAddrExt,
}

pub type NodeID = String;

impl KeyVal for Node {
  type Key = NodeID;
  fn get_key(&self) -> NodeID {
    self.nodeid.clone()
  }
  nospecificencoding!(Node);
  noattachment!();
}

impl Peer for Node{
//  type Address = SocketAddr;
  fn to_address(&self) -> SocketAddr {
    self.address.0
  }

}


