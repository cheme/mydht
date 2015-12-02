use std::sync::Arc;
use rustc_serialize::{Encoder,Encodable,Decoder};
use std::string::String;
use procs::{RunningProcesses,ArcRunningContext,RunningTypes};
use msgenc::send_variant::ProtoMessage as ProtoMessageSend;
use transport::{Address};
use keyval::KeyVal;
use utils::{OneResult,unlock_one_result};
//use utils::{ret_one_result};
use utils::TransientOption;
use std::io::Write;
use std::io::Read;
use std::io::Result as IoResult;

#[cfg(test)]
pub mod test;

// reexport from base
pub use mydht_base::peer::*;

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
  Ping(Vec<u8>,TransientOption<OneResult<bool>>,PeerPriority),
  /// Online node
  Online(PeerPriority),
}

/// Ping ret one result return false as soon as we do not follow it anymore
impl Drop for PeerState {
    fn drop(&mut self) {
        debug!("Drop of PeerState");
        match self {
          &mut PeerState::Ping(_,ref mut or,_) => {
            or.0.as_ref().map(|r|unlock_one_result(&r,false)).is_some();
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

#[derive(Debug,PartialEq,Clone)]
/// Change to State of a peer
pub enum PeerStateChange {
  Refused,
  Blocked,
  Offline,
  Online,
  Ping(Vec<u8>, TransientOption<OneResult<bool>>),
}


/// Rules for peers. Usefull for web of trust for instance, or just to block some peers.
/// TODO refactor : replace String by bytes array and Vec<u8>!! (+ &String makes no sense)
pub trait PeerMgmtMeths<P : Peer, V : KeyVal> : Send + Sync + 'static {
  /// get challenge for a node, most of the time a random value to avoid replay attack
  fn challenge (&self, &P) -> Vec<u8>; 
  /// sign a message. Node and challenge. Node in parameter is ourselve.
  fn signmsg (&self, &P, &[u8]) -> Vec<u8>; // node for signing and challenge if private key usage, pk is define in function
  /// check a message. Peer, challenge and signature.
  fn checkmsg (&self, &P, &[u8], &[u8]) -> bool; // node, challenge and signature
  /// accept a peer? (reference to running process and running context could be use to query
  /// ourself
  fn accept<M : PeerMgmtMeths<P,V>, RT : RunningTypes<P=P,V=V,A=P::Address,M=M>> (&self, &P, &RunningProcesses<RT>, &ArcRunningContext<RT>) -> Option<PeerPriority>;
  // call from accept it will loop on sending info to never online peer)
  /// Post action after adding a new online peer : eg propagate or update this in another store
  fn for_accept_ping<M : PeerMgmtMeths<P,V>, RT : RunningTypes<P=P,V=V,A=P::Address,M=M>> (&self, &Arc<P>, &RunningProcesses<RT>, &ArcRunningContext<RT>);
/*
  /// shadow a message using peer shadowing implementation, this interface allows custom usage of
  /// shadowing depending upon message content.
  /// Optional peer and message are content to be send, they could be use to apply different
  /// shadowing depending on message content (without having to deserialize the payload).
  /// Reference to running process and running cortext should be pass in the future (allowing
  /// shared keyval secret in truststore usage), for now if this is needed peer need to be updated
  /// with the shared secret. TODO return a w.shadower
  fn shadow (&self, &P, Vec<u8>, Option<&P>, Option<&V>) -> Vec<u8>;
  /// unshadow a message, co to shadow, to manage additional info that shadow may have added
  /// (especially if shadow apply multiple pattern of shadowing).
  /// We need to have shorter unshadowed slice (compression should not be implement in shadowing
  /// but in message encoding). TODO return a r.shadower, and maybe a shorter slice
  fn get_unshadower<'a> (&self, &P, &'a [u8]) -> (rshad, &'a [u8]);
*/
  /// Resolve mode of shadowing to use for a peer and and a message. TODO might need ref to running
  /// processes and contexts (or just our peer), but keep it simple for now
  /// Default implementation relies on shadow message
  fn get_shadower (&self, p : &P, m : &ProtoMessageSend<P,V>) -> <P::Shadow as Shadow>::ShadowMode {
    match m {
      &ProtoMessageSend::PING(..) |&ProtoMessageSend::PONG(..) => <P as Peer>::Shadow::default_auth_mode(),
      _ => <P as Peer>::Shadow::default_message_mode(),
    }
  }
}

