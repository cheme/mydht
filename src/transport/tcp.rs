//! Tcp transport. Connected transport (tcp), with basic support for attachment (like all connected
//! transport).
//! Some overhead to send size of frame.
//! TODO tcp mode where we open socket for every new transaction (no persistence in peermgmt of
//! socket (only server thread)) : result in no shared and running server thread using direct client
//! handle
extern crate byteorder;
use self::byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::net::{TcpListener};
use std::net::{TcpStream};
use std::io::Result as IoResult;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::net::{SocketAddr};
use std::io::Write;
use std::io::Read;
use time::Duration;
use std::time::Duration as StdDuration;
use std::thread::Thread;
use peer::{Peer};
use super::{Transport,ReadTransportStream,WriteTransportStream};
use std::iter;
use utils;
use num::traits::ToPrimitive;
use super::Attachment;
use std::net::Shutdown;

const BUFF_SIZE : usize = 10000; // use for attachment send/receive -- 21888 seems to be maxsize
const MAX_BUFF_SIZE : usize = 21888; // 21888 seems to be maxsize

/// Tcp struct : two options, timeout for connect and time out when connected.
pub struct Tcp {
  /// currently use for keep_alive, read and write timeout (api unstable so we do not care yet for
  /// distinction). When stable should become Option<Duration>.
  /// Currently only seconds are used (conversion over seconds!! waiting for api stabilize).
  pub streamtimeout : Duration,
  /// courrently not used
  pub connecttimeout : Duration,
}

impl Transport for Tcp {
  type ReadStream = TcpStream;
  type WriteStream = TcpStream;
  type Address = SocketAddr;
  fn start<C> (&self, p: &SocketAddr, readHandler : C) -> IoResult<()>
    where C : Fn(Self::ReadStream,Option<Self::WriteStream>) -> IoResult<()> {
    let mut listener = TcpListener::bind(p).unwrap();
    for socket in listener.incoming() {
        match socket {
            Err(e) => {error!("Socket acceptor error : {:?}", e);}
            Ok(mut s)  => {
              debug!("Initiating socket exchange : ");
              debug!("  - From {:?}", s.local_addr());
              debug!("  - From {:?}", s.peer_addr());
              debug!("  - With {:?}", s.peer_addr());
              s.set_keepalive (self.streamtimeout.num_seconds().to_u32());
              s.set_read_timeout(self.streamtimeout.num_seconds().to_u64().map(StdDuration::from_secs));
              s.set_write_timeout(self.streamtimeout.num_seconds().to_u64().map(StdDuration::from_secs));
              let rs = try!(s.try_clone());
              readHandler(rs,Some(s));
            }
        }
    };
    Ok(())
  }
  fn connectwith(&self,  p : &SocketAddr, timeout : Duration) -> IoResult<(Self::WriteStream, Option<Self::ReadStream>)> {
    // connect TODO new api timeout
    //let s = TcpStream::connect_timeout(p, self.connecttimeout);
    let s = try!(TcpStream::connect(p));
    try!(s.set_keepalive (self.streamtimeout.num_seconds().to_u32()));
    let rs = try!(s.try_clone());
    Ok((s,Some(rs)))
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
  /// this tcp runs in a separated thread and need to stop only depending on server loop
  /// implementation (timeout error, error
  fn rec_end_condition(&self) -> bool {
    false
  }
}

