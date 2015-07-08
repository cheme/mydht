use std::io::Result as IoResult;
use std::io::Write;
use std::io::Read;
use time::Duration;
use peer::{Peer};
use std::net::{SocketAddr};
use std::path::PathBuf;
use mydhtresult::Result; 


#[cfg(feature="mio-impl")]
pub mod tcp_loop;
pub mod tcp;
pub mod udp;
pub type Attachment = PathBuf;


/// Transport trait
/// TODO a synch primitive to check if start in ok state (if needed (see tcp_loop where init
/// eventloop)).
pub trait Transport : Send + Sync + 'static {

  /// Transport stream
  type Stream : TransportStream;
  /// Address type
//  type Address = SocketAddr;
  /// Is transport connected
  fn is_connected() -> bool;
 
  /// Start and run transport reception loop (Should only return at the end of the DHT).
  /// Optional function parameter is a handler for the message, two cases :
  ///   - reception loop is into the handler function, and we loop on every TransportStream (in its
  ///   own thread) for message reception, in this case the second parameter of the handler is None
  ///   and the handler will use IO::Read over the TransportStream.
  ///   - server thread will loop over Transport `receive` and use send result to the handler through
  ///   optional second parameter (TransportStream Read does not need to be implemented (it should
  ///   panic since right code will never call it). The loop reception loop should be in Transport
  ///   `receive` implementation.
  //fn receive<C> (&self, &Self::Address, C) where C : Fn(Self::Stream, Option<(Vec<u8>, Option<Attachment>)>) -> ();
  fn start<C> (&self, &SocketAddr, C) -> IoResult<()>  where C : Fn(Self::Stream, Option<(Vec<u8>, Option<Attachment>)>) -> IoResult<()>;

  /// Transport initialisation for sending
  //fn connectwith(&self, &Self::Address, Duration) -> IoResult<Self::Stream>;
  fn connectwith(&self, &SocketAddr, Duration) -> IoResult<Self::Stream>;

}
pub trait Transport_New : Send + Sync + 'static {
  type ReadStream : ReadTransportStream;
  type WriteStream : WriteTransportStream;
  type Address : Send + Clone + Sync;
 
  /// should we spawn a new thread for reception
  /// Defaults to true
  fn do_spawn_rec() -> bool {true}

  /// depending on impl method should send stream to peermgmt as a writestream (for instance tcp socket are
  /// read/write).
  /// D fn will not start every time (only if WriteStream created), and is only to transmit stream
  /// to either peermanager or as query (waiting for auth).
  fn start<C,D> (&self, &Self::Address, C, D) -> IoResult<()>
    where C : Fn(Self::ReadStream) -> IoResult<()>,
          D : Fn(Self::WriteStream) -> IoResult<()>;

  /// Sometimes : for instance with tcp, the writestream is the same as the read stream,
  /// if we expect to use the same (and not open two socket), receive watching process should be
  /// start.
  fn connectwith(&self, &Self::Address, Duration) -> IoResult<(Self::WriteStream, Option<Self::ReadStream>)>;
}


pub trait WriteTransportStream : Send + Sync + Write {
  // most of the time unneeded
  /// simply result in check connectivity false
  fn disconnect(&mut self);
  fn checkconnectivity(&self) -> bool;
}

pub trait ReadTransportStream : Send + Sync + Read {
  
  /// should end read loop
  fn disconnect(&mut self);

  /// check stream connectivity, on disconnected transport
  /// will allways return true
  /// (false would mean something is abnormal and peer may be removed)
  fn checkconnectivity(&self) -> bool;

  /// Receive loop unless RecTermCondition return true
  /// to decide if receive loop on a ReadStream should end.
  /// For instance disconnected transport should allways return true,
  /// when connected receiving tcp stream should loop unless it is unconnected.
  /// Could only be used to run action post receive.
  fn rec_end_condition(&self) -> bool;


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
