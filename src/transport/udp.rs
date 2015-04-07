//! disconnected udp proto
//! incompatible with proxy mode : here client can only send and server can only receive
//! In this model only one thread read (on receive connection)
//! ping is not use to establish connection (in disconnected when receiving ping user is online and
//! we send back ping if we didn't know him (eg find_value on ourself for bt proto)
//! Do not allow attachment.

use super::TransportStream;
use super::Transport;
use kvstore::{Attachment};
use std::io::Result as IoResult;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::net::SocketAddr;
use time::Duration;
use peer::Peer;
use std::iter;
use std::sync::{Mutex,Condvar,Arc};
use std::net::UdpSocket;

// TODO retest after switch to new io

pub struct Udp {
  sock : Arc<Mutex<Option<UdpSocket>>>,  // reference socket the only one to do receive
  sync : Condvar,
  buffsize : usize,
}

impl Udp {
  pub fn new (s : usize) -> Udp {
    Udp {
      sock : Arc::new(Mutex::new(None)),
      sync : Condvar::new(),
      buffsize : s,
    }
  }
}
struct UdpStream {
  sock : UdpSocket,  // we clone old io but streamreceive is not allowed
  with : SocketAddr, // old io could be clone , with new io manage protection ourselve
  //if define we can send overwhise it is send in server : panic!
}

impl Transport for Udp {
  type Stream = UdpStream;

  fn is_connected() -> bool {
    false
  }

  fn receive<C> (&self, p : &SocketAddr, closure : C) where C : Fn(UdpStream, Option<(Vec<u8>, Option<Attachment>)>) -> () {
    let mut socket = match UdpSocket::bind(p) {
      Ok(s) => s,
      Err(e) => panic!("Couldnot bind socket {}",e),
    };
    {
      let mut ms = self.sock.lock().unwrap();
      *ms = Some(socket.try_clone().unwrap());
      self.sync.notify_all();
    }
    let buffsize = self.buffsize;
    let mut tmpvec : Vec<u8> = vec![0; buffsize];
    let buf = tmpvec.as_mut_slice();
    loop {
      match socket.recv_from(buf) {
        Ok((size, from)) => {
          // TODO test with small size to see if full size here
          if size < buffsize {
            // let slice = &buff[0,size]
            let r = unsafe {
              Vec::from_raw_buf(buf.as_ptr(), size)
            };
            closure(UdpStream{with : from, sock : socket.try_clone().unwrap()}, Some((r,None)));
          }else{
            error!("Datagram on udp transport with size {:?} over buff {:?}, lost datagram", size, buffsize);
          }
        },
        Err(e) => error!("Couldnot receive datagram {}",e),
      }
    }
  }

  /// Unconected, will simply instantiate a handle over write only udp stream
  fn connectwith (&self, p : &SocketAddr, _ : Duration) -> IoResult<UdpStream> {
    let s = self.sock.lock().unwrap();
    let so : UdpSocket = match (*s) {
      None => {
        // wait for a bind in receiver
        let mguard = self.sock.lock().unwrap();
        match *self.sync.wait(mguard).unwrap() {
          None => panic!("bind to socket likely failed"),
          Some (ref so) => so.try_clone().unwrap(),
        }
      },
      Some (ref so) => so.try_clone().unwrap() ,
    };
    // get socket (non connected cannot timeout)
    Ok(UdpStream{sock : so, with : p.clone()})
  }

}

impl TransportStream for UdpStream {

  fn streamread(&mut self) -> IoResult<(Vec<u8>,Option<Attachment>)> {
    error!("Trying to read on non connected send only stream");
    Err(IoError::new(
      IoErrorKind::Other,
      "Udp transport is a non connected transport which cannot read from its stream",
    ))
  }

  fn streamwrite(&mut self, m : &[u8], a : Option<&Attachment>) -> IoResult<()> {
    if a.is_some(){
      panic!("Udp transport current implementation does not allow attachment");
    }
    self.sock.send_to(m, self.with).map(|_| ())
  }

}


