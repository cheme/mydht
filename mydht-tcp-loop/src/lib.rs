
#[macro_use] extern crate log;
extern crate byteorder;
extern crate mydht_base;
extern crate vec_map;
extern crate mio;
#[cfg(test)]
extern crate mydht_basetest;

/*use std::sync::mpsc;
use std::sync::mpsc::{Sender};
use std::result::Result as StdResult;
use std::mem;*/
//use self::byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Result as IoResult;
use mydht_base::mydhtresult::Result;
/*use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::io::Write;
use std::io::Read;*/
use std::time::Duration as StdDuration;
use mydht_base::transport::{
  Transport,
  Registerable,
  Token,
  Ready,
};
use std::net::SocketAddr;
//use self::mio::tcp::TcpSocket;
use self::mio::net::TcpListener;
use self::mio::net::TcpStream;
//use self::mio::tcp;
use self::mio::Token as MioToken;
use self::mio::Poll;
use self::mio::Ready as MioReady;
use self::mio::PollOpt;
//use super::Attachment;
//use std::sync::Mutex;
//use std::sync::Arc;
//use std::sync::Condvar;
//use std::sync::PoisonError;
//use std::os::unix::io::AsRawFd;
//use std::os::unix::io::FromRawFd;
use mydht_base::transport::{
  SerSocketAddr,
  LoopResult,
};

#[cfg(test)]
use mydht_basetest::transport::{
  reg_mpsc_recv_test as reg_mpsc_recv_test_base,
  reg_connect_2 as reg_connect_2_base,
  reg_rw_testing,
  reg_rw_corout_testing,
  reg_rw_cpupool_testing,
  reg_rw_threadpark_testing,
};
#[cfg(feature="with-extra-test")]
#[cfg(test)]
use mydht_base::utils::{sa4};


#[cfg(feature="with-extra-test")]
#[cfg(test)]
use std::net::Ipv4Addr;

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


impl Transport<Poll> for Tcp {
  type ReadStream = TcpStream;
  type WriteStream = TcpStream;
  type Address = SerSocketAddr;


  fn accept(&self) -> Result<(Self::ReadStream, Option<Self::WriteStream>)> {
    let (s,ad) = self.listener.accept()?;
    debug!("Initiating socket exchange : ");
    debug!("  - From {:?}", s.local_addr());
    debug!("  - With {:?}", s.peer_addr());
    debug!("  - At {:?}", ad);
    s.set_keepalive(self.keepalive)?;
//    try!(s.set_write_timeout(self.timeout.num_seconds().to_u64().map(Duration::from_secs)));
    if self.mult {
//            try!(s.set_keepalive (self.timeout.num_seconds().to_u32()));
        let rs = try!(s.try_clone());
        Ok((s,Some(rs)))
    } else {
        Ok((s,None))
    }
  }


  fn connectwith(&self,  p : &SerSocketAddr) -> IoResult<(Self::WriteStream, Option<Self::ReadStream>)> {
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

impl Registerable<Poll> for Tcp {
  fn register(&self, poll : &Poll, token: Token, interest: Ready) -> LoopResult<bool> {

    match interest {
      Ready::Readable =>
        poll.register(&self.listener, MioToken(token), MioReady::readable(), PollOpt::edge())?,
      Ready::Writable =>
        poll.register(&self.listener, MioToken(token), MioReady::writable(), PollOpt::edge())?,
    }

    Ok(true)
  }
  fn reregister(&self, poll : &Poll, token: Token, interest: Ready) -> LoopResult<bool> {
    match interest {
      Ready::Readable =>
        poll.reregister(&self.listener, MioToken(token), MioReady::readable(), PollOpt::edge())?,
      Ready::Writable =>
        poll.reregister(&self.listener, MioToken(token), MioReady::writable(), PollOpt::edge())?,
    }

    Ok(true)
  }
  fn deregister(&self, poll : &Poll) -> LoopResult<()> {
    poll.deregister(&self.listener)?;
    Ok(())
  }

}
/*tttttttttt 
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
}*/

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
  reg_rw_corout(40070,120,120,120,2);
}
#[test]
fn reg_rw_corout2() {
  reg_rw_corout(40080,240,120,120,2);
}
#[test]
fn reg_rw_corout3() {
  reg_rw_corout(40090,240,250,50,3);
}
#[test]
fn reg_rw_corout4() {
  reg_rw_corout(40100,240,50,250,2);
}

#[cfg(test)]
fn reg_rw_cpupool(start_port : u16, content_size : usize, read_buf : usize, write_buf : usize, nbmess : usize, poolsize : usize ) {
  let a0 = SerSocketAddr(sa4(Ipv4Addr::new(127,0,0,1), start_port));
  let t0 = Tcp::new (&a0, Some(StdDuration::from_secs(5)), true).unwrap();
  let a1 = SerSocketAddr(sa4(Ipv4Addr::new(127,0,0,1), start_port+1));
  let t1 = Tcp::new (&a1, Some(StdDuration::from_secs(5)), true).unwrap();
  // content, read buf size , write buf size, and nb send
  reg_rw_cpupool_testing(a0,t0,a1,t1,content_size,read_buf,write_buf,nbmess,poolsize);
}

#[test]
fn reg_rw_cpupool1() {
  reg_rw_cpupool(40110,120,120,120,2,2);
}
#[test]
fn reg_rw_cpupool2() {
  reg_rw_cpupool(40120,240,120,120,2,2);
}
#[test]
fn reg_rw_cpupool3() {
  reg_rw_cpupool(40130,240,250,50,20,2);
}
#[test]
fn reg_rw_cpupool4() {
  reg_rw_cpupool(40140,240,50,250,2,3);
}


#[cfg(test)]
fn reg_rw_threadpark(start_port : u16, content_size : usize, read_buf : usize, write_buf : usize, nbmess : usize ) {
  let a0 = SerSocketAddr(sa4(Ipv4Addr::new(127,0,0,1), start_port));
  let t0 = Tcp::new (&a0, Some(StdDuration::from_secs(5)), true).unwrap();
  let a1 = SerSocketAddr(sa4(Ipv4Addr::new(127,0,0,1), start_port+1));
  let t1 = Tcp::new (&a1, Some(StdDuration::from_secs(5)), true).unwrap();
  // content, read buf size , write buf size, and nb send
 reg_rw_threadpark_testing(a0,t0,a1,t1,content_size,read_buf,write_buf,nbmess);
}


#[test]
fn reg_rw_threadpark1() {
  reg_rw_threadpark(40210,120,120,120,2);
}
#[test]
fn reg_rw_threadpark2() {
  reg_rw_threadpark(40220,240,120,120,2);
}
#[test]
fn reg_rw_threadpark3() {
  reg_rw_threadpark(40230,240,250,50,20);
}
#[test]
fn reg_rw_threadpark4() {
  reg_rw_threadpark(40240,240,50,250,2);
}


