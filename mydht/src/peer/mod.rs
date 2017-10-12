
#[cfg(test)]
pub mod test;
// reexport from base
pub use mydht_base::peer::*;



/// Rules for peers. Usefull for web of trust for instance, or just to block some peers.
/// TODO simplify + remove sync
pub trait PeerMgmtMeths<P : Peer> : Send + Sync + 'static + Clone {
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
}


