//! Test primitives for transports.

extern crate byteorder;
extern crate mio;
extern crate slab;
use std::cmp::max;
use std::cmp::min;
use std::thread;
use std::time::Duration as StdDuration; 
use time::Duration;
use std::sync::mpsc;
use mydht_base::transport::{Transport,Address,ReaderHandle,Registerable,ReadTransportStream,WriteTransportStream};
//use mydht_base::transport::{SpawnRecMode};
//use std::io::Result as IoResult;
//use mydht_base::mydhtresult::Result;
use std::mem;
use std::io::{
  Write,
  Read,
  Error as IoError,
  Result as IoResult,
};
use std::sync::Arc;
use mydht_base::mydhtresult::{
  Result,
  Error,
  ErrorKind as MdhtErrorKind,
  ErrorLevel as MdhtErrorLevel,
};
use self::mio::{
  Poll,
  PollOpt,
  Events,
  Token,
  Ready,
  Registration,
  SetReadiness,
};
use self::slab::{
  Slab,
};
use std::sync::mpsc::{
  Receiver,
  TryRecvError,
  Sender,
  channel,
};
use std::sync::atomic::{
  AtomicUsize,
  AtomicBool,
  Ordering,
};
use self::byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

const LISTENER : Token = Token(0);
const MPSC_TEST : Token = Token(1);
const START_STREAM_IX : usize = 2;


pub struct MpscRec<T> {
  mpsc : Receiver<T>,
  reg : Registration,
}
impl<T> MpscRec<T> {
  pub fn recv(&self) -> Result<T> {
    match self.mpsc.try_recv() {
      Ok(t) => Ok(t),
      Err(e) => if let TryRecvError::Empty = e {
        Err(Error("".to_string(), MdhtErrorKind::ExpectedError,None))
      } else {
        Err(Error(format!("{}",e), MdhtErrorKind::ChannelRecvError,Some(Box::new(e))))
      },
    }
  }
}
impl<T> Registerable for MpscRec<T> {
  fn register(&self, p : &Poll, t : Token, r : Ready, po : PollOpt) -> Result<bool> {
    p.register(&self.reg,t,r,po)?;
    Ok(true)
  }
  fn reregister(&self, p : &Poll, t : Token, r : Ready, po : PollOpt) -> Result<bool> {
    p.reregister(&self.reg,t,r,po)?;
    Ok(true)
  }
}
pub struct MpscSend<T> {
  mpsc : Sender<T>,
  set_ready : SetReadiness,
}
impl<T> MpscSend<T> {
  pub fn send(&self, t : T) -> Result<()> {
    self.mpsc.send(t)?;
    self.set_ready.set_readiness(Ready::readable())?;
    Ok(())
  }
}
pub fn channel_reg<T> () -> (MpscSend<T>,MpscRec<T>) {
    let (s,r) = channel();
    let (reg,sr) = Registration::new2();
    (MpscSend {
      mpsc : s,
      set_ready : sr,
    }, MpscRec {
      mpsc : r,
      reg : reg,
    })
}

/// TODO should be a enum with os (fourth first option are exclusive)
struct CacheEntry<T : Transport, RR, WR> {
  ors : Option<T::ReadStream>,
  ows : Option<T::WriteStream>,
  rr : Option<RR>,
  wr : Option<WR>,
  os : Option<usize>,
  ad : T::Address,
}

#[inline]
fn spawn_loop_async<M : 'static + Send, TC, T : Transport, RR, WR, FR, FW>(transport : T, read_cl : FR, write_cl : FW, controller : TC, ended : Arc<AtomicUsize>) -> Result<MpscSend<M>> 
where 
  FR : 'static + Send + Fn(Option<RR>,Option<T::ReadStream>,&T,Arc<AtomicUsize>) -> Result<RR>,
  FW : 'static + Send + Fn(Option<WR>,Option<T::WriteStream>,&T) -> Result<WR>,
  //for<'a> TC : 'static + Send + Fn(M, &'a mut Slab<CacheEntry<T,RR,WR>>) -> Result<()>,
  TC : 'static + Send + Fn(M, &mut Slab<CacheEntry<T,RR,WR>>,&T) -> Result<()>,
{

  let (sender,receiver) = channel_reg();
  thread::spawn(move||
    loop_async(receiver, transport,read_cl, write_cl, controller,ended).unwrap());
  Ok(sender)
}

