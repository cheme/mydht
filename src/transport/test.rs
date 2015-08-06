//! Test primitives for transports.
//!
//!


use std::thread;
use std::sync::mpsc;
use transport::local_managed_transport::{TransportTest,LocalAdd};
use transport::{Transport,Address};
use std::io::Result as IoResult;
use std::io::Read;
use std::io::Write;
use time::Duration;


pub fn connect_rw_with_optional<A : Address + 'static, T : Transport<Address=A> + 'static> (t1 : T, t2 : T, a1 : &A, a2 : &A, with_optional : bool)
where <T as Transport>::ReadStream : 'static
{
  let mess = "hello world".as_bytes();
  let (s,r) = mpsc::channel();
  let readhandler = move |mut rs : <T as Transport>::ReadStream, ows : Option<<T as Transport>::WriteStream> | {
    if with_optional {
      assert!(ows.is_some());
    } else {
      assert!(ows.is_none());
    };
    let sspawn = s.clone();
    // need to thread the process (especially because blocking read and we are in receiving loop)
    let o = thread::spawn(move|| {
      let mut buff = vec!(0;10);
      let rr = rs.read(&mut buff[..]);
      assert!(rr.unwrap() == 10);
      let rr2 = rs.read(&mut buff[..]);
      assert!(rr2.unwrap() == 1);
      sspawn.send(true);
    });
    Ok(())
  };
  let a1c = a1.clone();
  let g = thread::spawn(move|| t1.start(&a1c, readhandler));
  let cres = t2.connectwith(a1, Duration::milliseconds(300));
  assert!(cres.is_ok());
  let (mut ws, ors) = cres.unwrap();
  if with_optional {
    assert!(ors.is_some());
  } else {
    assert!(ors.is_none());
  };
  
  let wr = ws.write(&mess[..]);
  assert!(wr.is_ok());
  r.recv();
}


pub fn sending<A : Address, T : Transport<Address=A>> (t1 : T, t2 : T, a1 : &A, a2 : &A) {
  if t1.do_spawn_rec() == (true,true) && t1.do_spawn_rec() == (true,true) {
  } else {
    error!("transport test was skipped due to incompatible transport behaviour");
  }
}
