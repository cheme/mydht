use std::io::Result as IoResult;
use std::sync::Arc;
use std::sync::mpsc::{Sender,Receiver};
use std::net::{ToSocketAddrs, SocketAddr};
use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};
use procs::mesgs::{PeerMgmtMessage,KVStoreMgmtMessage};
use std::string::String;
use std::str::FromStr;
use procs::{RunningProcesses,RunningContext,ArcRunningContext,RunningTypes};
use msgenc::{MsgEnc};
use transport::{Transport};
use keyval::KeyVal;
use utils::{OneResult,ret_one_result};
use utils::TransientOption;

pub mod node;

/// A peer is a special keyval with an attached address over the network
pub trait Peer : KeyVal {
  type Address;
  // TODO rename to get_address or get_address_clone, here name is realy bad
  fn to_address (&self) -> Self::Address;
//  fn to_address (&self) -> SocketAddr;
}

#[derive(RustcDecodable,RustcEncodable,Debug,PartialEq,Clone)]
/// State of a peer
pub enum PeerPriority {
  /// peers is online but not already accepted
  /// it is used when receiving a ping, peermanager on unchecked should ping only if auth is
  /// required
  Unchecked,
  /// online, no prio distinction between peers
  Normal,
  /// online, with a u8 priority
  Priority(u8),
}


#[derive(Debug,PartialEq,Clone)]
/// Change to State of a peer
pub enum PeerStateChange {
  Refused,
  Blocked,
  Offline,
  Online,
  Ping(String, TransientOption<OneResult<bool>>),
}

#[derive(RustcDecodable,RustcEncodable,Debug,PartialEq,Clone)]
/// State of a peer
pub enum PeerState {
  /// accept running on peer return None, it may be nice to store on heavy accept but it is not
  /// mandatory
  Refused,
  /// some invalid frame and others lead to block a peer, connection may be retry if address
  /// changed for the peer
  Blocked(PeerPriority),
  /// This priority is more an internal state and should not be return by accept.
  /// peers has not yet been ping or connection broke
  Offline(PeerPriority),
 /// This priority is more an internal state and should not be return by accept.
  /// sending ping with given challenge string, a PeerPriority is set if accept has been run
  /// (lightweight accept) or Unchecked
  Ping(String,TransientOption<OneResult<bool>>,PeerPriority),
  /// Online node
  Online(PeerPriority),
}

/// Ping ret one result return false as soon as we do not follow it anymore
impl Drop for PeerState {
    fn drop(&mut self) {
        debug!("Drop of PeerState");
        match self {
          &mut PeerState::Ping(_,ref mut or,_) => {
            or.0.as_ref().map(|r|ret_one_result(&r,false)).is_some();
            ()
          },
          _ => (),
        }
    }
}


impl PeerState {
  pub fn get_priority(&self) -> PeerPriority {
    match self {
      &PeerState::Refused => PeerPriority::Unchecked,
      &PeerState::Blocked(ref p) => p.clone(),
      &PeerState::Offline(ref p) => p.clone(),
      &PeerState::Ping(_,_,ref p) => p.clone(),
      &PeerState::Online(ref p) => p.clone(),
    }
  }
  pub fn new_state(&self, change : PeerStateChange) -> PeerState {
    let pri = self.get_priority();
    match change {
      PeerStateChange::Refused => PeerState::Refused,
      PeerStateChange::Blocked => PeerState::Blocked(pri),
      PeerStateChange::Offline => PeerState::Offline(pri),
      PeerStateChange::Online  => PeerState::Online(pri), 
      PeerStateChange::Ping(chal,or)  => PeerState::Ping(chal,or,pri), 
    }
  }
}
/// Rules for peers. Usefull for web of trust for instance, or just to block some peers.
pub trait PeerMgmtMeths<P : Peer, V : KeyVal> : Send + Sync {
  /// get challenge for a node, most of the time a random value to avoid replay attack
  fn challenge (&self, &P) -> String; 
  /// sign a message. Node and challenge.
  fn signmsg   (&self, &P, &String) -> String; // node for signing and challenge if private key usage, pk is define in function
  /// check a message. Peer, challenge and signature.
  fn checkmsg  (&self, &P, &String, &String) -> bool; // node, challenge and signature
  /// accept a peer? (reference to running process and running context could be use to query
  /// ourself
  fn accept<RT : RunningTypes<P=P,V=V>> (&self, &P, &RunningProcesses<RT>, &ArcRunningContext<RT>) -> Option<PeerPriority>;
  // call from accept it will loop on sending info to never online peer)
  /// Post action after adding a new online peer : eg propagate or update this in another store
  fn for_accept_ping<RT : RunningTypes<P=P,V=V>> (&self, &Arc<P>, &RunningProcesses<RT>, &ArcRunningContext<RT>);
}