/// currently only for full async (listener, read stream and write stream)
fn loop_async<M, TC, T : Transport, RR, WR, FR, FW>(receiver : MpscRec<M>, transport : T,read_cl : FR, write_cl : FW, controller : TC,ended : Arc<AtomicUsize>) -> Result<()> 
where 
  FR : Fn(Option<RR>,Option<T::ReadStream>,&T,Arc<AtomicUsize>) -> Result<RR>,
  FW : Fn(Option<WR>,Option<T::WriteStream>,&T) -> Result<WR>,
  TC : Fn(M, &mut Slab<CacheEntry<T,RR,WR>>, &T) -> Result<()>,
{



  let mut cache : Slab<CacheEntry<T,RR,WR>> = Slab::new();
  let mut events = Events::with_capacity(1024);
  let poll = Poll::new()?;

  assert!(true == receiver.register(&poll, MPSC_TEST, Ready::readable(),
                      PollOpt::edge())?);
  assert!(true == transport.register(&poll, LISTENER, Ready::readable(),
                      PollOpt::edge())?);
  let mut iter = 0;
  loop {
    poll.poll(&mut events, None).unwrap();
      for event in events.iter() {
          match event.token() {
              LISTENER => {
                try_breakloop!(transport.accept(), "Transport accept failure : {}", 
                |(rs,ows,ad) : (T::ReadStream,Option<T::WriteStream>,T::Address)| -> Result<()> {
                  let wad = if ows.is_some() {
                    Some(ad.clone())
                  } else {
                    None
                  };
                let read_token = {
                  let read_entry = cache.vacant_entry();
                  let read_token = read_entry.key();
                  assert!(true == rs.register(&poll, Token(read_token + START_STREAM_IX), Ready::readable(),
                    PollOpt::edge())?);
                  read_entry.insert(CacheEntry {
                    ors : Some(rs),
                    ows : None,
                    rr : None,
                    wr : None,
                    //os : ow.map(|t|(t.0).0),
                    os : None,
                    ad : ad,
                  });
                  read_token
                };

                ows.map(|ws| -> Result<()> {
                  let write_token = {
                    let write_entry = cache.vacant_entry();
                    let write_token = write_entry.key();
                    assert!(true == ws.register(&poll, Token(write_token + START_STREAM_IX), Ready::writable(),
                      PollOpt::edge())?);
                    write_entry.insert(CacheEntry {
                      ors : None,
                      ows : Some(ws),
                      rr : None,
                      wr : None,
                      os : Some(read_token),
                      ad : wad.unwrap(),
                    });
                    write_token
                  };
                  cache[read_token].os = Some(write_token);
                  Ok(())
                }).unwrap_or(Ok(()))?;

                Ok(())
              });
              
              },
              MPSC_TEST => {
                try_breakloop!(receiver.recv(),"Loop mpsc recv error : {:?}", |t| controller(t, &mut cache, &transport));
//                assert!(true == receiver.reregister(&poll, MPSC_TEST, Ready::readable(),
//                      PollOpt::edge())?);
              },
              tok => if let Some(ca) = cache.get_mut(tok.0 - START_STREAM_IX) {

                if ca.ors.is_some() {
                  let ors = mem::replace(&mut ca.ors, None);
                  ca.rr = Some(read_cl(None,ors,&transport,ended.clone())?);
                } else if ca.ows.is_some() {
                  let ows = mem::replace(&mut ca.ows, None);
                  ca.wr = Some(write_cl(None,ows,&transport)?);
                } else if ca.rr.is_some() {
                  let rr = mem::replace(&mut ca.rr, None);
                  ca.rr = Some(read_cl(rr,None,&transport,ended.clone())?);
                } else if ca.wr.is_some() {
                  let wr = mem::replace(&mut ca.wr, None);
                  ca.wr = Some(write_cl(wr,None,&transport)?);
                } else {
                  unreachable!();
                }
              } else {
                panic!("Unregistered token polled");
              },
          }
      }
  };

  Ok(())
}



