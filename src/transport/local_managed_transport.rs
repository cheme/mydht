//! Transport for testing on a local program.
//! It only provides communication between threads and should only be used for test purpose.
//!
//! This test transport is for managed transport : transport like tcp which got blocking read and
//! requires a thread for reading.
//! 
//! The transport contains sender to every other initialised transport and a receiver (channels).
//!
//! connection request is done with empty Vec<u8>.
//! connection established is done implicitly
//!
//! SyncChannel are used, in case of buffer issue we should Mutex over a Channel
//! 
//! The implementation differs from tcp because it is non connected (error on send not on connect),
//! and multiple connections between peers are not supported. : the transport is not well suited
//! for connectivity testing.
//! 
//! TODO bidire (no optional read) need to double space of cli sender with semantic like : addr
//! conn = addr and addr recv = addr + nb peer if already one at addr (addr switch)
//!

use std::sync::mpsc::{Sender,Receiver};
use std::sync::mpsc;
use std::sync::{Arc,Mutex};
use std::io::Result as IoResult;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::io::{Read,Write};
use time::Duration;
use transport::{Transport,Address,ReadTransportStream,WriteTransportStream};
#[cfg(test)]
use transport::test as ttest;

#[derive(RustcDecodable,RustcEncodable,Debug,PartialEq,Eq,Clone)]
pub struct LocalAdd (pub usize);

// note all mutex content is clone in receive loop and in connect_with (only here for init)
// here as transport must be sync .
pub struct TransportTest {
  pub address : usize,
  /// directory with all sender of other peers
  /// Addresses are position in vec
  /// usize is our address
  pub dir : Mutex<Vec<Sender<(usize,Arc<Vec<u8>>)>>>,
  
  /// usize is address from emitter
  pub recv : Mutex<Receiver<(usize,Arc<Vec<u8>>)>>,
  
  /// sender to connected client receiver
  pub cli : Mutex<Vec<Option<Sender<Arc<Vec<u8>>>>>>,
}

impl TransportTest {
  /// initialize n transport which can communicate with each other, the address to use with
  /// transport is simple the index in the returning vec
  pub fn create_transport (nb : usize) -> Vec<TransportTest> {
    let mut res = Vec::with_capacity(nb);
    let mut vecsender = Vec::with_capacity(nb);
    let mut vecreceiver = Vec::with_capacity(nb);
    let mut veccon = Vec::with_capacity(nb);

    for i in 0 .. nb {
      let (s,r) = mpsc::channel();
      vecsender.push(s);
      vecreceiver.push(r);
      veccon.push(None);
    }
    vecreceiver.reverse();
   
    for i in 0 .. nb {
      let tr = TransportTest {
        address : i,
        dir : Mutex::new(vecsender.clone()),
        recv : Mutex::new(vecreceiver.pop().unwrap()),
        cli : Mutex::new(veccon.clone()),
      };
      res.push(tr);
    }
    res
  }
}

pub struct LocalReadStream(Receiver<Arc<Vec<u8>>>,Vec<u8>);

impl ReadTransportStream for LocalReadStream {
  fn disconnect(&mut self) -> IoResult<()> {
    let (_,t) = mpsc::channel();
//    self.0.drop()
    self.0 = t;
    Ok(())
  }
  fn rec_end_condition(&self) -> bool {
    false
  }
}
impl WriteTransportStream for LocalWriteStream {
  fn disconnect(&mut self) -> IoResult<()> {
    // nothing TODO a disconnect message???

    Ok(())
  }
}

pub struct LocalWriteStream(usize,Sender<(usize,Arc<Vec<u8>>)>);

