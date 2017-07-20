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
//!
//! Message format is :
//! - first usize is peer address of sender (index in preinitialized directory)
//! - second usize is nb of connection established through a connectwith (received as from) :
//! only used when multiplexed.
//! - third is nb of con through reception (reader created on receiption).
//!
//!
//! When running non managed, the transport act like udp : with one frame containing one message
//! receive close on end_read_message, and on reception : handle to listener is only use once!!
//!
//! For more hybrid non managed transport like tcp_loop (where we run once but allow recption of
//! several frames (sync in transport with its rec)) we act like managed, and managed transport test should
//! be used.

use std::sync::mpsc::{Sender,Receiver};
use std::sync::mpsc;
use std::sync::{Arc,Mutex};
use std::io::Result as IoResult;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::io::{Read,Write};
use mydht_base::mydhtresult::Result;
use time::Duration;
use mydht_base::transport::{Transport,Address,ReadTransportStream,WriteTransportStream,SpawnRecMode,ReaderHandle};
#[cfg(test)]
use transport as ttest;
use transport::LocalAdd;


// note all mutex content is clone in receive loop and in connect_with (only here for init)
// here as transport must be sync .
pub struct TransportTest {
  pub multiplex : bool,
  pub managed : bool,
  pub address : usize,
  /// directory with all sender of other peers
  /// Addresses are position in vec
  /// usize is our address
  pub dir : Mutex<Vec<Sender<(usize,usize,usize,Arc<Vec<u8>>)>>>,
  
  /// usize is address from emitter
  pub recv : Mutex<Receiver<(usize,usize,usize,Arc<Vec<u8>>)>>,
  
  /// sender to connected client receiver, when connection established from peer
  pub cli_from : Mutex<Vec<Vec<Sender<Arc<Vec<u8>>>>>>,
  /// sender to connected client receiver, when connection established with peer
  pub cli_with : Mutex<Vec<Vec<Sender<Arc<Vec<u8>>>>>>,
}

impl TransportTest {
  /// initialize n transport which can communicate with each other, the address to use with
  /// transport is simple the index in the returning vec
  pub fn create_transport (nb : usize, multiplex : bool, managed : bool) -> Vec<TransportTest> {
    let mut res = Vec::with_capacity(nb);
    let mut vecsender = Vec::with_capacity(nb);
    let mut vecreceiver = Vec::with_capacity(nb);
    let mut veccon = Vec::with_capacity(nb);
    let mut veccon2 = Vec::with_capacity(nb);

    for _ in 0 .. nb {
      let (s,r) = mpsc::channel();
      vecsender.push(s);
      vecreceiver.push(r);
      if managed {
        veccon.push(Vec::new());
        veccon2.push(Vec::new());
      };
    }
    vecreceiver.reverse();
   
    for i in 0 .. nb {
      let tr = TransportTest {
        multiplex : multiplex,
        managed : managed,
        address : i,
        dir : Mutex::new(vecsender.clone()),
        recv : Mutex::new(vecreceiver.pop().unwrap()),
        cli_from : Mutex::new(veccon.clone()),
        cli_with : Mutex::new(veccon2.clone()),
      };
      res.push(tr);
    }
    res
  }
}

pub struct LocalReadStream(Receiver<Arc<Vec<u8>>>,Vec<u8>,bool,bool);

impl ReadTransportStream for LocalReadStream {
  fn disconnect(&mut self) -> IoResult<()> {
    self.2 = false;
    Ok(())
  }
  fn rec_end_condition(&self) -> bool {
    !self.3
  }
  fn end_read_msg(&mut self) -> () {
    if !self.3 {
      self.2 = false; // disconnect to ensure next read fails like with a non managed transport
    };
    ()
  }
}
impl WriteTransportStream for LocalWriteStream {
  fn disconnect(&mut self) -> IoResult<()> {
    self.4 = false;

    Ok(())
  }
}

pub struct LocalWriteStream(usize,usize,usize,Sender<(usize,usize,usize,Arc<Vec<u8>>)>,bool);

