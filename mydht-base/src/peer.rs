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
  CompW,
  CompWState,
  CompR,
  CompRState,
};
use std::fmt::Debug;
use rustc_serialize::{Encodable, Decodable};


/// A peer is a special keyval with an attached address over the network
pub trait Peer : KeyVal + 'static {
  type Address : Address;
  type Shadow : Shadow;
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
  fn get_shadower (&self, write : bool) -> Self::Shadow;
//  fn to_address (&self) -> SocketAddr;
  /// mode for shadowing frame for authentification (no shadow most of the tipme) : aka ping/pong,
  /// establishing a connection - this is use by default by peer rules
  fn default_auth_mode(&self) -> <Self::Shadow as Shadow>::ShadowMode;
  /// mode to shadow message and also to shadow layer of tunnel TODO switch to two different modes
  fn default_message_mode(&self) -> <Self::Shadow as Shadow>::ShadowMode;
  /// mode for header of tunnel
  fn default_header_mode(&self) -> <Self::Shadow as Shadow>::ShadowMode;
}

pub trait ShadowBase : Send + 'static + ExtRead + ExtWrite + Sized {
  #[inline]
  fn read_shadow_header<R : Read> (&mut self, r : &mut R) -> IoResult<()> {
    self.read_header(r)
  }
  #[inline]
  fn read_shadow_iter<R : Read> (&mut self, r : &mut R, buf: &mut [u8]) -> IoResult<usize> {
    self.read_from(r, buf)
  }
  #[inline]
  /// get the header required for a shadow scheme : for instance the shadowmode representation and
  /// the salt. The action also make the shadow iter method to use the right state (internal state
  /// for shadow mode). TODO refactor to directly write in a Writer??
  fn shadow_header<W : Write> (&mut self, w : &mut W) -> IoResult<()> {
    self.write_header(w)
  }
  #[inline]
  /// Shadow of message block (to use in the writer), and write it in writer. The function maps
  /// over write (see utils :: send_msg)
  fn shadow_iter<W : Write> (&mut self, cont : &[u8], w : &mut W) -> IoResult<usize> {
    self.write_into(w, cont)
  }
  #[inline]
  /// flush at the end of message writing (useless to add content : reader could not read it),
  /// usefull to emptied some block cyper buffer, it does not flush the writer which should be
  /// flush manually (allow using multiple shadower in messages such as tunnel).
  fn shadow_flush<W : Write> (&mut self, w : &mut W) -> IoResult<()> {
    self.flush_into(w)
  }

}

pub trait ShadowSim : ShadowBase {
  /// return simkey to be send or exchange. similar to write header but the key is not used,
  /// it is only info to reply with. TODO see usage but very likely to allways use get_mode to send
  fn send_shadow_simkey<W : Write>(&self, _ : &mut W) -> IoResult<()>;
 
  /// init from read key, similar to read header but to be use for writing.
  fn init_from_shadow_simkey<R : Read>(_ : &mut R) -> IoResult<Self>;

}

/// TODO after tunnel implementation see if mode of shadow really need change, if not 
/// refactor by using mode in get_shadow of peer directly and removing set_mode : allowing
/// enum for shadow (see how to pack for not max size (noshadowmode). It also means that read
/// header will no longer need to get the shadowmode from the header (less overhead).
///
/// Note that Shadow could be split in three : ReadShadow, WriteShadow and SymmetricShadow.
/// Using a single trait is convenient for a simplier interface, but a Shadow could not be use 
/// in two of the three previously describe context (except if implementation allows it).
/// TODO sim to get_sim and other if (associated sim)
pub trait Shadow : ShadowBase {
  /// associated sim shadow
  type ShadowSim : ShadowSim;
  /// type of shadow to apply (most of the time this will be () or bool but some use case may
  /// require 
  /// multiple shadowing scheme, and therefore probably an enum type).
  type ShadowMode : Clone + Eq + Encodable + Decodable + Debug;

  fn set_mode(&mut self, Self::ShadowMode);

  fn get_mode(&self) -> Self::ShadowMode;

  fn new_shadow_sim () -> IoResult<Self::ShadowSim>;

}


/// a struct for reading once with shadow TODO two lifetime?? yes after refactoring Comp is ok
pub type ShadowReadOnce<'a, S : 'a + Shadow, R : 'a + Read> = CompR<'a, 'a, R, S>;

#[inline]
pub fn new_shadow_read_once<'a, S : 'a + Shadow, R : 'a + Read>(r : &'a mut R, s : &'a mut S) -> ShadowReadOnce<'a,S,R> {
  CompR(r,s,CompRState::Initial)
}


/// a struct for writing once with shadow (in a write abstraction)
/// Flush does not lead to a new header write (for this please create a new shadowWriteOnce).
/// Flush does not flush the inner writer but only the shadower (to flush writer please first end 
pub type ShadowWriteOnce<'a, S : 'a + Shadow, W : 'a + Write> = CompW<'a, 'a, W , S>;
//pub struct ShadowWriteOnce<'a, S : 'a + Shadow, W : 'a + Write>(&'a mut S, &'a <S as Shadow>::ShadowMode, &'a mut W, bool);

#[inline]
pub fn new_shadow_write_once<'a, S : 'a + Shadow, W : 'a + Write>(r : &'a mut W, s : &'a mut S) -> ShadowWriteOnce<'a,S,W> {
  CompW(r,s,CompWState::Initial)
}