pub fn sync_tr_start<T : Transport,C>(transport : Arc<T>,c : C) -> Result<()> 
    where C : Send + 'static + Fn(T::ReadStream,Option<T::WriteStream>) -> Result<ReaderHandle>
{
    thread::spawn(move || {
     || -> Result<_> {
      try_infiniteloop!(transport.accept(), "Mpsc failure on client reception, ignoring : {}",|(rs, ows, _)| c(rs,ows) );
      Ok(())
     }().unwrap();
    });
    Ok(())
}
pub fn connect_rw_with_optional<A : Address, T : Transport<Address=A>> (t1 : T, t2 : T, a1 : &A, a2 : &A, with_optional : bool, async : bool)
{
//  assert!(t1.do_spawn_rec().1 == true); // managed so we can receive multiple message : test removed due to hybrid transport lik tcp_loop where it is usefull to test those properties
  let mess_to = "hello world".as_bytes();
  let mess_to_2 = "hello2".as_bytes();
  let mess_from = "pong".as_bytes();
  let (s,r) = mpsc::channel();
  let (s2,r2) = mpsc::channel();
  let readhandler = move |mut rs : T::ReadStream, ows : Option<T::WriteStream> | {
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
 
      sspawn.send(true).unwrap();
    });
    Ok(ReaderHandle::Thread(o))
  };
  let readhandler2 = move |mut rs : T::ReadStream, ows : Option<T::WriteStream> | {
    if with_optional {
      assert!(ows.is_some());
      Ok(ReaderHandle::Local) // not local but no thread started
    } else {
      assert!(ows.is_none());
      let sspawn = s2.clone();
      let o = thread::spawn(move|| {
        let mut buff = vec!(0;10);
        let rr = rs.read(&mut buff[..]);
        assert!(rr.unwrap() == 4);
        sspawn.send(true).unwrap();
      });
      Ok(ReaderHandle::Thread(o))
    }
  };
 
  //let a1c = a1.clone();
  //let a2c = a2.clone();
  let at1 = Arc::new(t1);
  let at1c = at1.clone();
  let at2 = Arc::new(t2);
  let at2c = at2.clone();

  if async {
    panic!("TODO implement async loop test");
  } else {
  //thread::spawn(move|| {at1c.start(readhandler).unwrap();});
  sync_tr_start(at1c,readhandler).unwrap();
  //thread::spawn(move|| {at2c.start(readhandler2).unwrap();});
  sync_tr_start(at2c,readhandler2).unwrap();
//  thread::sleep_ms(3000);
  }
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


  r.recv().unwrap();
  if !with_optional {
    r2.recv().unwrap();
  } else {()};
}



pub fn connect_rw_with_optional_non_managed<A : Address, T : Transport<Address=A>> (t1 : T, t2 : T, a1 : &A, a2 : &A, with_connect_rs : bool, with_recv_ws : bool, variant : bool, async : bool)
{
  let mess_to = "hello world".as_bytes();
  let mess_to_2 = "hello2".as_bytes();
  let mess_from = "pong".as_bytes();
  let (s,r) = mpsc::channel();
  let (s2,r2) = mpsc::channel();
  let readhandler = move |mut rs : T::ReadStream, ows : Option<T::WriteStream> | {
    if with_recv_ws {
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
        let rr2 = if variant {
          rs.read(&mut buff[0..1]) // we get only one byte (second message may be here)
        } else {
          rs.read(&mut buff[..])
        };
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
      sspawn.send(true).unwrap();
    });
    Ok(ReaderHandle::LocalTh(o))
  };
  let readhandler2 = move |mut rs : T::ReadStream, ows : Option<T::WriteStream> | {
    if with_recv_ws {
      assert!(ows.is_some());
      Ok(ReaderHandle::Local)
    } else {
      assert!(ows.is_none());
      let sspawn = s2.clone();
      let o = thread::spawn(move|| {
        let mut buff = vec!(0;10);
        let rr = rs.read(&mut buff[..]);
        assert!(rr.is_ok());
        assert!(rr.unwrap() == 4);
        sspawn.send(true).unwrap();
      });
      Ok(ReaderHandle::LocalTh(o))
    }
  };
 
  //let a1c = a1.clone();
  //let a2c = a2.clone();
  let at1 = Arc::new(t1);
  let at1c = at1.clone();
  let at2 = Arc::new(t2);
  let at2c = at2.clone();
  if async {
    panic!("TODO implement async loop test");
  } else {
//  thread::spawn(move|| {at1c.start(readhandler).unwrap();});
  sync_tr_start(at1c,readhandler).unwrap();
//  thread::spawn(move|| {at2c.start(readhandler2).unwrap();});
  sync_tr_start(at2c,readhandler2).unwrap();
//  thread::sleep_ms(3000);
  }
  let cres = at2.connectwith(a1, Duration::milliseconds(300));
  assert!(cres.as_ref().is_ok(),"{:?}", cres.as_ref().err());
  let (mut ws, mut ors) = cres.unwrap();
  if with_connect_rs {
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

  if !variant {
    r.recv().unwrap();// first message
  };
  let wr = ws.write(&mess_to_2[..]);
  assert!(wr.is_ok());
  assert!(ws.flush().is_ok());

  if variant {
    r.recv().unwrap();// first message
  };
 
  // second message
  r.recv().unwrap();
  if !with_recv_ws {
    r2.recv().unwrap();
  } else {()};
}


