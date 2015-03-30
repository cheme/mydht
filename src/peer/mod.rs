use std::io::Result as IoResult;
use std::sync::Arc;
use std::sync::mpsc::{Sender,Receiver};
use std::net::{ToSocketAddrs, SocketAddr};
use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};
use procs::mesgs::{PeerMgmtMessage,KVStoreMgmtMessage};
use std::string::String;
use std::str::FromStr;
use procs::{RunningProcesses,RunningContext};
use query::{QueryRules};
use msgenc::{MsgEnc};
use transport::{Transport};
use kvstore::KeyVal;

pub mod node;

/// A peer is a special keyval with an attached address over the network
pub trait Peer : KeyVal {
//  type Address : 'static;
//  fn to_address (&self) -> Self::Address;
  fn to_address (&self) -> SocketAddr;
}

#[derive(RustcDecodable,RustcEncodable,Debug,Copy,PartialEq,Clone)]
/// State of a peer
pub enum PeerPriority {
  /// our DHT rules reject the peer
  Blocked,
  /// peers has not yet been ping or connection broke
  Offline,
  /// online, no prio distinction between peers
  Normal,
  /// online, with a u8 priority
  Priority(u8),
}


/// Rules for peers. Usefull for web of trust for instance, or just to block some peers.
pub trait PeerMgmtRules<P : Peer, V : KeyVal> : Send + Sync + 'static {
  /// get challenge for a node, most of the time a random value to avoid replay attack
  fn challenge (&self, &P) -> String; 
  /// sign a message. Node and challenge.
  fn signmsg   (&self, &P, &String) -> String; // node for signing and challenge if private key usage, pk is define in function
  /// check a message. Peer, challenge and signature.
  fn checkmsg  (&self, &P, &String, &String) -> bool; // node, challenge and signature
  /// accept a peer? (reference to running process and running context could be use to query
  /// ourself
  fn accept<R : PeerMgmtRules<P,V>, Q : QueryRules, E : MsgEnc, T : Transport> (&self, &Arc<P>, &RunningProcesses<P,V>, &RunningContext<P,V,R,Q,E,T>) -> Option<PeerPriority>;
  // call from accept it will loop on sending info to never online peer)
  /// Post action after adding a new online peer : eg propagate or update this in another store
  fn for_accept_ping<R : PeerMgmtRules<P,V>, Q : QueryRules, E : MsgEnc, T : Transport> (&self, &Arc<P>, &RunningProcesses<P,V>, &RunningContext<P,V,R,Q,E,T>);
}


