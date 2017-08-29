
#[macro_use] extern crate log;
extern crate byteorder;
extern crate mydht_base;
extern crate time;
extern crate num;
extern crate vec_map;
extern crate mio;
#[cfg(test)]
extern crate mydht_basetest;

use std::sync::mpsc;
use std::sync::mpsc::{Sender};
use std::result::Result as StdResult;
use std::mem;
//use self::byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Result as IoResult;
use mydht_base::mydhtresult::Result;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::io::Write;
use std::io::Read;
use time::Duration;
use std::time::Duration as StdDuration;
use mydht_base::transport::{Transport,ReadTransportStream,WriteTransportStream,SpawnRecMode,ReaderHandle,Registerable};
use std::net::SocketAddr;
//use self::mio::tcp::TcpSocket;
use self::mio::net::TcpListener;
use std::fmt::Debug;
use self::mio::net::TcpStream;
//use self::mio::tcp;
use self::mio::Token;
use self::mio::Poll;
use self::mio::Ready;
use self::mio::PollOpt;
use self::mio::tcp::Shutdown;
//use super::Attachment;
use num::traits::ToPrimitive;
use self::vec_map::VecMap;
//use std::sync::Mutex;
//use std::sync::Arc;
//use std::sync::Condvar;
//use std::sync::PoisonError;
use std::error::Error;
use mydht_base::utils::{self,OneResult};
//use std::os::unix::io::AsRawFd;
//use std::os::unix::io::FromRawFd;
use mydht_base::transport::{SerSocketAddr};

#[cfg(feature="with-extra-test")]
#[cfg(test)]
use mydht_basetest::transport::connect_rw_with_optional;
#[cfg(test)]
use mydht_basetest::transport::{
  reg_mpsc_recv_test as reg_mpsc_recv_test_base,
  reg_connect_2 as reg_connect_2_base,
  reg_rw_testing,
  reg_rw_corout_testing,
};
#[cfg(feature="with-extra-test")]
#[cfg(test)]
use mydht_basetest::transport::connect_rw_with_optional_non_managed;
#[cfg(feature="with-extra-test")]
#[cfg(test)]
use mydht_base::utils::{sa4};


#[cfg(feature="with-extra-test")]
#[cfg(test)]
use std::net::Ipv4Addr;
#[cfg(feature="with-extra-test")]
#[cfg(test)]
use std::sync::Arc;
#[cfg(feature="with-extra-test")]
#[cfg(test)]
use mydht_base::transport::Address;
#[cfg(feature="with-extra-test")]
#[cfg(test)]
use std::thread;

const CONN_REC : usize = 0;

/// Tcp struct : two options, timeout for connect and time out when connected.
pub struct Tcp {
  keepalive : Option<StdDuration>,
  listener : TcpListener,
  mult : bool,
}


impl Tcp {
  /// constructor.
  pub fn new (p : &SocketAddr,  keepalive : Option<StdDuration>, mult : bool) -> IoResult<Tcp> {

    let tcplistener = TcpListener::bind(p)?;

    Ok(Tcp {
      keepalive : keepalive,
      listener : tcplistener,
      mult : mult,
    })
  }
}


impl Transport for Tcp {
  type ReadStream = TcpStream;
  type WriteStream = TcpStream;
  type Address = SerSocketAddr;

  /// if spawn we do not loop (otherwhise we should not event loop at all), the thread is just for
  /// one message.
  fn do_spawn_rec(&self) -> SpawnRecMode {
        SpawnRecMode::Local
  }

  fn start<C> (&self, readhandler : C) -> Result<()>
    where C : Fn(TcpStream,Option<TcpStream>) -> Result<ReaderHandle> {
    Ok(())
  }
  fn accept(&self) -> Result<(Self::ReadStream, Option<Self::WriteStream>, Self::Address)> {
    let (s,ad) = self.listener.accept()?;
    debug!("Initiating socket exchange : ");
    debug!("  - From {:?}", s.local_addr());
    debug!("  - With {:?}", s.peer_addr());
    s.set_keepalive(self.keepalive)?;
//    try!(s.set_write_timeout(self.timeout.num_seconds().to_u64().map(Duration::from_secs)));
    if self.mult {
//            try!(s.set_keepalive (self.timeout.num_seconds().to_u32()));
        let rs = try!(s.try_clone());
        Ok((s,Some(rs),SerSocketAddr(ad)))
    } else {
        Ok((s,None,SerSocketAddr(ad)))
    }
  }


  fn connectwith(&self,  p : &SerSocketAddr, timeout : Duration) -> IoResult<(Self::WriteStream, Option<Self::ReadStream>)> {
    let s = TcpStream::connect(&p.0)?;
    s.set_keepalive(self.keepalive)?;
    // TODO set nodelay and others!!
//    try!(s.set_keepalive (self.timeout.num_seconds().to_u32()));
    if self.mult {
      let rs = try!(s.try_clone());
      Ok((s,Some(rs)))
    } else {
      Ok((s,None))
    }
  }



}