/// for testing purpose
#[derive(Deserialize,Serialize,Debug,PartialEq,Eq,Clone)]
pub struct LocalAdd (pub usize);

impl Address for LocalAdd {
  /*fn write_as_bytes<W:Write> (&self, w : &mut W) -> IoResult<()> {
    let fsize = self.0 as u64; 
    try!(w.write_u64::<LittleEndian>(fsize));
    Ok(())
  }
  fn read_as_bytes<R:Read> (r : &mut R) -> IoResult<Self> {

    let fsize = try!(r.read_u64::<LittleEndian>());
    Ok(LocalAdd(fsize as usize))
  }*/
 
}

pub fn reg_mpsc_recv_test<T : Transport>(t : T) {

  let transport_reg_r = |_ : Option<()>, _ : Option<T::ReadStream>, _ : &T, _| {Ok(())};
  let transport_reg_w = |_ : Option<()>, _ : Option<T::WriteStream>, _ : &T| {Ok(())};
  let current_ix = Arc::new(AtomicUsize::new(0));
  let next_it = Arc::new(AtomicBool::new(false));
  let sp_cix = current_ix.clone();
  let sp_nit = next_it.clone();
  let nbit = 3;
  let controller = move |ix : usize, _ : &mut Slab<CacheEntry<T,(),()>>, _ : &T| {
    let i = sp_cix.fetch_add(1,Ordering::Relaxed);
    assert_eq!(i, ix);
    if i == nbit -1 {
      let sp_nit2 = sp_nit.clone();
      thread::spawn(move ||{
        thread::sleep(StdDuration::from_millis(100));
        sp_nit2.store(true,Ordering::Relaxed)
      });
    }
    if i == (2 * nbit) -1 {
      sp_nit.store(false,Ordering::Relaxed)
    }

    Ok(())
  };

  let sender = spawn_loop_async(t, transport_reg_r, transport_reg_w, controller, current_ix.clone()).unwrap();
  for i in 0 .. nbit {
      sender.send(i).unwrap()
  }
  while !next_it.load(Ordering::Relaxed) {
  }
  for i in nbit .. nbit + nbit {
      sender.send(i).unwrap()
  }
  while next_it.load(Ordering::Relaxed) {
  }
  let i = current_ix.load(Ordering::Relaxed);
  assert!(i == 2 * nbit);

}
 
pub fn reg_connect_2<T : Transport>(fromadd : &T::Address, t : T, c0 : T, c1 : T) {

  let content = [5];
  
  let controller = move |_ : (), _ : &mut Slab<CacheEntry<T,(),()>>, _ : &T| { Ok(()) };

  let notused = Arc::new(AtomicUsize::new(0));
  let next_it = Arc::new(AtomicBool::new(false));
  let sp_nit = next_it.clone();
  let transport_reg_w = |_ : Option<()>, _ : Option<T::WriteStream>, _ : &T| {Ok(())};

  let transport_reg_r = move |_ : Option<()>, ors : Option<T::ReadStream>, _ : &T,_| {

    // just check connection ok
    let mut buf = vec![0;1];
    ors.map(|mut rs|rs.read(&mut buf).unwrap());

    assert!(buf[0] == 5);
    buf[0] = 0;
    let sp_nit2 = sp_nit.clone();
    thread::spawn(move ||{
        thread::sleep(StdDuration::from_millis(100));
        // invert value
        sp_nit2.fetch_xor(true,Ordering::Relaxed)
    });

    Ok(())
  };
  
  spawn_loop_async(t, transport_reg_r, transport_reg_w, controller,notused).unwrap();

  let mut ws0 = c0.connectwith(fromadd, Duration::seconds(5)).unwrap();

  ws0.0.write(&content[..]).unwrap();
  
  while !next_it.load(Ordering::Relaxed) { }

  let mut ws1 = c1.connectwith(fromadd, Duration::seconds(5)).unwrap();
  ws1.0.write(&content[..]).unwrap();

  while next_it.load(Ordering::Relaxed) { }
}


