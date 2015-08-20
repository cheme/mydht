//! Test primitives for transports.
//!
//!


use std::thread;
use std::sync::mpsc;
use transport::{Transport,Address};
use std::io::Result as IoResult;
use mydhtresult::Result;
use std::io::Read;
use std::io::Write;
use time::Duration;
use std::sync::Arc;

pub fn connect_rw_with_optional<A : Address, T : Transport<Address=A>> (t1 : T, t2 : T, a1 : &A, a2 : &A, with_optional : bool)
{
//  assert!(t1.do_spawn_rec().1 == true); // managed so we can receive multiple message : test removed due to hybrid transport lik tcp_loop where it is usefull to test those properties
  let mess_to = "hello world".as_bytes();
  let mess_to_2 = "hello2".as_bytes();
  let mess_from = "pong".as_bytes();
  let (s,r) = mpsc::channel();
  let (s2,r2) = mpsc::channel();
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
      let remain = if with_optional {
        assert!(rr2.unwrap() == 1);
        true
      } else {
        let s = rr2.unwrap();
        if s == 1 {
          true
        } else {
          // TODO when false test is stuck??
          assert!(s == 7);
          false
        }
      };
      match ows {
        Some (mut ws) => {
          let wr = ws.write(&mess_from[..]);
          assert!(wr.is_ok());
          assert!(ws.flush().is_ok());
        },
        None => (),
      };
      if remain {
        let rr = rs.read(&mut buff[..]);
        assert!(rr.unwrap() == 6);
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
      let sspawn = s2.clone();
      let o = thread::spawn(move|| {
        let mut buff = vec!(0;10);
        let rr = rs.read(&mut buff[..]);
        assert!(rr.unwrap() == 4);
        sspawn.send(true);
      });
    };

    Ok(())
  };
 
  let a1c = a1.clone();
  let a2c = a2.clone();
  let at1 = Arc::new(t1);
  let at1c = at1.clone();
  let at2 = Arc::new(t2);
  let at2c = at2.clone();

  let g = thread::spawn(move|| {at1c.start(readhandler);});
  let g2 = thread::spawn(move|| {at2c.start(readhandler2);});
//  thread::sleep_ms(3000);
  let cres = at2.connectwith(a1, Duration::milliseconds(300));
  assert!(cres.as_ref().is_ok(),"{:?}", cres.as_ref().err());
  let (mut ws, mut ors) = cres.unwrap();
  if with_optional {
    assert!(ors.is_some());
  } else {
    assert!(ors.is_none());
  };
  
  let wr = ws.write(&mess_to[..]);
  assert!(wr.is_ok());
  assert!(ws.flush().is_ok());

  match ors {
    Some (ref mut rs) => {
      let mut buff = vec!(0;10);
      let rr = rs.read(&mut buff[..]);
      println!("rr : {:?}", rr);
      assert!(rr.is_ok());
      assert!(rr.unwrap() == 4);
    },
    // test in read handler
    None => {
      let cres = at1.connectwith(a2, Duration::milliseconds(300));
      assert!(cres.is_ok());
      let mut ws = cres.unwrap().0;
      let wr = ws.write(&mess_from[..]);
      assert!(wr.is_ok());
      assert!(ws.flush().is_ok());
    },
  };

  let wr = ws.write(&mess_to_2[..]);
  assert!(wr.is_ok());
  assert!(ws.flush().is_ok());


  r.recv();
  if !with_optional {
    r2.recv();
  } else {()};
}

pub fn connect_rw_with_optional_non_managed<A : Address, T : Transport<Address=A>> (t1 : T, t2 : T, a1 : &A, a2 : &A, with_optional : bool)
{
  assert!(t1.do_spawn_rec().1 == false); // managed so we can receive multiple message
  let spawn = t1.do_spawn_rec().0; // TODO conditional spawn (for now always spawn)
  let mess_to = "hello world".as_bytes();
  let mess_to_2 = "hello2".as_bytes();
  let mess_from = "pong".as_bytes();
  let (s,r) = mpsc::channel();
  let (s2,r2) = mpsc::channel();
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
      assert!(rr.is_ok());
      let s = rr.unwrap();
      if s == 10 {
        // first message
        let rr2 = rs.read(&mut buff[..]);
        assert!(rr2.is_ok());
        assert!(rr2.unwrap() == 1);
        match ows {
          Some (mut ws) => {
            let wr = ws.write(&mess_from[..]);
            assert!(wr.is_ok());
            assert!(ws.flush().is_ok());
          },
          None => (),

        };
      } else {
        // second message
        assert!(s == 6);
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
      let sspawn = s2.clone();
      let o = thread::spawn(move|| {
        let mut buff = vec!(0;10);
        let rr = rs.read(&mut buff[..]);
        assert!(rr.is_ok());
        assert!(rr.unwrap() == 4);
        sspawn.send(true);
      });
    };

    Ok(())
  };
 
  let a1c = a1.clone();
  let a2c = a2.clone();
  let at1 = Arc::new(t1);
  let at1c = at1.clone();
  let at2 = Arc::new(t2);
  let at2c = at2.clone();

  let g = thread::spawn(move|| {at1c.start(readhandler);});
  let g2 = thread::spawn(move|| {at2c.start(readhandler2);});
//  thread::sleep_ms(3000);
  let cres = at2.connectwith(a1, Duration::milliseconds(300));
  assert!(cres.as_ref().is_ok(),"{:?}", cres.as_ref().err());
  let (mut ws, mut ors) = cres.unwrap();
  if with_optional {
    assert!(ors.is_some());
  } else {
    assert!(ors.is_none());
  };
  
  let wr = ws.write(&mess_to[..]);
  assert!(wr.is_ok());
  assert!(ws.flush().is_ok());

  match ors {
    Some (ref mut rs) => {
      let mut buff = vec!(0;10);
      let rr = rs.read(&mut buff[..]);
      assert!(rr.unwrap() == 4);
    },
    // test in read handler
    None => {
      let cres = at1.connectwith(a2, Duration::milliseconds(300));
      assert!(cres.is_ok());
      let mut ws = cres.unwrap().0;
      let wr = ws.write(&mess_from[..]);
      assert!(wr.is_ok());
      assert!(ws.flush().is_ok());
    },
  };

  let wr = ws.write(&mess_to_2[..]);
  assert!(wr.is_ok());
  assert!(ws.flush().is_ok());

  // first message
  r.recv();
  // second message
  r.recv();
  if !with_optional {
    r2.recv();
  } else {()};
}

pub fn sending<A : Address, T : Transport<Address=A>> (t1 : T, t2 : T, a1 : &A, a2 : &A) {
  if t1.do_spawn_rec() == (true,true) && t1.do_spawn_rec() == (true,true) {
  } else {
    error!("transport test was skipped due to incompatible transport behaviour");
  }
}