/// layerable shadowrite once (fat pointer on reader and recursive flush up to terminal reader). //
/// last bool is to tell if it is first (reader is not flush on first) TODO not use remove
pub struct ShadowWriteOnceL<'a, S : Shadow>(S, (), &'a mut Write, bool,bool);



/// Multiple layered shadow write
pub struct MShadowWriteOnce<'a, S : 'a + Shadow, W : 'a + Write>(&'a mut [S], (), &'a mut W, &'a mut [bool]);


// TODO delete it
impl<'a, S : 'a + Shadow> ShadowWriteOnceL<'a,S> {
  pub fn new(s : S, r : &'a mut Write, is_first : bool) -> Self {
    ShadowWriteOnceL(s, (), r, false, is_first)
  }
}
impl<'a, S : 'a + Shadow, W : 'a + Write> MShadowWriteOnce<'a,S,W> {
  pub fn new(s : &'a mut [S], r : &'a mut W, writtenhead : &'a mut [bool]) -> Self {
    // TODO use Vec and set mode of all S
    MShadowWriteOnce(s, (), r, writtenhead)
  }
}

impl<'a, S : 'a + Shadow, W : 'a + Write> Write for MShadowWriteOnce<'a,S,W> {
  fn write(&mut self, cont: &[u8]) -> IoResult<usize> {
    if !(self.3)[0] {
      // write header
      if self.0.len() > 1 {
      if let Some((f,last)) = self.0.split_first_mut() {
      let mut el = MShadowWriteOnce(last, (),self.2, &mut self.3[1..]); 
      try!(f.shadow_header(&mut el));
      }} else {
        try!((self.0).get_mut(0).unwrap().shadow_header(self.2));
      }
      (self.3)[0] = true;
    }
    if self.0.len() > 1 {
    if let Some((f,last)) = self.0.split_first_mut() {
      let mut el = MShadowWriteOnce(last,(), self.2, &mut self.3[1..]); 
      return f.shadow_iter(cont, &mut el);
    }
    }
    // last
    (self.0).get_mut(0).unwrap().shadow_iter(cont, self.2)
  }
  fn flush(&mut self) -> IoResult<()> {
    if self.0.len() > 1 {
    if let Some((f,last)) = self.0.split_first_mut()  {
      let mut el = MShadowWriteOnce(last, (),self.2, &mut self.3[1..]); 
      try!(f.shadow_flush(&mut el));
      return el.flush();
    }
    }
    // last do not flush writer
    (self.0).get_mut(0).unwrap().shadow_flush(self.2)
 
  }
}
/*
impl<'a, S : 'a + Shadow> Write for MShadowWriteOnce<'a,S> {

  fn write(&mut self, cont: &[u8]) -> IoResult<usize> {
    for i in cont.len() .. 0 {
      if !self.3[i-1] {
        try!(self.0.shadow_header(&mut self.2, &self.1));
        self.3 = true;
      }
      self.0.get_mut(i-1).unwrap().shadow_iter(cont, &mut self.2, &self.1)
    }
  }
 
  /// flush does not flush the first inner writer
  fn flush(&mut self) -> IoResult<()> {
    // do not flush last
    for i in 0 .. cont.len() - 1 {
      try!(self.0.get_mut(i).unwrap().shadow_flush(&mut self.2, &self.1));
    }
  }
}*/



/* impl in CompW
impl<'a, S : 'a + Shadow, W : 'a + Write> Write for ShadowWriteOnce<'a,S,W> {

  fn write(&mut self, cont: &[u8]) -> IoResult<usize> {
    if !self.3 {
      try!(self.0.shadow_header(&mut self.2, &self.1));
      self.3 = true;
    }
    self.0.shadow_iter(cont, &mut self.2, &self.1)
  }
 
  /// flush does not flush the inner writer
  fn flush(&mut self) -> IoResult<()> {
    self.0.shadow_flush(&mut self.2, &self.1)
  }
}*/

impl<'a, S : 'a + Shadow> Write for ShadowWriteOnceL<'a,S> {

  fn write(&mut self, cont: &[u8]) -> IoResult<usize> {
    if !self.3 {
      try!(self.0.shadow_header(&mut self.2));
      self.3 = true;
    }
    self.0.shadow_iter(cont, &mut self.2)
  }
 
  /// flush does not flush the first inner writer
  fn flush(&mut self) -> IoResult<()> {
    try!(self.0.shadow_flush(&mut self.2));
    if !self.4 {
      self.2.flush()
    } else {
      Ok(())
    }
  }
}



pub struct NoShadow;
impl ShadowBase for NoShadow {



}

impl Shadow for NoShadow {
  type ShadowSim = NoShadow;

  type ShadowMode = ();
  #[inline]
  fn new_shadow_sim () -> IoResult<Self::ShadowSim> {
    Ok(NoShadow)
  }
 
  #[inline]
  fn set_mode (&mut self,_ : Self::ShadowMode)  {}
  #[inline]
  fn get_mode (&self) -> Self::ShadowMode {()}



}
impl ShadowSim for NoShadow {
  #[inline]
  fn send_shadow_simkey<W : Write>(&self, _ : &mut W) -> IoResult<()> {
    Ok(())
  }
  #[inline]
  fn init_from_shadow_simkey<R : Read>(_ : &mut R) -> IoResult<Self> {
    Ok(NoShadow)
  }
 
}
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
  fn flush_into<W : Write>(&mut self, w : &mut W) -> IoResult<()> {Ok(())}
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


