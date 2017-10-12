//! 
//!
//! TODO this implementation of udp stream totally broke new transport design it may run in unauth
//! mode as we may send all at once for a message and receive without origin info, mostly useless :
//! previous dht associated transport with its address so new transport may run, this new design
//! does not (in unauth it should equival transport add and peer key). For new design using slab
//! token may be resonable even if not secure at all (start 0 is ping, start usize other is peer
//! read token -> bad design local transport token is the only design : thread synch is bad then
//! plus two variant : none thread sync can be a use case).
//! Routing by origin address is only safe design : then send to reader (a thread or not safe
//! buffer)
//!
//!
//! disconnected udp proto
//! In this model only one thread read (on receive connection)
//! ping is not use to establish connection (in disconnected when receiving ping user is online and
//! we send back ping if we didn't know him (eg find_value on ourself for bt proto)
//! Do not allow attachment (or at least not big attachment).
//!
//! Message is send/receive as a whole and they should be small enough.
//!
//! TODO A mode to manage bigger message (in more than one buff) could be added, it requires
//! synchronisation : ensure that not two sender to same peer send on the socket.
//! That should hold (only one sender with a peer as design). But we would need a reader adapter on
//! stream reception (reader on some channel same kind of synch as for tcp_loop but with a reader
//! adapter). TODO this should be another transport since reader should be connected (could loop on
//! it (default do_spawn_rec)).
//!
//! Currently no support of shadower and race condition on thread, as we need some persistence
//! locally in the transport : TODO implement it
//!



#[macro_use] extern crate log;
extern crate byteorder;
extern crate mydht_base;
extern crate time;
extern crate num;
extern crate mio;

use mio::{Poll,Token,Ready,PollOpt};
use mydht_base::transport::{
  Transport,
  ReadTransportStream,
  WriteTransportStream,
  SerSocketAddr,
  Registerable,
};
//use super::{Attachment};
use std::io::Result as IoResult;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::net::SocketAddr;

use mydht_base::mydhtresult::{Result,Error,ErrorKind};
//use peer::Peer;
//use std::iter;
use std::slice;
use std::net::UdpSocket;
use std::io::Write;
use std::io::Read;
//use std::collections::VecDeque;

// TODO retest after switch to new io

pub struct Udp {
  // TODO there need to be a way to avoid this Mutex!!! (especially since we read at a single
  // location TODO even consider unsafe solutions
  /// actual socket for reception
  sock : UdpSocket,  // reference socket the only one to do receive
  /// size of buff for msg reception (and maxsize for sending to) 
  buffsize : usize,
}

impl Udp {
  /// warning bind on initialize
  pub fn new (p : &SocketAddr, s : usize) -> IoResult<Udp> {
    let socket = try!(UdpSocket::bind(p));
    Ok(Udp {
      sock : socket,
      buffsize : s,
    })
  }
}

//struct ReadUdpStreamVariant (VecDeque<u8>);
pub struct ReadUdpStream (Vec<u8>);

pub struct UdpStream {
  sock : UdpSocket,  // we clone old io but streamreceive is not allowed
  with : SerSocketAddr, // old io could be clone , with new io manage protection ourselve
  //if define we can send overwhise it is send in server : panic!
  buf : Vec<u8>,
  maxsize : usize,
}

// TODO set buff in  stream for attachment, large frame... (requires ordering header...)
impl Write for UdpStream {
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
      if self.buf.len() + buf.len() > self.maxsize {
        Err(IoError::new (
          IoErrorKind::Other,
          "Udp writer buffer full",
        ))
      } else {
        self.buf.write(buf)
      }
    }
    fn flush(&mut self) -> IoResult<()> {
      try!(self.sock.send_to(&self.buf[..], self.with.0));
      self.buf = Vec::new();
      Ok(())
    }
}

