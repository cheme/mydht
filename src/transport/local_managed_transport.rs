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

use std::sync::mpsc::{Sender,Receiver};
use std::sync::mpsc;
use std::sync::{Arc,Mutex};
use std::io::Result as IoResult;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::io::{Read,Write};
use time::Duration;
use transport::{Transport,Address,ReadTransportStream,WriteTransportStream};

#[derive(Debug,Eq,PartialEq,Clone)]
pub struct LocalAdd (usize);

// note all mutex content is clone in receive loop and in connect_with (only here for init)
// here as transport must be sync .
//
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

    //TODO !!!
    
    
    res
  }
}
pub struct LocalReadStream(Receiver<Arc<Vec<u8>>>);
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

pub struct LocalWriteStream(Sender<(usize,Arc<Vec<u8>>)>);
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
      // TODO read on reader , route to either thread
      Ok(())
  }

  fn connectwith(&self, address : &Self::Address, timeout : Duration) -> IoResult<(Self::WriteStream, Option<Self::ReadStream>)> {
          let er : IoResult<(Self::WriteStream, Option<Self::ReadStream>)> = Err(IoError::new(IoErrorKind::Other, "TODO implement"));
          er

  }

 
}

impl Address for LocalAdd{}
impl Write for LocalWriteStream {
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
      // TODO
      let r : IoResult<usize> = Ok(0);
      r
    }
    fn flush(&mut self) -> IoResult<()> {
      // TODO 
      Ok(())
    }
}


impl Read for LocalReadStream {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
//      TODO
      let r : IoResult<usize> = Ok(0);
      r
    }
}
