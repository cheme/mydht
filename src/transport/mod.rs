use std::io::Result as IoResult;
use std::io::Write;
use std::io::Read;
use time::Duration;
use peer::{Peer};
use std::net::{SocketAddr};
use std::path::PathBuf;
use std::fmt::Debug;
use mydhtresult::Result; 
use utils::OneResult;

#[cfg(feature="mio-impl")]
pub mod tcp_loop;
pub mod tcp;
pub mod udp;
pub type Attachment = PathBuf;
#[cfg(test)]
pub mod local_transport;
#[cfg(test)]
pub mod test;




pub trait Address : Sync + Send + Clone + Debug + 'static {}

impl Address for SocketAddr {}

/// for testing purpose
#[derive(RustcDecodable,RustcEncodable,Debug,PartialEq,Eq,Clone)]
pub struct LocalAdd (pub usize);
impl Address for LocalAdd{}



/// transport must be sync (in running type), it implies that it is badly named as transport must
/// only contain enough information to instantiate needed component (even not sync) in the start
/// method (plus connect with required info). Some sync may still be needed for connect (sync with
/// instanciated component in start).
pub trait Transport : Send + Sync + 'static {
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
  fn start<C> (&self, C) -> Result<()>
    where C : Fn(Self::ReadStream,Option<Self::WriteStream>) -> Result<()>;

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

// TODO add clone constraint with possibility to panic!("clone is not allowed : local send
// threads are not possible")
pub trait WriteTransportStream : Send + Write + 'static {
  // most of the time unneeded
  /// simply result in check connectivity false
  fn disconnect(&mut self) -> IoResult<()>;
//  fn checkconnectivity(&self) -> bool;
}

pub trait ReadTransportStream : Send + Read + 'static {
  
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


  /// Call after each msg successfull reading : usefull for some transport
  /// (non managed so we can start another server fn (if first was spawn))
  #[inline]
  fn end_read_msg(&mut self) -> () {()}

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
