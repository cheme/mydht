//! Tcp transport. Connected transport (tcp), with basic support for attachment (like all connected
//! transport).
//! Some overhead to send size of frame.
//! TODO tcp mode where we open socket for every new transaction (no persistence in peermgmt of
//! socket (only server thread)) : result in no shared and running server thread using direct client
//! handle
//! TODO bidirect tcp option : we do not multiplex frame : send on a port and receive on another
//! one. (just dont return ows or ors).
extern crate byteorder;
use self::byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::net::{TcpListener};
use std::net::{TcpStream};
use std::io::Result as IoResult;
use mydhtresult::Result;
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
  streamtimeout : Duration,
  /// courrently not used
  connecttimeout : Duration,
  /// either we reuse tcp socket for input / output or open another one (two unidirectional socket
  /// being useless for tcp but still good for test/diagnostics or some special firewal settings
  mult : bool,
  listener : TcpListener,
}

impl Tcp {
  pub fn new(  p: &SocketAddr, streamtimeout : Duration, connecttimeout : Duration, mult : bool) -> IoResult<Tcp> {
    let listener = try!(TcpListener::bind(p));
    Ok(Tcp {
      streamtimeout : streamtimeout,
      connecttimeout : connecttimeout,
      mult : mult,
      listener : listener,
    })

  }
}

impl Transport for Tcp {
  type ReadStream = TcpStream;
  type WriteStream = TcpStream;
  type Address = SocketAddr;
  fn start<C> (&self, readHandler : C) -> Result<()>
    where C : Fn(Self::ReadStream,Option<Self::WriteStream>) -> Result<()> {
    for socket in self.listener.incoming() {
        match socket {
            Err(e) => {error!("Socket acceptor error : {:?}", e);}
            Ok(mut s)  => {
              debug!("Initiating socket exchange : ");
              debug!("  - From {:?}", s.local_addr());
              debug!("  - From {:?}", s.peer_addr());
              debug!("  - With {:?}", s.peer_addr());
              if self.mult {
                s.set_keepalive (self.streamtimeout.num_seconds().to_u32());
                s.set_read_timeout(self.streamtimeout.num_seconds().to_u64().map(StdDuration::from_secs));
                s.set_write_timeout(self.streamtimeout.num_seconds().to_u64().map(StdDuration::from_secs));

                let rs = try!(s.try_clone());
                readHandler(rs,Some(s));
              } else {
                readHandler(s,None);
              }
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
    if self.mult {
      let rs = try!(s.try_clone());
      Ok((s,Some(rs)))
    } else {
      Ok((s,None))
    }
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

