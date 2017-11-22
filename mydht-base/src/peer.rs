use transport::Address;
use keyval::KeyVal;
use utils::{
  TransientOption,
  OneResult,
  unlock_one_result,
};
use std::io::{
  Write,
  Read,
  Result as IoResult,
};
use readwrite_comp::{
  ExtRead,
  ExtWrite,
};

/// A peer is a special keyval with an attached address over the network
pub trait Peer : KeyVal + 'static {
  /// Address of the peer, this is the last low level address for which the peer is known to reply.
  /// This address may not be the address to which we are connected, with a tcp connection, it is
  /// the address with the listener port, not the port use by the stream : thus it is publish on
  /// handshake
  type Address : Address;
  type ShadowWAuth : ExtWrite + Send;
  type ShadowRAuth : ExtRead + Send;
  type ShadowWMsg : ExtWrite + Send;
  type ShadowRMsg : ExtRead + Send;
  fn get_address (&self) -> &Self::Address;
 // TODO rename to get_address or get_address_clone, here name is realy bad
  // + see if a ref returned would not be best (same limitation as get_key for composing types in
  // multi transport) + TODO alternative with ref to address for some cases
  fn to_address (&self) -> Self::Address {
    self.get_address().clone()
  }
  /// instantiate a new shadower for this peer (shadower wrap over write stream and got a lifetime
  /// of the connection). // TODO enum instead of bool!! (currently true for write mode false for
  /// read only)
  fn get_shadower_r_auth (&self) -> Self::ShadowRAuth;
  fn get_shadower_r_msg (&self) -> Self::ShadowRMsg;
  fn get_shadower_w_auth (&self) -> Self::ShadowWAuth;
  fn get_shadower_w_msg (&self) -> Self::ShadowWMsg;
//  fn to_address (&self) -> SocketAddr;
}



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





pub struct NoShadow;


/*
impl ShadowSim for NoShadow {
  #[inline]
  fn send_shadow_simkey<W : Write>(&self, _ : &mut W) -> IoResult<()> {
    Ok(())
  }
  #[inline]
  fn init_from_shadow_simkey<R : Read>(_ : &mut R) -> IoResult<Self> {
    Ok(NoShadow)
  }
 
}*/
impl ExtWrite for NoShadow {
  #[inline]
  fn write_header<W : Write>(&mut self, _ : &mut W) -> IoResult<()> {
    Ok(())
  }
  #[inline]
  fn write_into<W : Write>(&mut self, w : &mut W, cont : &[u8]) -> IoResult<usize> {
    w.write(cont)
  }   
  #[inline]
  fn flush_into<W : Write>(&mut self, _ : &mut W) -> IoResult<()> {Ok(())}
  #[inline]
  fn write_end<W : Write>(&mut self, _ : &mut W) -> IoResult<()> {Ok(())}
}
impl ExtRead for NoShadow {
  #[inline]
  fn read_header<R : Read>(&mut self, _ : &mut R) -> IoResult<()> {Ok(())}
  #[inline]
  fn read_from<R : Read>(&mut self, r : &mut R, buf : &mut[u8]) -> IoResult<usize> {
    r.read(buf)
  }
  #[inline]
  fn read_end<R : Read>(&mut self, _ : &mut R) -> IoResult<()> {Ok(())}
}




#[derive(Deserialize,Serialize,Debug,PartialEq,Clone)]
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



#[derive(Deserialize,Serialize,Debug,PartialEq,Clone)]
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


