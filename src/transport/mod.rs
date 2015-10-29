use std::io::Result as IoResult;
use std::io::Write;
use std::io::Read;
use time::Duration;

use std::net::{SocketAddr};
use std::path::PathBuf;
use std::fmt::Debug;
use mydhtresult::Result; 
use std::thread::JoinHandle;

#[cfg(feature="mio-impl")]
use coroutine::Handle as CoHandle;
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

#[cfg(feature="mio-impl")]
/// possible handle when starting a server process to read
pub enum ReaderHandle {
  Local, // Nothing, run locally
  LocalTh(JoinHandle<()>), // run locally but in a thread
  Thread(JoinHandle<()>),
  Coroutine(CoHandle),
  // Scoped // TODO?
}

#[cfg(not(feature="mio-impl"))]
/// possible handle when starting a server process to read
pub enum ReaderHandle {
  Local, // Nothing, run locally
  LocalTh(JoinHandle<()>), // run locally but in a thread
  Thread(JoinHandle<()>),
  // Scoped // TODO?
}

#[cfg(feature="mio-impl")]
pub enum SpawnRecMode {
  Local,
  LocalSpawn, // local with spawn
  Threaded,
  Coroutine,
}

#[cfg(not(feature="mio-impl"))]
pub enum SpawnRecMode {
  Local,
  LocalSpawn, // local with spawn
  Threaded,
  Coroutine,
}

/// transport must be sync (in running type), it implies that it is badly named as transport must
/// only contain enough information to instantiate needed component (even not sync) in the start
/// method (plus connect with required info). Some sync may still be needed for connect (sync with
/// instanciated component in start).
pub trait Transport : Send + Sync + 'static {
  type ReadStream : ReadTransportStream;
  type WriteStream : WriteTransportStream;
  type Address : Address;
 
  /// should we spawn a new thread for reception
  /// default to threaded mode
  fn do_spawn_rec(&self) -> SpawnRecMode {SpawnRecMode::Threaded}
 

  /// depending on impl method should send stream to peermgmt as a writestream (for instance tcp socket are
  /// read/write).
  /// D fn will not start every time (only if WriteStream created), and is only to transmit stream
  /// to either peermanager or as query (waiting for auth).
  fn start<C> (&self, C) -> Result<()>
    where C : Fn(Self::ReadStream,Option<Self::WriteStream>) -> Result<ReaderHandle>;

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


  // TODO for asymmetric coroutine usage the ref to the coroutine is needed and
  // therefore a method like. Do not use associated type as call to completeInit
  // will only be done dependantly on ReaderHandle (limitted nb of option,
  // but could use an enum like ThContext).
  // fn completeInit(&mut self, Option<CoroutineRef>);

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
