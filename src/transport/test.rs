//! Test primitives for transports.
//!
//!


use std::thread;
use std::sync::mpsc;
use transport::{Transport,Address};
use std::io::Result as IoResult;
use std::io::Read;
use std::io::Write;
use time::Duration;
use std::sync::Arc;


pub fn connect_rw_with_optional<A : Address, T : Transport<Address=A>> (t1 : T, t2 : T, a1 : &A, a2 : &A, with_optional : bool)
{
  let mess = "hello world".as_bytes();
  let mess2 = "pong".as_bytes();
  let (s,r) = mpsc::channel();
  let readhandler = move |mut rs : <T as Transport>::ReadStream, mut ows : Option<<T as Transport>::WriteStream> | {
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
      match ows {
        Some (mut ws) => {
          let wr = ws.write(&mess2[..]);
          assert!(wr.is_ok());
        },
        None => (),
      };
      sspawn.send(true);
    });
    Ok(())
  };
  let readhandler2 = move |mut rs : <T as Transport>::ReadStream, mut ows : Option<<T as Transport>::WriteStream> | {
    if with_optional {
      assert!(ows.is_some());
    } else {
      assert!(ows.is_none());
      let mut buff = vec!(0;10);
      let rr = rs.read(&mut buff[..]);
      assert!(rr.unwrap() == 4);
    };

    Ok(())
  };
 
  let a1c = a1.clone();
  let a2c = a2.clone();
  let at1 = Arc::new(t1);
  let at1c = at1.clone();
  let at2 = Arc::new(t2);
  let at2c = at2.clone();

  let g = thread::spawn(move|| at1c.start(readhandler));
  let g2 = thread::spawn(move|| at2c.start(readhandler2));
//  thread::sleep_ms(3000);
  let cres = at2.connectwith(a1, Duration::milliseconds(300));
  assert!(cres.as_ref().is_ok(),"{:?}", cres.as_ref().err());
  let (mut ws, mut ors) = cres.unwrap();
  if with_optional {
    assert!(ors.is_some());
  } else {
    assert!(ors.is_none());
  };
  
  let wr = ws.write(&mess[..]);
  assert!(wr.is_ok());

  match ors {
    Some (mut rs) => {
      let mut buff = vec!(0;10);
      let rr = rs.read(&mut buff[..]);
      assert!(rr.unwrap() == 4);
    },
    // test in read handler
    None => {
      let cres = at1.connectwith(a2, Duration::milliseconds(300));
      assert!(cres.is_ok());
      let wr = cres.unwrap().0.write(&mess2[..]);
      assert!(wr.is_ok());
    },
  };

  r.recv();
}


pub fn sending<A : Address, T : Transport<Address=A>> (t1 : T, t2 : T, a1 : &A, a2 : &A) {
  if t1.do_spawn_rec() == (true,true) && t1.do_spawn_rec() == (true,true) {
  } else {
    error!("transport test was skipped due to incompatible transport behaviour");
  }
}
