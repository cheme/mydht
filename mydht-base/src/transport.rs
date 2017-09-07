#[cfg(feature="mio-impl")]
extern crate coroutine;

use mio::{Poll,Token,Ready,PollOpt};
use std::ops::Deref;
use byteorder::{
  LittleEndian,
  ReadBytesExt,
  WriteBytesExt
};
use std::str::FromStr;
use std::io::Result as IoResult;
use std::result::Result as StdResult;
use std::io::Write;
use std::io::Read;
use std::io::{
  Error as IoError,
  ErrorKind as IoErrorKind,
};

use time::Duration;

use mydhtresult::{
  Result,
  ErrorKind,
  Error,
};
use std::error::Error as ErrorTrait;

use std::thread::JoinHandle;

use serde::{Serialize, Deserialize, Serializer, Deserializer};
use serde::de::{DeserializeOwned};
use std::fmt::Debug;
use std::net::{SocketAddr};
use std::net::Shutdown;
use std::net::{TcpStream};
use mio::net::TcpStream as MioStream;
#[cfg(feature="mio-impl")]
use self::coroutine::Handle as CoHandle;

#[cfg(test)]
use std::io::Cursor;
#[cfg(test)]
use std::net::{
  SocketAddrV4,
  Ipv4Addr,
  SocketAddrV6,
  Ipv6Addr,
};

//use utils::Ref;

/// entry discribing the read or write stream
pub struct SlabEntry<T : Transport, RR, WR, P> {
  /// state for the stream
  pub state : SlabEntryState<T,RR,WR>,
  /// corresponding read or write stream slab index
  pub os : Option<usize>,
  pub peer : Option<P>,
}

pub enum SlabEntryState<T : Transport, RR, WR> 
{
  ReadStream(T::ReadStream),
  WriteStream(T::WriteStream),
  ReadSpawned(RR),
  WriteSpawned(WR),
  Empty,
}


pub trait Address : Eq + Sync + Send + Clone + Debug + Serialize + DeserializeOwned + 'static {
/*  /// for tunnel (otherwhise rust serialize is use on peer)
  fn write_as_bytes<W:Write> (&self, &mut W) -> IoResult<()>;
  /// for tunnel (otherwhise rust serialize is use on peer)
  fn read_as_bytes<R:Read> (&mut R) -> IoResult<Self>;*/
}
/*
impl<A : Sync + Send + Clone + Debug + 'static> Address for A {
  fn write_as_bytes<W:Write> (&self, w : &mut W) -> Result<()> {
    panic!("tt");
  }
  fn read_as_bytes<R:Read> (r : &mut R) -> Result<Self> {
    panic!("tt");
  }
}
*/


impl Serialize for SerSocketAddr {
  fn serialize<S:Serializer> (&self, s: S) -> StdResult<S::Ok, S::Error> {
//    s.emit_str(&self.0.to_string()[..])
     (&self.0.to_string()[..]).serialize(s)
  }
}

impl<'de> Deserialize<'de> for SerSocketAddr {
  fn deserialize<D:Deserializer<'de>> (d : D) -> StdResult<SerSocketAddr, D::Error> {

    let ad = <String>::deserialize(d)?;
    Ok(SerSocketAddr(FromStr::from_str(&ad[..]).unwrap()))
/*    d.read_str().map(|ad| {
      SerSocketAddr(FromStr::from_str(&ad[..]).unwrap())
    })*/
  }
}

impl Deref for SerSocketAddr {
  type Target = SocketAddr;
  fn deref<'a> (&'a self) -> &'a SocketAddr {
    &self.0
  }
}


/// serializable socket address
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SerSocketAddr(pub SocketAddr);


impl Address for SerSocketAddr { 
 /* // TODO replace to to_byte
  fn write_as_bytes<W:Write> (&self, w : &mut W) -> IoResult<()> {
    let sstr = self.0.to_string();
    let stri = sstr.as_bytes();
    let fsize = stri.len() as u64; 

    //tryfor!(BOErr,w.write_u64::<LittleEndian>(fsize));
    try!(w.write_u64::<LittleEndian>(fsize));
    try!(w.write_all(stri));
    Ok(())
  }
  // TODO replace to from_byte
  /// for tunnel (otherwhise rust serialize is use on peer)
  fn read_as_bytes<R:Read> (r : &mut R) -> IoResult<Self> {

    let fsize = try!(r.read_u64::<LittleEndian>());
    let mut addbyte = vec![0;fsize as usize];
    try!(r.read(&mut addbyte[..]));
    let adds = String::from_utf8_lossy(&addbyte[..]);
    match SocketAddr::from_str(&adds) {
      Ok(add) => Ok(SerSocketAddr(add)),
      Err(e) => 
        Err(IoError::new(IoErrorKind::InvalidData, e)),
    }
  }*/
}