/*impl Read for ReadUdpStreamVariant {
  fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
    let l = buf.len();
    if l > 0 {
      if l > self.0.len() {
        let r = self.0.len();
        {
          let (start,end) = self.0.as_slices();
          copy_memory(start, buf);
          copy_memory(end, &mut buf[start.len()..]);
        }
        self.0.clear();
        Ok(r)
      } else {
        {
          let (start,end) = self.0.as_slices();
          if l > start.len() {
            let sl = start.len();
            copy_memory(start, buf);
            copy_memory(&end[..(l - sl)], &mut buf[sl..]);
          } else {
            copy_memory(&start[..l], buf);
          };
        }
        self.0 = self.0.split_off(l);
        Ok(l)
      }
    } else {
      Ok(0)
    }
 
  }
}*/
impl Read for ReadUdpStream {
  fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
    let l = buf.len();
    if l > 0 {
      if l > self.0.len() {
        let r = self.0.len();
        buf[..r].copy_from_slice(&self.0[..]);
        self.0.clear();
        Ok(r)
      } else {
        buf.copy_from_slice(&self.0[..l]);
        self.0 = (&self.0[l..]).to_vec();
        Ok(l)
      }   
    } else {
      Ok(0)
    }
  }
}
impl Registerable for Udp {
  fn register(&self, _ : &Poll, _: Token, _ : Ready, _ : PollOpt) -> Result<bool> {
    Ok(false)
  }
  fn reregister(&self, _ : &Poll, _: Token, _ : Ready, _ : PollOpt) -> Result<bool> {
    Ok(false)
  }
 
}
impl Transport for Udp {
  type ReadStream = ReadUdpStream;
  type WriteStream = UdpStream;
  type Address = SerSocketAddr;
  

  fn accept(&self) -> Result<(Self::ReadStream, Option<Self::WriteStream>)> {
    let mut tmpvec : Vec<u8> = vec![0; self.buffsize];
    let buf = tmpvec.as_mut_slice();
    let (size,_ad) = self.sock.recv_from(buf)?;
    if size < self.buffsize {
      // TODO test safe approach
      let r = unsafe {
        slice::from_raw_parts(buf.as_ptr(), size).to_vec()
      };
      Ok((ReadUdpStream(r), None))
    } else {
      error!("Datagram on udp transport with size {:?} over buff {:?}, lost datagram", size, self.buffsize);
      Err(Error("Udp Oversized datagram".to_string(), ErrorKind::ExpectedError, None))
    }
   
  }


  /// does not return a read handle as udp is unconnected (variant with read buf synch would need to
  /// be returned).
  /// Function never fail as udp is unconnected (write will fail!!)
  fn connectwith(&self, p : &SerSocketAddr) -> IoResult<(UdpStream, Option<ReadUdpStream>)> {
    let readso = try!(self.sock.try_clone());
    // get socket (non connected cannot timeout)
    Ok((UdpStream {
      sock : readso,
      with : p.clone(),
      buf: Vec::new(),
      maxsize : self.buffsize,
    },None))
  }

}


impl Registerable for UdpStream {
  fn register(&self, _ : &Poll, _ : Token, _ : Ready, _ : PollOpt) -> Result<bool> {
    Ok(false)
  }
  fn reregister(&self, _ : &Poll, _ : Token, _ : Ready, _ : PollOpt) -> Result<bool> {
    Ok(false)
  }
}
impl Registerable for ReadUdpStream {
  fn register(&self, _ : &Poll, _ : Token, _ : Ready, _ : PollOpt) -> Result<bool> {
    Ok(false)
  }
  fn reregister(&self, _ : &Poll, _ : Token, _ : Ready, _ : PollOpt) -> Result<bool> {
    Ok(false)
  }
}


/// Nothing is really done since udp is disconnected
impl WriteTransportStream for UdpStream {
  fn disconnect(&mut self) -> IoResult<()> {
    Ok(())
  }
}

/// Nothing is really done since udp is disconnected
impl ReadTransportStream for ReadUdpStream {
  fn disconnect(&mut self) -> IoResult<()> {
    Ok(())
  }
  fn rec_end_condition(&self) -> bool {
    true
  }
}



