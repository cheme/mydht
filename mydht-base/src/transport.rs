//use mio::{Poll,Token,Ready,PollOpt};
use std::ops::Deref;
use std::str::FromStr;
use std::io::Result as IoResult;
use std::result::Result as StdResult;
use std::io::Write;
use std::io::Read;
use std::time::Duration;
pub use service_pre::transport::{
  ReadYield,
  WriteYield,
};
pub use service_pre::eventloop::{
  Registerable,
  Poll,
  Token,
  Ready,
  Events,
  Event,
  TriggerReady,
};
pub use service_pre::error::{
  Error as LoopError,
  Result as LoopResult,
  ErrorKind as LoopErrorKind,
};

#[cfg(feature = "mio-transport")]
extern crate mio;

use mydhtresult::{
  Result,
};

use serde::{
  Serialize,
  Deserialize,
  Serializer,
  Deserializer,
};
use serde::de::DeserializeOwned;
use std::fmt::Debug;
#[cfg(any(feature = "blocking-transport", feature = "mio-transport"))]
use std::net::{SocketAddr, Shutdown};
#[cfg(feature = "blocking-transport")]
use std::net::TcpStream;
#[cfg(feature = "mio-transport")]
use self::mio::net::TcpStream as MioStream;

/// entry discribing the read or write stream
pub struct SlabEntry<PO, T: Transport<PO>, RR, WR, WB, RP> {
  /// state for the stream
  pub state: SlabEntryState<PO, T, RR, WR, WB, RP>,
  /// corresponding read or write stream slab index
  pub os: Option<usize>,
  pub peer: Option<RP>,
}

pub enum SlabEntryState<PO, T: Transport<PO>, RR, WR, WB, P> {
  /// RP is dest peer reference
  ReadStream(T::ReadStream, Option<P>),
  /// WB is a buffer to use while stream is unconnected
  WriteStream(T::WriteStream, WB),
  /// optional p is trusted peer ref if no auth
  WriteConnectSynch(WB, Option<P>),
  ReadSpawned(RR),
  WriteSpawned(WR),
  Empty,
}

pub trait Address: Eq + Send + Clone + Debug + Serialize + DeserializeOwned + 'static {
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

#[cfg(any(feature = "blocking-transport", feature = "mio-transport"))]
impl Serialize for SerSocketAddr {
  fn serialize<S: Serializer>(&self, s: S) -> StdResult<S::Ok, S::Error> {
    //    s.emit_str(&self.0.to_string()[..])
    (&self.0.to_string()[..]).serialize(s)
  }
}

#[cfg(any(feature = "blocking-transport", feature = "mio-transport"))]
impl<'de> Deserialize<'de> for SerSocketAddr {
  fn deserialize<D: Deserializer<'de>>(d: D) -> StdResult<SerSocketAddr, D::Error> {
    let ad = <String>::deserialize(d)?;
    Ok(SerSocketAddr(FromStr::from_str(&ad[..]).unwrap()))
    /*    d.read_str().map(|ad| {
      SerSocketAddr(FromStr::from_str(&ad[..]).unwrap())
    })*/
  }
}

#[cfg(any(feature = "blocking-transport", feature = "mio-transport"))]
impl Deref for SerSocketAddr {
  type Target = SocketAddr;
  fn deref<'a>(&'a self) -> &'a SocketAddr {
    &self.0
  }
}

#[cfg(any(feature = "blocking-transport", feature = "mio-transport"))]
/// serializable socket address
#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct SerSocketAddr(pub SocketAddr);

#[cfg(any(feature = "blocking-transport", feature = "mio-transport"))]
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

/// transport must be sync (in running type), it implies that it is badly named as transport must
/// only contain enough information to instantiate needed component (even not sync) in the start
/// method (plus connect with required info). Some sync may still be needed for connect (sync with
/// instanciated component in start).
pub trait Transport<PO>: 'static + Registerable<PO> {
  type ReadStream: ReadTransportStream + Registerable<PO>;
  type WriteStream: WriteTransportStream + Registerable<PO>;
  type Address: Address;

  /// transport listen for incomming connection
  fn accept(&self) -> Result<(Self::ReadStream, Option<Self::WriteStream>)>;

  /// Sometimes : for instance with tcp, the writestream is the same as the read stream,
  /// if we expect to use the same (and not open two socket), receive watching process should be
  /// start.
  fn connectwith(&self, &Self::Address) -> IoResult<(Self::WriteStream, Option<Self::ReadStream>)>;

  /// Disconnect an active connection in case we have a no spawn transport model (eg deregister a
  /// socket on an event_loop).
  /// Return false if spawn rec (then closing conn is done by ending the thread from the
  /// peermanager).
  fn disconnect(&self, &Self::Address) -> IoResult<bool> {
    Ok(false)
  }
}

// TODO add clone constraint with possibility to panic!("clone is not allowed : local send
// threads are not possible")
pub trait WriteTransportStream: Write + 'static {
  // most of the time unneeded
  /// simply result in check connectivity false
  fn disconnect(&mut self) -> IoResult<()>;
  //  fn checkconnectivity(&self) -> bool;
}

pub trait ReadTransportStream: Read + 'static {
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
  fn end_read_msg(&mut self) -> () {
    ()
  }

  // TODO for asymmetric coroutine usage the ref to the coroutine is needed and
  // therefore a method like. Do not use associated type as call to completeInit
  // will only be done dependantly on ReaderHandle (limitted nb of option,
  // but could use an enum like ThContext).
  // fn completeInit(&mut self, Option<CoroutineRef>);
}

#[cfg(feature = "blocking-transport")]
impl WriteTransportStream for TcpStream {
  fn disconnect(&mut self) -> IoResult<()> {
    self.shutdown(Shutdown::Write)
  }
}
#[cfg(feature = "blocking-transport")]
impl ReadTransportStream for TcpStream {
  fn disconnect(&mut self) -> IoResult<()> {
    self.shutdown(Shutdown::Read)
  }
  /// TODO remove
  fn rec_end_condition(&self) -> bool {
    false
  }
}

#[cfg(feature = "mio-transport")]
impl WriteTransportStream for MioStream {
  fn disconnect(&mut self) -> IoResult<()> {
    self.shutdown(Shutdown::Write)
  }
}
#[cfg(feature = "mio-transport")]
impl ReadTransportStream for MioStream {
  fn disconnect(&mut self) -> IoResult<()> {
    self.shutdown(Shutdown::Read)
  }
  /// TODO remove
  fn rec_end_condition(&self) -> bool {
    false
  }
}