/// test commands
pub enum SimpleLoopCommand<T : Transport> {
  ConnectWith(T::Address),
  SendTo(T::Address, Vec<u8>, usize),
  Expect(T::Address, Vec<u8>,Arc<AtomicUsize>),
}
struct WsState<'a,WS : WriteTransportStream> (&'a mut(Vec<u8>,usize,WS));
impl<'a,WS : WriteTransportStream> WsState<'a,WS> {
  pub fn send(&mut self, bufsize : usize) -> Result<()> {
    let &mut WsState( &mut (ref mut c, ref mut i,ref mut ws)) = self;

    let mut clen = c.len();
    loop {

      if *i == clen {
        return Ok(());
      }

      if *i > clen {
        panic!("i : {}, c.len() : {}",i,clen);
      }
      let e : Result<usize> = ws.write(&c[*i..min(*i + bufsize,clen)]).map_err(|ee|ee.into());
      let br = if let Some(nbr) = try_ignore!(e,"Loop send error : {:?}") {
        *i += nbr;
        nbr == 0
      } else {
        true
      };
      if *i == clen {
        c.truncate(0);
        *i = 0;
        clen = 0;
      }
      if br { break }
    }
    Ok(())
  }
}
struct RsState<'a, RS : ReadTransportStream> (&'a mut(Vec<u8>,usize,RS));
impl<'a,RS : ReadTransportStream> RsState<'a,RS> {
  pub fn recv(&mut self, bufsize : usize) -> Result<()> {
    let &mut RsState( &mut (ref mut c, ref mut i,ref mut rs)) = self;
    let clen = c.len();
    loop {
      if *i == clen {
        return Ok(());
      }

      let e : Result<usize> = rs.read(&mut c[*i..min(*i + bufsize,clen)]).map_err(|ee|ee.into());
      let br = if let Some(nbr) = try_ignore!(e,"Loop recv error : {:?}") {
        *i += nbr;
        nbr == 0
      } else {
        true
      };
      if br { break }
    }
    Ok(())
  }
}
impl<T : Transport> CacheEntry<T, (Vec<u8>,usize,T::ReadStream), (Vec<u8>,usize,T::WriteStream)> {
  /// non blocking send up to wouldblock
  pub fn send(&mut self, bufsize : usize) -> Result<()> {
    WsState(self.wr.as_mut().unwrap()).send(bufsize)// panic if wrong state
  }
  /// non blocking recv up to wouldblock
  pub fn recv(&mut self, bufsize : usize) -> Result<()> {
    RsState(self.rr.as_mut().unwrap()).recv(bufsize)// panic if wrong state
  }
}


