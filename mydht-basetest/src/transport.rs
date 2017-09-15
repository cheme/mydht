//! Test primitives for transports.
extern crate byteorder;
extern crate mio;
extern crate slab;
extern crate coroutine;
extern crate futures;
extern crate futures_cpupool;
use self::futures::Future;
use self::futures::future::ok;
use self::futures::future::FutureResult;
use self::futures_cpupool::CpuPool;
use self::futures_cpupool::CpuFuture;
use std::cell::RefCell;
use std::cell::RefMut;
use std::rc::Rc;
use std::thread::JoinHandle;
use self::coroutine::asymmetric::{
  Handle as CoRHandle,
  Coroutine,
};
use std::cmp::max;
use std::cmp::min;
use std::thread;
use std::time::Duration as StdDuration; 
use time::Duration;
use std::sync::mpsc;
use mydht_base::transport::{
  Transport,
  Address,
  ReaderHandle,
  Registerable,
  ReadTransportStream,
  WriteTransportStream,
  SlabEntry,
  SlabEntryState,
};
//use mydht_base::transport::{SpawnRecMode};
//use std::io::Result as IoResult;
//use mydht_base::mydhtresult::Result;
use std::mem;
use std::io::{
  Write,
  Read,
  Error as IoError,
  ErrorKind as IoErrorKind,
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


#[inline]
fn spawn_loop_async<M : 'static + Send, TC, T : Transport, RR, WR, FR, FW>(transport : T, read_cl : FR, write_cl : FW, controller : TC, ended : Arc<AtomicUsize>, exp : Vec<u8>, connect_done : Arc<AtomicUsize>, cpp : Arc<CpuPool>) -> Result<MpscSend<M>> 
where 
  FR : 'static + Send + Fn(Option<RR>,Option<T::ReadStream>,&T,Arc<AtomicUsize>,Vec<u8>,Arc<CpuPool>) -> Result<RR>,
  FW : 'static + Send + Fn(Option<WR>,Option<T::WriteStream>,&T,Arc<CpuPool>) -> Result<WR>,
  //for<'a> TC : 'static + Send + Fn(M, &'a mut Slab<SlabEntry<T,RR,WR>>) -> Result<()>,
  TC : 'static + Send + Fn(M, &mut Slab<SlabEntry<T,RR,WR,(),T::Address>>,&T,Arc<CpuPool>) -> Result<()>,
{

  let (sender,receiver) = channel_reg();
  thread::spawn(move||
    loop_async(receiver, transport,read_cl, write_cl, controller,ended, exp, connect_done, cpp).unwrap());
  Ok(sender)
}

/// currently only for full async (listener, read stream and write stream)
fn loop_async<M, TC, T : Transport, RR, WR, FR, FW>(receiver : MpscRec<M>, transport : T,read_cl : FR, write_cl : FW, controller : TC,ended : Arc<AtomicUsize>, exp : Vec<u8>, connect_done : Arc<AtomicUsize>,cpp : Arc<CpuPool>) -> Result<()> 
where 
  FR : Fn(Option<RR>,Option<T::ReadStream>,&T,Arc<AtomicUsize>,Vec<u8>,Arc<CpuPool>) -> Result<RR>,
  FW : Fn(Option<WR>,Option<T::WriteStream>,&T,Arc<CpuPool>) -> Result<WR>,
  TC : Fn(M, &mut Slab<SlabEntry<T,RR,WR,(),T::Address>>, &T,Arc<CpuPool>) -> Result<()>,
{



  let mut cache : Slab<SlabEntry<T,RR,WR,(),T::Address>> = Slab::new();
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
                  read_entry.insert(SlabEntry {
                    state : SlabEntryState::ReadStream(rs,None),
                    os : None,
                    peer : Some(ad),
                  });
                  read_token
                };

                ows.map(|ws| -> Result<()> {
                  let write_token = {
                    let write_entry = cache.vacant_entry();
                    let write_token = write_entry.key();
                    assert!(true == ws.register(&poll, Token(write_token + START_STREAM_IX), Ready::writable(),
                      PollOpt::edge())?);
                    write_entry.insert(SlabEntry {
                      state : SlabEntryState::WriteStream(ws,()),
                      os : Some(read_token),
                      peer : wad,
                    });
                    write_token
                  };
                  cache[read_token].os = Some(write_token);
                  Ok(())
                }).unwrap_or(Ok(()))?;
                connect_done.fetch_add(1,Ordering::Relaxed);
                Ok(())
              });
              
              },
              MPSC_TEST => {
                try_breakloop!(receiver.recv(),"Loop mpsc recv error : {:?}", |t| controller(t, &mut cache, &transport,cpp.clone()));
//                assert!(true == receiver.reregister(&poll, MPSC_TEST, Ready::readable(),
//                      PollOpt::edge())?);
              },
              tok => if let Some(ca) = cache.get_mut(tok.0 - START_STREAM_IX) {
                match ca.state {
                  SlabEntryState::ReadStream(_,_) => {
                    let state = mem::replace(&mut ca.state, SlabEntryState::Empty);
                    if let SlabEntryState::ReadStream(rs,_) = state {
                      ca.state = SlabEntryState::ReadSpawned(read_cl(None,Some(rs),&transport,ended.clone(),exp.clone(),cpp.clone())?);
                    }
                  },
                  SlabEntryState::WriteStream(_,_) => {
                    let state = mem::replace(&mut ca.state, SlabEntryState::Empty);
                    if let SlabEntryState::WriteStream(ws,_) = state {
                      ca.state = SlabEntryState::WriteSpawned(write_cl(None,Some(ws),&transport,cpp.clone())?);
                    }
                  },
                  SlabEntryState::ReadSpawned(_) => {
                    let state = mem::replace(&mut ca.state, SlabEntryState::Empty);
                    if let SlabEntryState::ReadSpawned(rr) = state {
                      ca.state = SlabEntryState::ReadSpawned(read_cl(Some(rr),None,&transport,ended.clone(),exp.clone(),cpp.clone())?);
                    }
                  },
                  SlabEntryState::WriteSpawned(_) => {
                    let state = mem::replace(&mut ca.state, SlabEntryState::Empty);
                    if let SlabEntryState::WriteSpawned(wr) = state {
                      ca.state = SlabEntryState::WriteSpawned(write_cl(Some(wr),None,&transport,cpp.clone())?);
                    }
                  },
                  SlabEntryState::Empty => {
                    unreachable!()
                  },
                }
              } else {
                panic!("Unregistered token polled");
              },
          }
      }
  };

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

  let transport_reg_r = |_ : Option<()>, _ : Option<T::ReadStream>, _ : &T,_,_,_| {Ok(())};
  let transport_reg_w = |_ : Option<()>, _ : Option<T::WriteStream>, _ : &T,_| {Ok(())};
  let current_ix = Arc::new(AtomicUsize::new(0));
  let next_it = Arc::new(AtomicBool::new(false));
  let sp_cix = current_ix.clone();
  let sp_nit = next_it.clone();
  let nbit = 3;
  let controller = move |ix : usize, _ : &mut Slab<SlabEntry<T,(),(),(),T::Address>>, _ : &T,_| {
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

  let connect_done = Arc::new(AtomicUsize::new(0));
  let sender = spawn_loop_async(t, transport_reg_r, transport_reg_w, controller, current_ix.clone(),Vec::new(),connect_done,Arc::new(CpuPool::new(1))).unwrap();
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
  
  let controller = move |_ : (), _ : &mut Slab<SlabEntry<T,(),(),(),T::Address>>, _ : &T,_| { Ok(()) };

  let notused = Arc::new(AtomicUsize::new(0));
  let next_it = Arc::new(AtomicBool::new(false));
  let sp_nit = next_it.clone();
  let transport_reg_w = |_ : Option<()>, _ : Option<T::WriteStream>, _ : &T,_| {Ok(())};

  let transport_reg_r = move |_ : Option<()>, ors : Option<T::ReadStream>, _ : &T,_,_,_| {

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
  
  let connect_done = Arc::new(AtomicUsize::new(0));
  spawn_loop_async(t, transport_reg_r, transport_reg_w, controller,notused,Vec::new(),connect_done.clone(),Arc::new(CpuPool::new(1))).unwrap();

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
struct SlabEntryInnerState<'a,A : 'a + Transport,B : 'a,C : 'a,D : 'a>(&'a mut SlabEntry<A,B,C,(),D>);
impl<'a,T : Transport> SlabEntryInnerState<'a,T, (Vec<u8>,usize,T::ReadStream), (Vec<u8>,usize,T::WriteStream),T::Address> {
  /// non blocking send up to wouldblock
  pub fn send(&mut self, bufsize : usize) -> Result<()> {
    match self.0.state {
      SlabEntryState::WriteSpawned(ref mut wr) => WsState(wr).send(bufsize),
      _ => panic!("wrong state for send"),
    }
  }
  /// non blocking recv up to wouldblock
  pub fn recv(&mut self, bufsize : usize) -> Result<()> {
    match self.0.state {
      SlabEntryState::ReadSpawned(ref mut rr) => RsState(rr).recv(bufsize),
      _ => panic!("wrong state for recv"),
    }
  }
}
fn simple_command_controller_cpupool<T : Transport>(command : SimpleLoopCommand<T>, cache : &mut Slab<SlabEntry<T,CpuFuture<(),Error>,CpuFuture<T::WriteStream,Error>,(),T::Address>>, t : &T, cpupool : Arc<CpuPool>) -> Result<()> {
    match command {
      SimpleLoopCommand::ConnectWith(ad) => {
        let c = cache.iter().any(|(_,e)|e.peer.as_ref() == Some(&ad) && match e.state {
          SlabEntryState::WriteStream(_,_) | SlabEntryState::WriteSpawned(_) => true,
          _ => false,
        });
        if c {
          // do nothing (no connection closed here)
        } else {
          let (ws,ors) = t.connectwith(&ad, Duration::seconds(5)).unwrap();
          let ork = if ors.is_some() {
            Some(cache.insert(SlabEntry {
              state : SlabEntryState::ReadStream(ors.unwrap(),None),
              os : None,
              peer : Some(ad.clone()),
            }))
          } else {
            // try update
            cache.iter().position(|(_,e)|e.peer.as_ref() == Some(&ad) && match e.state {
              SlabEntryState::ReadStream(_,_) | SlabEntryState::ReadSpawned(_) => true,
              _ => false,
            })
          };
          let vkey = cache.insert(SlabEntry {
            state : SlabEntryState::WriteStream(ws,()),
            os : ork,
            peer : Some(ad),
          });
          ork.map(|rix|cache[rix].os=Some(vkey));
        };
      },
      SimpleLoopCommand::SendTo(ad, content, bufsize) => {
        let (_, ref mut c) = cache.iter_mut().find(|&(_, ref e)|e.peer.as_ref() == Some(&ad) && match e.state {
          SlabEntryState::WriteStream(_,_) | SlabEntryState::WriteSpawned(_) => true,
          _ => false,
        }).unwrap();
        match c.state {
          SlabEntryState::WriteStream (_,_) => {
            let state = mem::replace(&mut c.state, SlabEntryState::Empty);
            if let SlabEntryState::WriteStream(mut ws,()) = state {
              let f : CpuFuture<T::WriteStream,Error> = cpupool.spawn_fn(move|| {
                WriteCpuPool(&mut ws).write_all(&content[..]).unwrap();// TODO replace by implementaiton with right size buffer
                ok(ws)
              });
              c.state = SlabEntryState::WriteSpawned(f);
            }
          },
          SlabEntryState::WriteSpawned (_) => {
            let state = mem::replace(&mut c.state, SlabEntryState::Empty);
            if let SlabEntryState::WriteSpawned(mut wr) = state {
              let f : CpuFuture<T::WriteStream,Error> = cpupool.spawn(wr.and_then(|mut w| {
                let mut incontent = content;
                WriteCpuPool(&mut w).write_all(&incontent[..]).unwrap();// TODO replace by implementaiton with right size buffer
                ok(w)
              }));
              c.state = SlabEntryState::WriteSpawned(f);
            }
          },
          _ => panic!("failed write : not connected"),
        };
      },
      SimpleLoopCommand::Expect(ad, expect,ended) => {
        panic!("no expect");
      },
    };
    Ok(()) 
}


fn simple_command_controller_threadpark<T : Transport>(command : SimpleLoopCommand<T>, cache : &mut Slab<SlabEntry<T,JoinHandle<()>,(Sender<Vec<u8>>,JoinHandle<()>),(),T::Address>>, t : &T) -> Result<()> {
    match command {
      SimpleLoopCommand::ConnectWith(ad) => {
        let c = cache.iter().any(|(_,e)|e.peer.as_ref() == Some(&ad) && match e.state {
          SlabEntryState::WriteStream(_,()) | SlabEntryState::WriteSpawned(_) => true,
          _ => false,
        });
        if c {
          // do nothing (no connection closed here)
        } else {
          let (ws,ors) = t.connectwith(&ad, Duration::seconds(5)).unwrap();
          let ork = if ors.is_some() {
            Some(cache.insert(SlabEntry {
              state : SlabEntryState::ReadStream(ors.unwrap(),None),
              os : None,
              peer : Some(ad.clone()),
            }))
          } else {
            // try update
            cache.iter().position(|(_,e)|e.peer.as_ref() == Some(&ad) && match e.state {
              SlabEntryState::ReadStream(_,_) | SlabEntryState::ReadSpawned(_) => true,
              _ => false,
            })
          };
          let vkey = cache.insert(SlabEntry {
            state : SlabEntryState::WriteStream(ws,()),
            os : ork,
            peer : Some(ad),
          });
          ork.map(|rix|cache[rix].os=Some(vkey));
        };
      },
      SimpleLoopCommand::SendTo(ad, content, bufsize) => {
        let (_, ref mut c) = cache.iter_mut().find(|&(_, ref e)|e.peer.as_ref() == Some(&ad) && match e.state {
          SlabEntryState::WriteStream(_,_) | SlabEntryState::WriteSpawned(_) => true,
          _ => false,
        }).unwrap();
        match c.state {
          SlabEntryState::WriteStream(_,_) => {
            let state = mem::replace(&mut c.state, SlabEntryState::Empty);
            if let SlabEntryState::WriteStream(ws,_) = state {
              let mut ces = thread_park_w_spawn(ws,bufsize);
              ces.0.send(content).unwrap();
              /*if ces.2.is_some() {
                if let Some(ref isstart) = ces.2.as_ref() {
                  // block until thread start to avoid racy situation when starting thread -> block
                  // loop on thread creation : in code use Builder for thread spawn to catch error
                  loop {
                    if isstart.load(Ordering::Relaxed) {
                      break;
                    } else {
                      thread::yield_now();
                    }
                  }
                }
                ces.2 = None;
              }*/
              ces.1.thread().unpark();
              c.state = SlabEntryState::WriteSpawned(ces); 
            }
          },
          SlabEntryState::WriteSpawned (ref mut ca) => {
            ca.0.send(content).unwrap();
            ca.1.thread().unpark();
          },
          _ => panic!("failed write : not connected"),
        };
      },
      SimpleLoopCommand::Expect(ad, expect,ended) => {
        panic!("no expect");
      },
    };
    Ok(()) 
}

fn simple_command_controller_corout<T : Transport>(command : SimpleLoopCommand<T>, cache : &mut Slab<SlabEntry<T,CoRHandle,(Rc<RefCell<Vec<u8>>>,CoRHandle),(),T::Address>>, t : &T,_ : Arc<CpuPool>) -> Result<()> {
    match command {
      SimpleLoopCommand::ConnectWith(ad) => {
        let c = cache.iter().any(|(_,e)|e.peer.as_ref() == Some(&ad) && match e.state {
          SlabEntryState::WriteStream(_,_) | SlabEntryState::WriteSpawned(_) => true,
          _ => false,
        });
        if c {
          // do nothing (no connection closed here)
        } else {
          let (ws,ors) = t.connectwith(&ad, Duration::seconds(5)).unwrap();
          let ork = if ors.is_some() {
            Some(cache.insert(SlabEntry {
              state : SlabEntryState::ReadStream(ors.unwrap(),None),
              os : None,
              peer : Some(ad.clone()),
            }))
          } else {
            // try update
            cache.iter().position(|(_,e)|e.peer.as_ref() == Some(&ad) && match e.state {
              SlabEntryState::ReadStream(_,_) | SlabEntryState::ReadSpawned(_) => true,
              _ => false,
            })
          };
          let vkey = cache.insert(SlabEntry {
            state : SlabEntryState::WriteStream(ws,()),
            os : ork,
            peer : Some(ad),
          });
          ork.map(|rix|cache[rix].os=Some(vkey));
        };
      },
      SimpleLoopCommand::SendTo(ad, content, bufsize) => {
        let (_, ref mut c) = cache.iter_mut().find(|&(_, ref e)|e.peer.as_ref() == Some(&ad)  && match e.state {
          SlabEntryState::WriteStream(_,_) | SlabEntryState::WriteSpawned(_) => true,
          _ => false,
        }).unwrap();
        match c.state {
          SlabEntryState::WriteStream (_,_) => {
            corout_ows(&mut c.state, content, bufsize);
          },
          SlabEntryState::WriteSpawned (ref mut wr) => {
            wr.0.borrow_mut().extend_from_slice(&content[..]);
            wr.1.resume(0).unwrap();
          },
          _ => panic!("failed write : not connected"),
        };
      },
      SimpleLoopCommand::Expect(ad, expect,ended) => {
        panic!("no expect");
      },
    };
    Ok(()) 
}

fn corout_ows<T : Transport>(cstate : &mut SlabEntryState<T,CoRHandle,(Rc<RefCell<Vec<u8>>>,CoRHandle),(),T::Address>, content : Vec<u8>, bufsize : usize) {
  let state = mem::replace(cstate,SlabEntryState::Empty);
  if let SlabEntryState::WriteStream(ws,_) = state {
    let st = corout_ows2(ws, content,bufsize);
    *cstate = SlabEntryState::WriteSpawned(st);
  }
}

fn corout_ows2<TW : 'static + Write>(mut ws : TW, content : Vec<u8>, bufsize : usize) 
  -> (Rc<RefCell<Vec<u8>>>,CoRHandle) {
    let buf = Rc::new(RefCell::new(content));
    let bc = buf.clone();
    let mut cohandle = Coroutine::spawn(move|corout,_| {
      let mut it = 0;
      loop {
        let mut blen = 0;
        while blen == 0 {
          blen = buf.borrow().len();
          if blen == 0 {
            corout.yield_with(0);
          }
        }
        let iw = WriteCorout(&mut ws,corout).write(&buf.borrow()[..min(bufsize,blen)]).unwrap();
        it += 1;
        if iw > 0 {
          let mut bmut = buf.borrow_mut();
          bmut.reverse();
          let l = bmut.len(); // need recount as write could switch context
          bmut.truncate(l - iw);
          bmut.reverse();
          //RefMut::map(bmut,|rm|rm = a);
        }
      }

    });
    cohandle.resume(0).unwrap();
    (bc,cohandle)
}

fn simple_command_controller<T : Transport>(command : SimpleLoopCommand<T>, cache : &mut Slab<SlabEntry<T,(Vec<u8>,usize,T::ReadStream),(Vec<u8>,usize,T::WriteStream),(),T::Address>>, t : &T,_ : Arc<CpuPool>) -> Result<()> {
    match command {
      SimpleLoopCommand::ConnectWith(ad) => {
        let c = cache.iter().any(|(_,e)|e.peer.as_ref() == Some(&ad) && match e.state {
          SlabEntryState::WriteStream(_,_) | SlabEntryState::WriteSpawned(_) => true,
          _ => false,
        });
        if c {
          // do nothing (no connection closed here)
        } else {
          let (ws,ors) = t.connectwith(&ad, Duration::seconds(5)).unwrap();
          let ork = if ors.is_some() {
            Some(cache.insert(SlabEntry {
              state : SlabEntryState::ReadStream(ors.unwrap(),None),
              os : None,
              peer : Some(ad.clone()),
            }))
          } else {
            // try update
            cache.iter().position(|(_,e)|e.peer.as_ref() == Some(&ad) && match e.state {
              SlabEntryState::ReadStream(_,_) | SlabEntryState::ReadSpawned(_) => true,
              _ => false,
            })
          };
          let vkey = cache.insert(SlabEntry {
            state : SlabEntryState::WriteStream(ws,()),
            os : ork,
            peer : Some(ad),
          });
          ork.map(|rix|cache[rix].os=Some(vkey));
        };
      },
      SimpleLoopCommand::SendTo(ad, content, bufsize) => {
        let (_, ref mut c) = cache.iter_mut().find(|&(_, ref e)|e.peer.as_ref() == Some(&ad) && match e.state {
          SlabEntryState::WriteStream(_,_) | SlabEntryState::WriteSpawned(_) => true,
          _ => false,
        }).unwrap();
        match c.state {
          SlabEntryState::WriteStream (_,_) => {
            let state = mem::replace(&mut c.state,SlabEntryState::Empty);
            if let SlabEntryState::WriteStream(ws,_) = state {
              c.state = SlabEntryState::WriteSpawned((content,0,ws));
            }
          },
          SlabEntryState::WriteSpawned (ref mut wr) => {
            wr.0.extend_from_slice(&content[..]);
          },
          _ => panic!("failed write : not connected"),
        };
        SlabEntryInnerState(c).send(bufsize)?;
      },
      SimpleLoopCommand::Expect(ad, expect,ended) => {
//        let (_, ref mut e) = cache.iter_mut().find(|&(_, ref e)|(e.rr.is_some())).unwrap();
//        panic!("e : {:?}, par : {:?}",e.peer,ad);
 //       let (_, ref mut e) = cache.iter_mut().find(|&(_, ref e)|e.peer == ad && (e.rr.is_some())).unwrap();
        // no filter on address as it is write stream address and not listener
        for (_, ref mut e) in cache.iter_mut() {
          if let SlabEntryState::ReadSpawned((ref mut c, _, _)) = e.state {
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

fn transport_corout_w_testing<T : Transport>(ocw : Option<(Rc<RefCell<Vec<u8>>>,CoRHandle)>, ows : Option<T::WriteStream>, _ : &T,bufsize : usize) -> Result<(Rc<RefCell<Vec<u8>>>,CoRHandle)> {
  match ocw {
    Some(mut s) => {
      if s.0.borrow().len() > 0 {
        s.1.resume(0).unwrap();
      }
      return Ok(s);
    },
    None => match ows {
      Some(ws) => {
        let s = corout_ows2(ws,Vec::new(),bufsize);
        return Ok(s);
      },
      None => panic!("uinint corout"),
    },
  }
}
fn transport_cpupool_w_testing<W : 'static + Send,T>(ocw : Option<CpuFuture<W,Error>>, ows : Option<W>, _ : &T,bufsize : usize,cpupool : Arc<CpuPool>) -> Result<CpuFuture<W,Error>> {
  match ocw {
    Some(s) => {
      return Ok(s);
    },
    None => match ows {
      Some(ws) => {
        return Ok(cpupool.spawn(ok(ws)));
      },
      None => panic!("uinint cpupool"),
    },
  }
}
fn thread_park_w_spawn<W : 'static + Send + Write>(ws : W, bufsize : usize) -> (Sender<Vec<u8>>,JoinHandle<()>) {
  let (s,r) = channel();
  let h = thread::Builder::new().spawn(move || {
    let mut ws = ws;
    loop {
    let to_send : Vec<u8> = match r.try_recv() {
        Ok(t) => t,
        Err(e) => if let TryRecvError::Empty = e {
          thread::park();
          continue;
        } else {
          panic!("th park mpsc error : {:?}",e);
        },
    };
    let mut i = 0;
    while i < to_send.len() {
      let nbr = WriteThreadparkPool(&mut ws).write(&to_send[i..min(i + bufsize,to_send.len())]).unwrap();
      i += nbr;
    }
  }}).unwrap();
  return (s,h);
}

fn transport_threadpark_w_testing<W : 'static + Send + Write,T>(ocw : Option<(Sender<Vec<u8>>,JoinHandle<()>)>, ows : Option<W>, _ : &T,bufsize : usize) -> Result<(Sender<Vec<u8>>,JoinHandle<()>)> {
  match ocw {
    Some(mut s) => {
      s.1.thread().unpark();
      return Ok(s);
    },
    None => match ows {
      Some(ws) => Ok(thread_park_w_spawn(ws,bufsize)),
      None => panic!("uinint thpark"),
    },
  }

}






fn transport_corout_r_testing<T : Transport>(ocr : Option<CoRHandle>, ors : Option<T::ReadStream>, _ : &T, bufsize : usize, contentsize : usize, ended : Arc<AtomicUsize>, expected : Vec<u8>) -> Result<CoRHandle> {
  match ocr {
    Some(mut s) => {
      s.resume(0).unwrap();
      return Ok(s);
    },
    None => (),
  };
  let mut co_handle = Coroutine::spawn(move |corout,_|{
    let mut rs = ors.unwrap();
    let mut nbread : usize = 0;
    let mut buf = vec![0;contentsize];
    while nbread < contentsize {
      nbread += ReadCorout(&mut rs, corout).read(&mut buf[nbread..min(nbread + bufsize,contentsize)]).unwrap();
    };
    assert!(&buf[..] == &expected[..], "left : {:?}, right : {:?}",&buf[..],&expected[..]);
    ended.fetch_add(1,Ordering::Relaxed);
    0
  });
  co_handle.resume(0).unwrap();
  Ok(co_handle)
}
fn transport_cpupool_r_testing<T : Transport>(ocr : Option<CpuFuture<(),Error>>, ors : Option<T::ReadStream>, _ : &T, bufsize : usize, contentsize : usize, ended : Arc<AtomicUsize>, expected : Vec<u8>, cpupool : Arc<CpuPool>) -> Result<CpuFuture<(),Error>> {
  match ocr {
    Some(mut s) => {
     // s.poll().unwrap(); // poll
      return Ok(s);
    },
    None => (),
  };
  let mut cpufut = cpupool.spawn_fn(move || {
    let mut rs = ors.unwrap();
    let mut nbread : usize = 0;
    let mut buf = vec![0;contentsize];
    while nbread < contentsize {
      nbread += ReadCpuPool(&mut rs).read(&mut buf[nbread..min(nbread + bufsize,contentsize)]).unwrap();
    };
    assert!(&buf[..] == &expected[..], "left : {:?}, right : {:?}",&buf[..],&expected[..]);
    ended.fetch_add(1,Ordering::Relaxed);
    ok(())
  });
  Ok(cpufut)
}
fn transport_threadpark_r_testing<T : Transport>(ocr : Option<JoinHandle<()>>, ors : Option<T::ReadStream>, _ : &T, bufsize : usize, contentsize : usize, ended : Arc<AtomicUsize>, expected : Vec<u8>) -> Result<JoinHandle<()>> {
  match ocr {
    Some(mut s) => {
      s.thread().unpark();
      return Ok(s);
    },
    None => (),
  };
  let handle = thread::spawn(move || {
    let mut rs = ors.unwrap();
    let mut nbread : usize = 0;
    let mut buf = vec![0;contentsize];
    while nbread < contentsize {
      nbread += ReadThreadparkPool(&mut rs).read(&mut buf[nbread..min(nbread + bufsize,contentsize)]).unwrap();
    };
    assert!(&buf[..] == &expected[..], "left : {:?}, right : {:?}",&buf[..],&expected[..]);
    ended.fetch_add(1,Ordering::Relaxed);
  });
  Ok(handle)
}
  

pub fn reg_rw_corout_testing<T : Transport>(fromadd : T::Address, tfrom : T, toadd : T::Address, tto : T, content_size : usize, read_buf_size : usize, write_buf_size : usize, nbmess : usize) {
  let mut contents = Vec::with_capacity(nbmess);
  let mut all_r = vec![0;nbmess * content_size];
  for im in 0 .. nbmess {
    let mut content = vec![0;content_size];
    for i in 0..content_size {
      let v = (i + im) as u8;
      content[i]= v;
      all_r[im * content_size + i] = v;
    }
    contents.push(content);
  }
  
  let ended_expect = Arc::new(AtomicUsize::new(0));
  let ended_expectf = Arc::new(AtomicUsize::new(0));
  let connect_done = Arc::new(AtomicUsize::new(0));
  let tomsg = spawn_loop_async(tto, move |a,b,c,cended,all_r,cpp|
                               transport_corout_r_testing(a,b,c,read_buf_size,content_size * nbmess,cended, all_r), move |a,b,c,cpp| transport_corout_w_testing(a,b,c,write_buf_size), simple_command_controller_corout,ended_expect.clone(),all_r,connect_done.clone(),Arc::new(CpuPool::new(1))).unwrap();
  let frommsg = spawn_loop_async(tfrom, move |a,b,c,cended,all_r,cpp| 
                                transport_corout_r_testing(a,b,c,read_buf_size,content_size * nbmess,cended, all_r), move |a,b,c,cpp| transport_corout_w_testing(a,b,c,write_buf_size), simple_command_controller_corout,ended_expectf.clone(),Vec::new(),connect_done.clone(),Arc::new(CpuPool::new(1))).unwrap();

  // send and receive commands
  frommsg.send(SimpleLoopCommand::ConnectWith(toadd.clone())).unwrap();
  while connect_done.load(Ordering::Relaxed) != 1 { }
  for content in contents.iter() {
    frommsg.send(SimpleLoopCommand::SendTo(toadd.clone(),content.clone(),write_buf_size)).unwrap();
  }

  while ended_expect.load(Ordering::Relaxed) != 1 { }

}


pub struct WriteCorout<'a,W : 'a + Write> (&'a mut W, &'a mut Coroutine);
impl<'a,W : Write> Write for WriteCorout<'a,W> {
 fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
   loop {
     match self.0.write(buf) {
       Ok(r) => return Ok(r),
       Err(e) => if let IoErrorKind::WouldBlock = e.kind() {
         self.1.yield_with(0);
       } else {
         return Err(e);
       },
     }
   }
 }
 fn flush(&mut self) -> IoResult<()> {
   loop {
     match self.0.flush() {
       Ok(r) => return Ok(r),
       Err(e) => if let IoErrorKind::WouldBlock = e.kind() {
         self.1.yield_with(0);
       } else {
         return Err(e);
       },
     }
   }
 }
 // no write_all impl as default write all would require state
 //fn write_all(&mut self, buf: &[u8]) -> IoResult<()> { .. }

}

pub struct ReadCorout<'a,R : 'a + Read> (&'a mut R, &'a mut Coroutine);
impl<'a,R : Read> Read for ReadCorout<'a,R> {
  fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
    loop {
      match self.0.read(buf) {
        Ok(r) => return Ok(r),
        Err(e) => if let IoErrorKind::WouldBlock = e.kind() {
          self.1.yield_with(0);
        } else {
          return Err(e);
        },
      }
    }
  }
}
pub struct ReadCpuPool<'a,R : 'a + Read> (&'a mut R);
impl<'a,R : Read> Read for ReadCpuPool<'a,R> {
  fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
    loop {
      match self.0.read(buf) {
        Ok(r) => return Ok(r),
        Err(e) => if let IoErrorKind::WouldBlock = e.kind() {
          // do nothing, ease thread switch
          thread::yield_now();
        } else {
          return Err(e);
        },
      }
    }
  }
}
pub struct WriteCpuPool<'a,W : 'a + Write> (&'a mut W);
impl<'a,W : Write> Write for WriteCpuPool<'a,W> {
 fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
   loop {
     match self.0.write(buf) {
       Ok(r) => return Ok(r),
       Err(e) => if let IoErrorKind::WouldBlock = e.kind() {
          thread::yield_now();
       } else {
         return Err(e);
       },
     }
   }
 }
 fn flush(&mut self) -> IoResult<()> {
   loop {
     match self.0.flush() {
       Ok(r) => return Ok(r),
       Err(e) => if let IoErrorKind::WouldBlock = e.kind() {
          thread::yield_now();
       } else {
         return Err(e);
       },
     }
   }
 }
 // no write_all impl as default write all would require state
 //fn write_all(&mut self, buf: &[u8]) -> IoResult<()> { .. }

}

pub struct ReadThreadparkPool<'a,R : 'a + Read> (&'a mut R);
impl<'a,R : Read> Read for ReadThreadparkPool<'a,R> {
  fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
    loop {
      match self.0.read(buf) {
        Ok(r) => return Ok(r),
        Err(e) => if let IoErrorKind::WouldBlock = e.kind() {
          thread::park();
        } else {
          return Err(e);
        },
      }
    }
  }
}
pub struct WriteThreadparkPool<'a,W : 'a + Write> (&'a mut W);
impl<'a,W : Write> Write for WriteThreadparkPool<'a,W> {
 fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
   loop {
     match self.0.write(buf) {
       Ok(r) => return Ok(r),
       Err(e) => if let IoErrorKind::WouldBlock = e.kind() {
          thread::park();
       } else {
         return Err(e);
       },
     }
   }
 }
 fn flush(&mut self) -> IoResult<()> {
   loop {
     match self.0.flush() {
       Ok(r) => return Ok(r),
       Err(e) => if let IoErrorKind::WouldBlock = e.kind() {
          thread::park();
       } else {
         return Err(e);
       },
     }
   }
 }
 // no write_all impl as default write all would require state
 //fn write_all(&mut self, buf: &[u8]) -> IoResult<()> { .. }

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
  let connect_done = Arc::new(AtomicUsize::new(0));
  //spawn_loop_async(tto, transport_reg_r1, transport_reg_w1, controller1).unwrap();
  //spawn_loop_async(tfrom, transport_reg_r_testing, transport_reg_w_testing, controller2).unwrap();

  let tomsg = spawn_loop_async(tto, move |a,b,c,cended,all_r,cpp| transport_reg_r_testing(a,b,c,read_buf_size,content_size * nbmess,cended), move |a,b,c,cpp| transport_reg_w_testing(a,b,c,write_buf_size), simple_command_controller,ended_expect.clone(),Vec::new(),connect_done.clone(),Arc::new(CpuPool::new(1))).unwrap();
  let frommsg = spawn_loop_async(tfrom, move |a,b,c,cended,all_r,cpp| transport_reg_r_testing(a,b,c,read_buf_size,content_size * nbmess,cended), move |a,b,c,cpp| transport_reg_w_testing(a,b,c,write_buf_size), simple_command_controller,ended_expect.clone(),Vec::new(),connect_done.clone(),Arc::new(CpuPool::new(1))).unwrap();

  // send and receive commands
  frommsg.send(SimpleLoopCommand::ConnectWith(toadd.clone())).unwrap();
  while connect_done.load(Ordering::Relaxed) != 1 { }
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

pub fn reg_rw_cpupool_testing<T : Transport>(fromadd : T::Address, tfrom : T, toadd : T::Address, tto : T, content_size : usize, read_buf_size : usize, write_buf_size : usize, nbmess : usize, numcpu : usize) {
  assert!(numcpu > 1);
  let mut contents = Vec::with_capacity(nbmess);
  let mut all_r = vec![0;nbmess * content_size];
  for im in 0 .. nbmess {
    let mut content = vec![0;content_size];
    for i in 0..content_size {
      let v = (i + im) as u8;
      content[i]= v;
      all_r[im * content_size + i] = v;
    }
    contents.push(content);
  }
  
  let cpupool = Arc::new(CpuPool::new(numcpu));
  let ended_expect = Arc::new(AtomicUsize::new(0));
  let ended_expectf = Arc::new(AtomicUsize::new(0));
  let connect_done = Arc::new(AtomicUsize::new(0));
  let cpp = cpupool.clone();
  let tomsg = spawn_loop_async(tto, move |a,b,c,cended,all_r,cpp1|
                               transport_cpupool_r_testing(a,b,c,read_buf_size,content_size * nbmess,cended, all_r,cpp1), move |a,b,c,cpp2| transport_cpupool_w_testing(a,b,c,write_buf_size,cpp2), move|c,ca,t,cpp3|simple_command_controller_cpupool(c,ca,t,cpp3),ended_expect.clone(),all_r,connect_done.clone(),cpp).unwrap();
  let cpp = cpupool.clone();

  let frommsg = spawn_loop_async(tfrom, move |a,b,c,cended,all_r,cpp1| 
                                transport_cpupool_r_testing(a,b,c,read_buf_size,content_size * nbmess,cended, all_r,cpp1), move |a,b,c,cpp2| transport_cpupool_w_testing(a,b,c,write_buf_size,cpp2), move|c,ca,t,cpp3|simple_command_controller_cpupool(c,ca,t,cpp3),ended_expectf.clone(),Vec::new(),connect_done.clone(),cpp).unwrap();

  // send and receive commands
  frommsg.send(SimpleLoopCommand::ConnectWith(toadd.clone())).unwrap();
  while connect_done.load(Ordering::Relaxed) != 1 { }
  for content in contents.iter() {
    frommsg.send(SimpleLoopCommand::SendTo(toadd.clone(),content.clone(),write_buf_size)).unwrap();
  }

  while ended_expect.load(Ordering::Relaxed) != 1 { }

}
pub fn reg_rw_threadpark_testing<T : Transport>(fromadd : T::Address, tfrom : T, toadd : T::Address, tto : T, content_size : usize, read_buf_size : usize, write_buf_size : usize, nbmess : usize) {

  let mut contents = Vec::with_capacity(nbmess);
  let mut all_r = vec![0;nbmess * content_size];
  for im in 0 .. nbmess {
    let mut content = vec![0;content_size];
    for i in 0..content_size {
      let v = (i + im) as u8;
      content[i]= v;
      all_r[im * content_size + i] = v;
    }
    contents.push(content);
  }
  
  let cpupool = Arc::new(CpuPool::new(1));
  let ended_expect = Arc::new(AtomicUsize::new(0));
  let ended_expectf = Arc::new(AtomicUsize::new(0));
  let connect_done = Arc::new(AtomicUsize::new(0));
  let cpp = cpupool.clone();
  let tomsg = spawn_loop_async(tto, move |a,b,c,cended,all_r,_|
                               transport_threadpark_r_testing(a,b,c,read_buf_size,content_size * nbmess,cended, all_r), move |a,b,c,_| transport_threadpark_w_testing(a,b,c,write_buf_size), move|c,ca,t,_|simple_command_controller_threadpark(c,ca,t),ended_expect.clone(),all_r,connect_done.clone(),cpp).unwrap();
  let cpp = cpupool.clone();

  let frommsg = spawn_loop_async(tfrom, move |a,b,c,cended,all_r,_| 
                                transport_threadpark_r_testing(a,b,c,read_buf_size,content_size * nbmess,cended, all_r), move |a,b,c,_| transport_threadpark_w_testing(a,b,c,write_buf_size), move|c,ca,t,_|simple_command_controller_threadpark(c,ca,t),ended_expectf.clone(),Vec::new(),connect_done.clone(),cpp).unwrap();

  // send and receive commands
  frommsg.send(SimpleLoopCommand::ConnectWith(toadd.clone())).unwrap();
  while connect_done.load(Ordering::Relaxed) != 1 { }
  for content in contents.iter() {
    frommsg.send(SimpleLoopCommand::SendTo(toadd.clone(),content.clone(),write_buf_size)).unwrap();
  }

  while ended_expect.load(Ordering::Relaxed) != 1 { }

}