/// default dospawn impl : it is a managed transport.
impl Transport for TransportTest {
  /// chanel from transport receiving (loop on connection)
  type ReadStream = LocalReadStream;
  /// chanel to other transport
  type WriteStream = LocalWriteStream;
  /// index in transport dir
  type Address = LocalAdd;
  fn start<C> (&self, readhandler : C) -> Result<()>
    where C : Fn(Self::ReadStream,Option<Self::WriteStream>) -> Result<ReaderHandle> {
      // lock mutex indefinitely but it is the only occurence
      let r = self.recv.lock().unwrap();

      loop {
        match r.recv() {
          Ok((addr, nbcon_from, nbcon_with, content)) => {
            if content.len() > 0 {
              assert!(nbcon_with == 0 || nbcon_from == 0);
              let (clis,nbcon) = if nbcon_with == 0 {
                (self.cli_from.lock().unwrap(),nbcon_from)
              } else {
                (self.cli_with.lock().unwrap(),nbcon_with)
              };
              match clis.get(addr) {
                Some(ref s) => {
                  try!(s.get(nbcon - 1).unwrap().send(content));
                },
                _ => {
                  if self.managed {
                    error!("received message but no connection established");
                    panic!("received message but no connection established");
                  } else {
                    // TODO useless channel ... 
                    let (_,r) = mpsc::channel();
                    // bad clone but for test... cf TODO chanel : local read stream should be enum
                    let rs = LocalReadStream(r,(*content).clone(),true,self.managed);
                    try!(readhandler(rs,None));
                  }
                },
              };
            } else {
              // new connection // TODO Not if !managed (all on connection 0)
              
              assert!(nbcon_with == 0);
              // no connection message when not managed
              assert!(self.managed);
              let (locread, connb) = 
              {
                let (s,r) = mpsc::channel();
                let mut clis = self.cli_from.lock().unwrap();
                let mut cur = clis.get_mut(addr).unwrap();
                cur.push(s);
                let nbcon = cur.len();
                assert!(nbcon == nbcon_from);
                (LocalReadStream(r,Vec::new(),true,self.managed), nbcon)
              };

              let locwrite = if self.multiplex {
                let dest = self.dir.lock().unwrap().get(addr).unwrap().clone();
                Some(LocalWriteStream(self.address, 0, connb, dest.clone(),true))
              } else {
                None
              };
              try!(readhandler(locread,locwrite));
            }
          },
          Err(_) => {
            error!("transport test receive failure");
          },
        }
      }
      // TODO read on reader , route to either thread
      //Ok(())
  }

  fn connectwith(&self, address : &Self::Address, _ : Duration) -> IoResult<(Self::WriteStream, Option<Self::ReadStream>)> {
    
    let (locread,connb) =  if self.managed {
      let mut clis = self.cli_with.lock().unwrap();
      let mut cur = clis.get_mut(address.0).unwrap();
      // done in both case to keep up to date index of con (useless channel when non connected)
      let (s,r) = mpsc::channel();
      cur.push(s);
      if self.multiplex {
        let nbcon = cur.len();
        (Some(LocalReadStream(r,Vec::new(),true,self.managed)), nbcon)
      } else {
        let nbcon = cur.len();
        (None, nbcon)
      }
    } else {
      (None,0)
    };
 
    let locwrite = {
      let dest = self.dir.lock().unwrap().get(address.0).unwrap().clone();
      LocalWriteStream(self.address, connb, 0, dest.clone(),true)
    };
 
   // connect msg
   if self.managed {
     match 
    locwrite.3.send((locwrite.0, locwrite.1, locwrite.2, Arc::new(Vec::new()))) {
      Err(_) => 
        return Err(IoError::new (
          IoErrorKind::BrokenPipe,
          "mpsc send failed",
        )),
      _ => (),
    }
   };

    Ok((locwrite,locread))

  }

  fn do_spawn_rec(&self) -> SpawnRecMode {
    if self.managed {
      SpawnRecMode::Threaded
    } else {
      SpawnRecMode::LocalSpawn
    }
  }


}

impl Write for LocalWriteStream {
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
      if !self.4 {
        return Err(IoError::new (
          IoErrorKind::NotConnected,
          "Closed writer",
        ))
      };
    
      let len = buf.len();
      self.3.send((self.0,self.1,self.2,Arc::new(buf.to_vec()))).unwrap();
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
      // TODO read once on mpsc for non managed (or none and init in buf (better)
      if !self.2 {
        return Err(IoError::new (
          IoErrorKind::NotConnected,
          "Closed reader",
        ))
      };
      let culen = self.1.len();
      if !self.3 {
        // non managed
        if culen == 0 {
          return Err(IoError::new (
            IoErrorKind::NotConnected,
            "reader empty for non managed",
          ))
 
        }
      };
      //let bulen = buf.len();
      let mut afrom = if culen != 0 {
        // verydirty
        Arc::new(Vec::new())
      } else {
        self.0.recv().unwrap()
      };
      let (nbr,rem) = {
      let from : &mut Vec<u8> = if culen != 0 {
        &mut self.1
      } else {
        Arc::make_mut(&mut afrom)
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
 let multiplex = true;
 let a1 = &LocalAdd(0);
 let a2 = &LocalAdd(1);
 let mut trs = TransportTest::create_transport (2,multiplex,true);
 let t2 = trs.pop().unwrap();
 let t1 = trs.pop().unwrap();

 ttest::connect_rw_with_optional (t1 , t2 , a1 , a2, multiplex); 
}
#[test]
fn test_connect_rw_dup () {
 let multiplex = false;
 let a1 = &LocalAdd(0);
 let a2 = &LocalAdd(1);
 let mut trs = TransportTest::create_transport (2,multiplex,true);
 let t2 = trs.pop().unwrap();
 let t1 = trs.pop().unwrap();

 ttest::connect_rw_with_optional (t1 , t2 , a1 , a2, multiplex); 
}
#[test]
fn test_connect_rw_nonmanaged () {
 let multiplex = false;
 let a1 = &LocalAdd(0);
 let a2 = &LocalAdd(1);
 let mut trs = TransportTest::create_transport (2,multiplex,false);
 let t2 = trs.pop().unwrap();
 let t1 = trs.pop().unwrap();

 ttest::connect_rw_with_optional_non_managed (t1 , t2 , a1 , a2, multiplex,multiplex,false); 
}