/// default dospawn impl : it is a managed transport.
impl Transport for TransportTest {
  /// chanel from transport receiving (loop on connection)
  type ReadStream = LocalReadStream;
  /// chanel to other transport
  type WriteStream = LocalWriteStream;
  /// index in transport dir
  type Address = LocalAdd;
  fn start<C> (&self, address : &Self::Address, readhandler : C) -> IoResult<()>
    where C : Fn(Self::ReadStream,Option<Self::WriteStream>) -> IoResult<()> {
      // lock mutex indefinitely but it is the only occurence
      let r = self.recv.lock().unwrap();

      // idem for cli
      let us = self.dir.lock().unwrap().get(self.address).unwrap().clone();
      loop {
        match r.recv() {
          Ok((addr, content)) => {
            if content.len() > 0 {
              let clis = self.cli.lock().unwrap();
              match clis.get(addr) {
                Some(&Some(ref s)) => {
                  s.send(content);
                },
                _ => {
                  error!("received message but no connection established");
                  panic!("received message but no connection established");
                },
              };
            } else {
              // new connection
              let (s,r) = mpsc::channel();
              
              let locread = LocalReadStream(r,Vec::new());
// TODO conditionally send write stream !!!
              let locwrite = {
                let dest = self.dir.lock().unwrap().get(addr).unwrap().clone();
                Some(LocalWriteStream(self.address, dest.clone()))
              };
              {
                let mut clis = self.cli.lock().unwrap();
                let mut cur = clis.get_mut(addr).unwrap();
                *cur = Some(s);
              }
              readhandler(locread,locwrite);
            }
          },
          Err(_) => {
            error!("transport test receive failure");
          },
        }
      }
      // TODO read on reader , route to either thread
      Ok(())
  }

  fn connectwith(&self, address : &Self::Address, timeout : Duration) -> IoResult<(Self::WriteStream, Option<Self::ReadStream>)> {
    let locwrite = {
      let dest = self.dir.lock().unwrap().get(address.0).unwrap().clone();
      LocalWriteStream(self.address, dest.clone())
    };
 
    // TODO conditionally do that
    
    let (s,r) = mpsc::channel();
    let us = self.dir.lock().unwrap().get(self.address).unwrap().clone();
    let locread = Some(LocalReadStream(r,Vec::new()));
    {
      let mut clis = self.cli.lock().unwrap();
      let mut cur = clis.get_mut(address.0).unwrap();
      *cur = Some(s);
    }

    // connect msg
    locwrite.1.send((locwrite.0, Arc::new(Vec::new())));

    Ok((locwrite,locread))

  }

 
}

impl Address for LocalAdd{}
impl Write for LocalWriteStream {
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
      let len = buf.len();
      self.1.send((self.0,Arc::new(buf.to_vec()))).unwrap();
      Ok(len)
    }
    fn flush(&mut self) -> IoResult<()> {
      Ok(())
    }
}


//pub struct LocalReadStream(Receiver<Arc<Vec<u8>>>,Vec<u8>);
/// highly inefficient
impl Read for LocalReadStream {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
      let bulen = buf.len();
      let culen = self.1.len();
      let mut afrom = if culen != 0 {
        // verydirty
        Arc::new(Vec::new())
      } else {
        self.0.recv().unwrap()
      };
      let (nbr,rem) = {
      let mut from : &mut Vec<u8> = if culen != 0 {
        &mut self.1
      } else {
        Arc::make_unique(&mut afrom)
      };
      let fromlen = from.len();
      let mut rfrom : & [u8] = from.as_ref();
      let nbr = try!(rfrom.read(buf));
      if nbr < fromlen {
        (nbr, (&from[nbr..fromlen]).to_vec())
      } else {
        (nbr, Vec::new())
      }
    };
      self.1 = rem;
      Ok(nbr)
    }
}

#[test]
fn test_connect_rw () {
 let a1 = &LocalAdd(0);
 let a2 = &LocalAdd(1);
 let mut trs = TransportTest::create_transport (2);
 let t2 = trs.pop().unwrap();
 let t1 = trs.pop().unwrap();

 ttest::connect_rw_with_optional (t1 , t2 , a1 , a2, true); 
}