fn simple_command_controller<T : Transport>(command : SimpleLoopCommand<T>, cache : &mut Slab<CacheEntry<T,(Vec<u8>,usize,T::ReadStream),(Vec<u8>,usize,T::WriteStream)>>, t : &T) -> Result<()> {
    match command {
      SimpleLoopCommand::ConnectWith(ad) => {
        let c = cache.iter().any(|(_,e)|e.ad == ad && (e.ows.is_some() || e.wr.is_some()));
        if c {
          // do nothing (no connection closed here)
        } else {
          let (ws,ors) = t.connectwith(&ad, Duration::seconds(5)).unwrap();
          let ork = if ors.is_some() {
            Some(cache.insert(CacheEntry {
              ors : ors,
              ows : None,
              rr : None,
              wr : None,
              os : None,
              ad : ad.clone(),
            }))
          } else {
            // try update
            cache.iter().position(|(_,e)|e.ad == ad && (e.ors.is_some() || e.rr.is_some()))
          };
          let vkey = cache.insert(CacheEntry {
            ors : None,
            ows : Some(ws),
            rr : None,
            wr : None,
            os : ork,
            ad : ad,
          });
          ork.map(|rix|cache[rix].os=Some(vkey));
        };
      },
      SimpleLoopCommand::SendTo(ad, content, bufsize) => {
        let (_, ref mut c) = cache.iter_mut().find(|&(_, ref e)|e.ad == ad && (e.ows.is_some() || e.wr.is_some())).unwrap();
        if c.ows.is_some() {
          let ws = mem::replace(&mut c.ows,None).unwrap();
          c.wr = Some((content, 0, ws));
        } else if c.wr.is_some() {
          c.wr.as_mut().map(|a|a.0.extend_from_slice(&content[..]));
        } else {
          panic!("failed write : not connected")
        };
        c.send(bufsize)?;
      },
      SimpleLoopCommand::Expect(ad, expect,ended) => {
//        let (_, ref mut e) = cache.iter_mut().find(|&(_, ref e)|(e.rr.is_some())).unwrap();
//        panic!("e : {:?}, par : {:?}",e.ad,ad);
 //       let (_, ref mut e) = cache.iter_mut().find(|&(_, ref e)|e.ad == ad && (e.rr.is_some())).unwrap();
        // no filter on address as it is write stream address and not listener
        for (_, ref mut e) in cache.iter_mut() {
          if e.rr.is_some() {
            let &mut (ref mut c, _,_) = e.rr.as_mut().unwrap();
            if c[..expect.len()] == expect[..] {
              *c = c.split_off(expect.len());
              ended.fetch_add(1,Ordering::Relaxed);
              return Ok(());
            }
          }
        }
        panic!("missing expect")
      },
    };
    Ok(()) 
}
fn transport_reg_w_testing<T : Transport>(ocw : Option<(Vec<u8>,usize,T::WriteStream)>, ows : Option<T::WriteStream>, _ : &T,bufsize : usize) -> Result<(Vec<u8>,usize,T::WriteStream)> {
  let mut state = match ocw {
    Some(s) => s,
    None => match ows {
      Some(t) => (Vec::new(),0,t),
      None => panic!("not a send state"),
    },
  };
  WsState(&mut state).send(bufsize)?;
  Ok(state)
}
fn transport_reg_r_testing<T : Transport>(ocr : Option<(Vec<u8>,usize,T::ReadStream)>, ors : Option<T::ReadStream>, _ : &T, bufsize : usize, contentsize : usize, ended : Arc<AtomicUsize>) -> Result<(Vec<u8>,usize,T::ReadStream)> {
  let mut state = match ocr {
    Some(s) => s,
    None => match ors {
      Some(t) => (vec![0;contentsize],0,t),
      None => panic!("not a recv state"),
    },
  };
  RsState(&mut state).recv(bufsize)?;
  if state.1 == contentsize {
    ended.fetch_add(1,Ordering::Relaxed);
  }
  Ok(state)
}
  

pub fn reg_rw_testing<T : Transport>(fromadd : T::Address, tfrom : T, toadd : T::Address, tto : T, content_size : usize, read_buf_size : usize, write_buf_size : usize,nbmess:usize) {
  let mut contents = Vec::with_capacity(nbmess);
  for im in 0 .. nbmess {
    let mut content = vec![0;content_size];
    for i in 0..content_size {
      content[i]= (i + im) as u8;
    }
    contents.push(content);
  }
  
  let ended_expect = Arc::new(AtomicUsize::new(0));
  //spawn_loop_async(tto, transport_reg_r1, transport_reg_w1, controller1).unwrap();
  //spawn_loop_async(tfrom, transport_reg_r_testing, transport_reg_w_testing, controller2).unwrap();
  let tomsg = spawn_loop_async(tto, move |a,b,c,cended| transport_reg_r_testing(a,b,c,read_buf_size,content_size * nbmess,cended), move |a,b,c| transport_reg_w_testing(a,b,c,write_buf_size), simple_command_controller,ended_expect.clone()).unwrap();
  let frommsg = spawn_loop_async(tfrom, move |a,b,c,cended| transport_reg_r_testing(a,b,c,read_buf_size,content_size * nbmess,cended), move |a,b,c| transport_reg_w_testing(a,b,c,write_buf_size), simple_command_controller,ended_expect.clone()).unwrap();

  // send and receive commands
  frommsg.send(SimpleLoopCommand::ConnectWith(toadd.clone())).unwrap();
  for content in contents.iter() {
    frommsg.send(SimpleLoopCommand::SendTo(toadd.clone(),content.clone(),write_buf_size)).unwrap();
  }

  // TODO timeout panic thread
  while ended_expect.load(Ordering::Relaxed) != 1 { }

  ended_expect.store(0,Ordering::Relaxed);
  for content in contents.iter() {
    tomsg.send(SimpleLoopCommand::Expect(fromadd.clone(),content.clone(),ended_expect.clone())).unwrap();
  }
  while ended_expect.load(Ordering::Relaxed) != nbmess { }
}

