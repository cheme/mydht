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
//!

use super::{Transport,ReadTransportStream,WriteTransportStream};
use super::{Attachment};
use std::io::Result as IoResult;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::net::SocketAddr;
use time::Duration;
use peer::Peer;
use std::iter;
use std::slice;
use std::net::UdpSocket;
use std::io::Write;
use std::io::Read;
use std::collections::VecDeque;
use std::slice::bytes::copy_memory;

// TODO retest after switch to new io

pub struct Udp {
  // TODO there need to be a way to avoid this Mutex!!! (especially since we read at a single
  // location TODO even consider unsafe solutions
  /// actual socket for reception
  sock : UdpSocket,  // reference socket the only one to do receive
  /// size of buff for msg reception (and maxsize for sending to) 
  buffsize : usize,
  spawn : bool,
}

impl Udp {
  /// warning bind on initialize
  pub fn new (p : &SocketAddr, s : usize, spawn : bool) -> IoResult<Udp> {
    let mut socket = try!(UdpSocket::bind(p));
    Ok(Udp {
      sock : socket,
      buffsize : s,
      spawn : spawn,
    })
  }
}

struct ReadUdpStreamVariant (VecDeque<u8>);
struct ReadUdpStream (Vec<u8>);

struct UdpStream {
  sock : UdpSocket,  // we clone old io but streamreceive is not allowed
  with : SocketAddr, // old io could be clone , with new io manage protection ourselve
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
      try!(self.sock.send_to(&self.buf[..], self.with));
      self.buf = Vec::new();
      Ok(())
    }
}

impl Read for ReadUdpStreamVariant {
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
}
impl Read for ReadUdpStream {
  fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
    let l = buf.len();
    if l > 0 {
      if l > self.0.len() {
        let r = self.0.len();
        copy_memory(&self.0[..], buf);
        self.0.clear();
        Ok(r)
      } else {
        copy_memory(&self.0[..l], buf);
        self.0 = (&self.0[l..]).to_vec();
        Ok(l)
      }   
    } else {
      Ok(0)
    }
  }
}

impl Transport for Udp {
  type ReadStream = ReadUdpStream;
  type WriteStream = UdpStream;
  type Address = SocketAddr;
  
  fn do_spawn_rec(&self) -> (bool, bool) {
    (self.spawn, false)
  }

  fn start<C> (&self, readHandle : C) -> IoResult<()>
    where C : Fn(Self::ReadStream,Option<Self::WriteStream>) -> IoResult<()> {
    let buffsize = self.buffsize;
    let mut tmpvec : Vec<u8> = vec![0; buffsize];
    let buf = tmpvec.as_mut_slice();
    loop {
      match self.sock.recv_from(buf) {
        Ok((size, from)) => {
          // TODO test with small size to see if full size here
          if size < buffsize {
            // let slice = &buff[0,size]
            let r = unsafe {
              slice::from_raw_parts(buf.as_ptr(), size).to_vec()
            };
            readHandle(ReadUdpStream(r), None);
          }else{
            error!("Datagram on udp transport with size {:?} over buff {:?}, lost datagram", size, buffsize);
          }
        },
        Err(e) => error!("Couldnot receive datagram {}",e),
      }
    };
    Ok(())
  }

  /// does not return a read handle as udp is unconnected (variant with read buf synch would need to
  /// be returned).
  /// Function never fail as udp is unconnected (write will fail!!)
  fn connectwith(&self, p : &SocketAddr, _ : Duration) -> IoResult<(UdpStream, Option<ReadUdpStream>)> {
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


