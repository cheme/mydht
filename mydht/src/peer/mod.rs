use std::sync::Arc;
use serde::{Serializer,Serialize,Deserializer};
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



/// Rules for peers. Usefull for web of trust for instance, or just to block some peers.
/// TODO simplify
pub trait PeerMgmtMeths<P : Peer> : Send + Sync + 'static {
  /// get challenge for a node, most of the time a random value to avoid replay attack
  fn challenge (&self, &P) -> Vec<u8>; 
  /// sign a message. Node and challenge. Node in parameter is ourselve.
  fn signmsg (&self, &P, &[u8]) -> Vec<u8>; // node for signing and challenge if private key usage, pk is define in function
  /// check a message. Peer, challenge and signature.
  fn checkmsg (&self, &P, &[u8], &[u8]) -> bool; // node, challenge and signature
  /// accept a peer? (reference to running process and running context could be use to query
  /// ourself
  /// Post PONG message handle
  /// If accept is heavy it can run asynch by returning PeerPriority::Unchecked and sending, then
  /// check will be done by sending accept query to PeerMgmt service
  fn accept (&self, &P) -> Option<PeerPriority>;
  // call from accept it will loop on sending info to never online peer)
  // TODO still useful?? Post action after adding a new online peer : eg propagate or update this in another store
  // Post PING message handle (on successfull challenge)
//  fn for_accept_ping<M : PeerMgmtMeths<P,V>, RT : RunningTypes<P=P,V=V,A=P::Address,M=M>> (&self, &Arc<P>, &RunningProcesses<RT>, &ArcRunningContext<RT>);
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
}


