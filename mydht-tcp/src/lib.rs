
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
extern crate time;
extern crate num;
use num::traits::ToPrimitive;
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
use time::Duration;
use std::time::Duration as StdDuration;
//use std::thread::Thread;
//use peer::{Peer};
//use mydht_base::transport::{ReadTransportStream,WriteTransportStream};
use mydht_base::transport::{Transport,ReaderHandle,SerSocketAddr};
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

impl Transport for Tcp {
  type ReadStream = TcpStream;
  type WriteStream = TcpStream;
  type Address = SerSocketAddr;
  fn start<C> (&self, readhandler : C) -> Result<()>
    where C : Fn(Self::ReadStream,Option<Self::WriteStream>) -> Result<ReaderHandle> {
    for socket in self.listener.incoming() {
      match socket {
        Err(e) => {error!("Socket acceptor error : {:?}", e);}
        Ok(s)  => {
          debug!("Initiating socket exchange : ");
          debug!("  - From {:?}", s.local_addr());
          debug!("  - From {:?}", s.peer_addr());
          debug!("  - With {:?}", s.peer_addr());
          if self.mult {
//            try!(s.set_keepalive (self.streamtimeout.num_seconds().to_u32()));
            try!(s.set_read_timeout(self.streamtimeout.num_seconds().to_u64().map(StdDuration::from_secs)));
            try!(s.set_write_timeout(self.streamtimeout.num_seconds().to_u64().map(StdDuration::from_secs)));

            let rs = try!(s.try_clone());
            match readhandler(rs,Some(s)) {
              Ok(_) => (),
              Err(e) => error!("Read handler failure : {}",e),
            }
          } else {
            match readhandler(s,None) {
              Ok(_) => (),
              Err(e) => error!("Read handler failure : {}",e),
            }
          }
        }
      }
    };
    Ok(())
  }
  fn connectwith(&self,  p : &SerSocketAddr, _ : Duration) -> IoResult<(Self::WriteStream, Option<Self::ReadStream>)> {
    // connect TODO new api timeout (third param)
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