/*
#[test]
fn test_addr_socket () {
  let s1 = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127,0,0,1),69914));
  let s2 = SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::new(127,0,0,1,127,0,0,1),69914,0,0));
  let mut cursor = Cursor::new(Vec::new());
  s1.write_as_bytes(&mut cursor);
  cursor.set_position(0);
  assert!(SocketAddr::read_as_bytes (&mut cursor).unwrap() == s1);
  cursor.set_position(0);
  s2.write_as_bytes(&mut cursor);
  cursor.set_position(0);
  assert!(SocketAddr::read_as_bytes (&mut cursor).unwrap() == s2);
}
*/
pub trait Registerable {
  /// registration on main io loop when possible (if not return false)
  fn register(&self, &Poll, Token, Ready, PollOpt) -> Result<bool>;
  /// async reregister
  fn reregister(&self, &Poll, Token, Ready, PollOpt) -> Result<bool>;
 
}
/// transport must be sync (in running type), it implies that it is badly named as transport must
/// only contain enough information to instantiate needed component (even not sync) in the start
/// method (plus connect with required info). Some sync may still be needed for connect (sync with
/// instanciated component in start).
pub trait Transport : Send + Sync + 'static + Registerable {
  type ReadStream : ReadTransportStream;
  type WriteStream : WriteTransportStream;
  type Address : Address;
 
  /// TODO remove (loop spawner are used : replace by spawner type)
  fn do_spawn_rec(&self) -> SpawnRecMode {SpawnRecMode::Threaded}
  

  /// depending on impl method should send stream to peermgmt as a writestream (for instance tcp socket are
  /// read/write).
  /// D fn will not start every time (only if WriteStream created), and is only to transmit stream
  /// to either peermanager or as query (waiting for auth).
  /// TODO remove
  fn start<C> (&self, C) -> Result<()>
    where C : Fn(Self::ReadStream,Option<Self::WriteStream>) -> Result<ReaderHandle>;

  /// transport listen for incomming connection 
  ///
  /// TODO remove address (not related to peer address
  /// and accessible by transport handle
  fn accept(&self) -> Result<(Self::ReadStream, Option<Self::WriteStream>, Self::Address)>;

  /// Sometimes : for instance with tcp, the writestream is the same as the read stream,
  /// if we expect to use the same (and not open two socket), receive watching process should be
  /// start.
  ///
  /// TODO remove duration timeout (not for all transport)
  fn connectwith(&self, &Self::Address, Duration) -> IoResult<(Self::WriteStream, Option<Self::ReadStream>)>;

  /// Disconnect an active connection in case we have a no spawn transport model (eg deregister a
  /// socket on an event_loop).
  /// Return false if spawn rec (then closing conn is done by ending the thread from the
  /// peermanager).
  fn disconnect(&self, &Self::Address) -> IoResult<bool> {Ok(false)}
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
  Coroutine, // TODO remove (issue with pattern matching)
}


// TODO add clone constraint with possibility to panic!("clone is not allowed : local send
// threads are not possible")
pub trait WriteTransportStream : Send + Write + 'static + Registerable {
  // most of the time unneeded
  /// simply result in check connectivity false
  fn disconnect(&mut self) -> IoResult<()>;
//  fn checkconnectivity(&self) -> bool;
}

pub trait ReadTransportStream : Send + Read + 'static + Registerable {



  /// should end read loop
  fn disconnect(&mut self) -> IoResult<()>;

  // check stream connectivity, on disconnected transport
  // will allways return true
  // (false would mean something is abnormal and peer may be removed)
 // fn checkconnectivity(&self) -> bool;

  /// TODO remove
  fn rec_end_condition(&self) -> bool;


  /// TODO remove
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
impl Registerable for TcpStream {

  fn register(&self, _ : &Poll, _ : Token, _ : Ready, _ : PollOpt) -> Result<bool> {
    Ok(false)
  }
  fn reregister(&self, _ : &Poll, _ : Token, _ : Ready, _ : PollOpt) -> Result<bool> {
    Ok(false)
  }
}

impl WriteTransportStream for TcpStream {
  fn disconnect(&mut self) -> IoResult<()> {
    self.shutdown(Shutdown::Write)
  }
}
impl ReadTransportStream for TcpStream {
  fn disconnect(&mut self) -> IoResult<()> {
    self.shutdown(Shutdown::Read)
  }
  /// TODO remove
  fn rec_end_condition(&self) -> bool {
    false
  }
}
impl Registerable for MioStream {

  fn register(&self, p : &Poll, t : Token, r : Ready, po : PollOpt) -> Result<bool> {
    p.register(self, t, r, po)?;
    Ok(true)
  }
  fn reregister(&self, p : &Poll, t : Token, r : Ready, po : PollOpt) -> Result<bool> {
    p.reregister(self, t, r, po)?;
    Ok(true)
  }
}


impl WriteTransportStream for MioStream {
  fn disconnect(&mut self) -> IoResult<()> {
    self.shutdown(Shutdown::Write)
  }
}
impl ReadTransportStream for MioStream {
  fn disconnect(&mut self) -> IoResult<()> {
    self.shutdown(Shutdown::Read)
  }
  /// TODO remove
  fn rec_end_condition(&self) -> bool {
    false
  }
}

