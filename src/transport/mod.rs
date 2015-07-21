use std::io::Result as IoResult;
use std::io::Write;
use std::io::Read;
use time::Duration;
use peer::{Peer};
use std::net::{SocketAddr};
use std::path::PathBuf;
use std::fmt::Debug;
use mydhtresult::Result; 


#[cfg(feature="mio-impl")]
pub mod tcp_loop;
pub mod tcp;
pub mod udp;
pub type Attachment = PathBuf;

/// TODO Dummy TRansport imple for testing (currently tcp everywhere)

/// Transport trait
/// TODO a synch primitive to check if start in ok state (if needed (see tcp_loop where init
/// eventloop)).
pub trait Transport_old : Send + Sync + 'static {

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

pub trait Address : Sync + Send + Clone + Debug {}

impl Address for SocketAddr {}

pub trait Transport : Send + Sync {
  type ReadStream : ReadTransportStream;
  type WriteStream : WriteTransportStream;
  type Address : Address;
 
  /// should we spawn a new thread for reception
  /// Defaults to true
  /// First returned bool is true if spawn false if not
  /// Second returned bool is true if spawn process need to be managed (likely to loop)
  fn do_spawn_rec(&self) -> (bool,bool) {(true,true)}
 

  /// depending on impl method should send stream to peermgmt as a writestream (for instance tcp socket are
  /// read/write).
  /// D fn will not start every time (only if WriteStream created), and is only to transmit stream
  /// to either peermanager or as query (waiting for auth).
  /// TODO Remove second parameter (bind address should be in initialization of transport(see udp))
  fn start<C> (&self, &Self::Address, C) -> IoResult<()>
    where C : Fn(Self::ReadStream,Option<Self::WriteStream>) -> IoResult<()>;

  /// Sometimes : for instance with tcp, the writestream is the same as the read stream,
  /// if we expect to use the same (and not open two socket), receive watching process should be
  /// start.
  fn connectwith(&self, &Self::Address, Duration) -> IoResult<(Self::WriteStream, Option<Self::ReadStream>)>;

  /// Disconnect an active connection in case we have a no spawn transport model (eg deregister a
  /// socket on an event_loop).
  /// Return false if spawn rec (then closing conn is done by ending the thread from the
  /// peermanager).
  fn disconnect(&self, &Self::Address) -> IoResult<bool> {Ok(false)}
}


pub trait WriteTransportStream : Send + Sync + Write {
  // most of the time unneeded
  /// simply result in check connectivity false
  fn disconnect(&mut self) -> IoResult<()>;
//  fn checkconnectivity(&self) -> bool;
}

pub trait ReadTransportStream : Send + Sync + Read {
  
  /// should end read loop
  fn disconnect(&mut self) -> IoResult<()>;

  // check stream connectivity, on disconnected transport
  // will allways return true
  // (false would mean something is abnormal and peer may be removed)
 // fn checkconnectivity(&self) -> bool;

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
