
//! Tcp transport. Connected transport (tcp), with basic support for attachment (like all connected
//! transport).
//! Some overhead to send size of frame.
//! TODO tcp mode where we open socket for every new transaction (no persistence in peermgmt of
//! socket (only server thread)) : result in no shared and running server thread using direct client
//! handle
//! TODO bidirect tcp option : we do not multiplex frame : send on a port and receive on another
//! one. (just dont return ows or ors).
#[macro_use] extern crate log;
extern crate byteorder;
extern crate mydht_base;
//use self::byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::net::{TcpListener};
use std::net::{TcpStream};
use std::io::Result as IoResult;
use mydht_base::mydhtresult::Result;
//use std::io::Error as IoError;
//use std::io::ErrorKind as IoErrorKind;
use std::net::{SocketAddr};
//use std::io::Write;
//use std::io::Read;
use std::time::Duration;
//use std::thread::Thread;
//use peer::{Peer};
//use mydht_base::transport::{ReadTransportStream,WriteTransportStream};
use mydht_base::transport::{
  Transport,
  SerSocketAddr,
  Registerable,
  Token,
  Ready,
};
//use std::iter;
//use utils;
//use super::Attachment;
//use std::net::Shutdown;




/// Tcp struct : two options, timeout for connect and time out when connected.
pub struct Tcp {
  /// currently use for keep_alive, read and write timeout (api unstable so we do not care yet for
  /// distinction). When stable should become Option<Duration>.
  /// Currently only seconds are used (conversion over seconds!! waiting for api stabilize).
  streamtimeout : Duration,
  /// either we reuse tcp socket for input / output or open another one (two unidirectional socket
  /// being useless for tcp but still good for test/diagnostics or some special firewal settings
  mult : bool,
  listener : TcpListener,
}

impl Tcp {
  pub fn new(p: &SocketAddr, streamtimeout : Duration, mult : bool) -> IoResult<Tcp> {
    let listener = try!(TcpListener::bind(p));
    Ok(Tcp {
      streamtimeout : streamtimeout,
      mult : mult,
      listener : listener,
    })

  }
}
impl<PO> Registerable<PO> for Tcp {
  fn register(&self, _ : &PO, _ : Token, _ : Ready) -> Result<bool> {
    Ok(false)
  }
  fn reregister(&self, _ : &PO, _ : Token, _ : Ready) -> Result<bool> {
    Ok(false)
  }

  fn deregister(&self, _poll: &PO) -> Result<()> {
    Ok(())
  }
}
impl<PO> Transport<PO> for Tcp {
  type ReadStream = TcpStream;
  type WriteStream = TcpStream;
  type Address = SerSocketAddr;
 
  fn accept(&self) -> Result<(Self::ReadStream, Option<Self::WriteStream>)> {
    let (s,ad) = self.listener.accept()?;
    debug!("Initiating socket exchange : ");
    debug!("  - From {:?}", s.local_addr());
    debug!("  - With {:?}", s.peer_addr());
    debug!("  - At {:?}", ad);
    try!(s.set_read_timeout(Some(self.streamtimeout)));
    try!(s.set_write_timeout(Some(self.streamtimeout)));
    if self.mult {
//            try!(s.set_keepalive (self.streamtimeout.num_seconds().to_u32()));
        let rs = try!(s.try_clone());
        Ok((s,Some(rs)))
    } else {
        Ok((s,None))
    }
  }

  fn connectwith(&self,  p : &SerSocketAddr) -> IoResult<(Self::WriteStream, Option<Self::ReadStream>)> {
    //let s = TcpStream::connect_timeout(p, self.connecttimeout);
    let s = try!(TcpStream::connect(p.0));
//    try!(s.set_keepalive (self.streamtimeout.num_seconds().to_u32()));
    if self.mult {
      let rs = try!(s.try_clone());
      Ok((s,Some(rs)))
    } else {
      Ok((s,None))
    }
  }
}