impl Registerable for Tcp {
  fn register(&self, poll : &Poll, token: Token, interest: Ready, opts: PollOpt) -> Result<bool> {
    poll.register(&self.listener,token,interest,opts)?;
    Ok(true)
  }
  fn reregister(&self, poll : &Poll, token: Token, interest: Ready, opts: PollOpt) -> Result<bool> {
    poll.register(&self.listener,token,interest,opts)?;
    Ok(true)
  }
}
 
#[cfg(feature="with-extra-test")]
#[test]
fn connect_rw () {
  let start_port = 40000;

  let a1 = SerSocketAddr(sa4(Ipv4Addr::new(127,0,0,1), start_port));
  let a2 = SerSocketAddr(sa4(Ipv4Addr::new(127,0,0,1), start_port+1));
  let tcp_transport_1 : Tcp = Tcp::new (&a1, Some(StdDuration::from_secs(5)), true).unwrap();
  let tcp_transport_2 : Tcp = Tcp::new (&a2, Some(StdDuration::from_secs(5)), true).unwrap();
  // TODO test with spawn rewrite test without start but 

  connect_rw_with_optional(tcp_transport_1,tcp_transport_2,&a1,&a2,true,true);
}

#[test]
fn reg_mpsc_recv_test() {
  let start_port = 40010;
  let a1 = SerSocketAddr(sa4(Ipv4Addr::new(127,0,0,1), start_port));
  let t = Tcp::new (&a1, Some(StdDuration::from_secs(5)), true).unwrap();
  reg_mpsc_recv_test_base(t);
}


#[test]
fn reg_connect_2() {
  let start_port = 40020;
  let a0 = SerSocketAddr(sa4(Ipv4Addr::new(127,0,0,1), start_port));
  let t0 = Tcp::new (&a0, Some(StdDuration::from_secs(5)), true).unwrap();
  let a1 = SerSocketAddr(sa4(Ipv4Addr::new(127,0,0,1), start_port+1));
  let t1 = Tcp::new (&a1, Some(StdDuration::from_secs(5)), true).unwrap();
  let a2 = SerSocketAddr(sa4(Ipv4Addr::new(127,0,0,1), start_port+2));
  let t2 = Tcp::new (&a2, Some(StdDuration::from_secs(5)), true).unwrap();
  reg_connect_2_base(&a0,t0,t1,t2);
}

#[test]
fn reg_rw_state1() {
  reg_rw_state(40030,120,120,120,2);
}
#[test]
fn reg_rw_state2() {
  reg_rw_state(40040,240,120,120,2);
}
#[test]
fn reg_rw_state3() {
  reg_rw_state(40050,240,250,50,2);
}
#[test]
fn reg_rw_state4() {
  reg_rw_state(40060,240,50,250,2);
}




#[cfg(test)]
fn reg_rw_state(start_port : u16, content_size : usize, read_buf : usize, write_buf : usize, nbmess : usize ) {
  let a0 = SerSocketAddr(sa4(Ipv4Addr::new(127,0,0,1), start_port));
  let t0 = Tcp::new (&a0, Some(StdDuration::from_secs(5)), true).unwrap();
  let a1 = SerSocketAddr(sa4(Ipv4Addr::new(127,0,0,1), start_port+1));
  let t1 = Tcp::new (&a1, Some(StdDuration::from_secs(5)), true).unwrap();
  // content, read buf size , write buf size, and nb send
  reg_rw_testing(a0,t0,a1,t1,content_size,read_buf,write_buf,nbmess);
}
#[cfg(test)]
fn reg_rw_corout(start_port : u16, content_size : usize, read_buf : usize, write_buf : usize, nbmess : usize ) {
  let a0 = SerSocketAddr(sa4(Ipv4Addr::new(127,0,0,1), start_port));
  let t0 = Tcp::new (&a0, Some(StdDuration::from_secs(5)), true).unwrap();
  let a1 = SerSocketAddr(sa4(Ipv4Addr::new(127,0,0,1), start_port+1));
  let t1 = Tcp::new (&a1, Some(StdDuration::from_secs(5)), true).unwrap();
  // content, read buf size , write buf size, and nb send
  reg_rw_corout_testing(a0,t0,a1,t1,content_size,read_buf,write_buf,nbmess);
}

#[test]
fn reg_rw_corout1() {
  reg_rw_corout(40030,120,120,120,2);
}
#[test]
fn reg_rw_corout2() {
  reg_rw_corout(40040,240,120,120,2);
}
#[test]
fn reg_rw_corout3() {
  reg_rw_corout(40050,240,250,50,3);
}
#[test]
fn reg_rw_corout4() {
  reg_rw_corout(40060,240,50,250,2);
}



#[test]
fn reg_conn_rw_1() {
  let content_size = 123;
  let buf_read_size = 20;
  let buf_write_size = 20;

}
