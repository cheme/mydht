use std::io::Result as IoResult;
use std::io::Write;
use std::io::Read;
use time::Duration;
use peer::{Peer};
use std::net::{SocketAddr};
use std::path::PathBuf;

pub mod tcp;
pub mod udp;
pub type Attachment = PathBuf;
/// Transport trait
pub trait Transport : Send + Sync + 'static {
  /// Transport stream
  type Stream : TransportStream;
  /// Address type
//  type Address = SocketAddr;
  /// Is transport connected
  fn is_connected() -> bool;
  /// Transport reception loop, function option is the message for non connected transport, for
  /// connected transport the stream is used.
  //fn receive<C> (&self, &Self::Address, C) where C : Fn(Self::Stream, Option<(Vec<u8>, Option<Attachment>)>) -> ();
  fn receive<C> (&self, &SocketAddr, C) where C : Fn(Self::Stream, Option<(Vec<u8>, Option<Attachment>)>) -> ();

  /// Transport initialisation for sending
  //fn connectwith(&self, &Self::Address, Duration) -> IoResult<Self::Stream>;
  fn connectwith(&self, &SocketAddr, Duration) -> IoResult<Self::Stream>;

}

/// Transport stream
pub trait TransportStream : Send + Sync + 'static + Write + Read {
/* 
/// write to someone 
fn streamwrite(&mut self, &[u8], Option<&Attachment>) -> IoResult<()>;
*/
/*
/// read from someone 
fn streamread(&mut self) -> IoResult<(Vec<u8>,Option<Attachment>)>;
*/
}
