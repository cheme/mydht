//! Pool of connected transport
//! Dispatch is done depending on address only
//! For async an async loop is in pool, for sync a new pool is use per peer
//!
//!
use std::result::Result as StdResult;
use std::sync::{
  Arc,
};
use std::sync::mpsc::{
  Sender,
  SendError,
  channel,
};
use mio::{
  SetReadiness,
  Registration,
  Events,
  Poll as MioPoll,
  Ready,
  PollOpt,
  Token
};
use std::sync::atomic::{
  AtomicBool,
  Ordering,
};
use std::io::{
  Error as IoError,
  ErrorKind as IoErrorKind,
};
use mydht_base::kvcache::{
  KVCache,
};
use mydht_base::mydhtresult::{
  ErrorKind as MdhtErrorKind,
  ErrorLevel as MdhtErrorLevel,
  Error as MdhtError,
};
use mydht_base::transport::{
  Address,
  Transport,
  Registerable,
};
use std::io::{
  Read,
  Write,
};
use futures::{
  Future,
  IntoFuture,
  Sink,
  Poll,
  Async,
};
use futures::future::{
  PollFn,
  poll_fn,
};
use std::thread;
use std::thread::{
  JoinHandle,
  Thread,
};
use std::marker::PhantomData;
use std::io::Result as IoResult;
use mydht_base::mydhtresult::{
  Result,
  ErrorKind,
  Error,
};
use std::mem;
use futures_cpupool::{
  CpuPool,
  CpuFuture,
};

// TODO fn loopdispatcher : param transport, loopselector : allways use a mio poll including mpsc
// user space events
// also run as main api interface
/*
trait LoopDispatcher : Sized {
  type Transport : Transport;
  type Loop : Loop;

  type RSpawnerFacto : SpawnerFactory<<Self::Loop as Loop>::RecvExecPool>;
  type WSpawnerFacto : SpawnerFactory<<Self::Loop as Loop>::SendExecPool>;

  /// forward send to right loop or create transport stream and send to right loop
  fn create_or_use_send();
  /// loop dispatcher listen on our transport address
  fn new(t : Self::Transport) -> Result<Self>;
  fn get_loop_spawner();
}
*/
/// Send handle trait for mpsc trait and same thread exec handle or even function over loop cached
/// mutex, return only if message receive ok (for a subsequent ping, ping should not race fith
/// those handle message
trait HandleSend<M> {
  fn send(&self, t: M) -> StdResult<(), SendError<M>>;
}
/*
trait LoopSelector<T : Transport> {
  const LOOP_SIZE : usize;
  type Address : Address;
  type LoopHandle : HandleSend<T>;
  fn get_loop_handle (&self, &Self::Address) -> Self::LoopHandle; 
}
*/
pub enum LoopMessage<T : Transport> {
  /// new address from transport : may ping directly TODO conditional ping
  NewAddressR(<T as Transport>::Address, <T as Transport>::ReadStream, Option<<T as Transport>::WriteStream>),
  NewAddressS(<T as Transport>::Address, <T as Transport>::WriteStream, Option<<T as Transport>::ReadStream>),
 
}
 
/// Trait but closer to config of a loop function (see main default implementation)
trait Loop : Sized {
  type Transport : Transport;
  /// Executor for write stream : likely CpuPool for async and InlineExecutor of sync (or cpupool
  /// size one to send in independant thread from receive
  type SendExecPool : Spawner;
  /// Executor for receive stream : likely CpuPool for async and InlineExecutor of sync (one per
  type RecvExecPool : Spawner;
  /// Sink for Sending : buffer over send pool -> TODO useless??
  type SendSink : Sink;
  type Handle : HandleSend<LoopMessage<Self::Transport>>;

  fn run_loop(self) -> Self::Handle;
}
const LISTENER: Token = Token(0);
const MPSC_TOKEN: Token = Token(1);
const START_FREE_TOKEN: usize = 2;
const POOL_EVENT_SLAB_SIZE: usize = 1024;

//#[derive(Clone)]
pub struct PollSender<T> (pub Sender<T>, pub SetReadiness);

impl<T> Clone for PollSender<T> {
  fn clone(&self) -> Self {
    PollSender(self.0.clone(),self.1.clone())
  }
}

impl<T> PollSender<T> {
  /// Same proto as mpsc standard send
  fn send(&self, t: T) -> Result<()> {
    self.0.send(t)?;
    match self.1.set_readiness(Ready::readable()) {
      Ok(_) => Ok(()),
      Err(e) => {
        error!("Error with mpsc poll sender readiness update : {:?}", e);
        Err(MdhtError("mpsc poll sender send readiness issue".to_string(),MdhtErrorKind::Bug,Some(Box::new(e))))
      },
    }
  }
}

/// TODO spawner from transport
/// cache should be fast usize access (vec, slab) TODO slab implementation (use slab crate) of
/// KVCache<usize,v> or/and Cache<usize,v>
fn spawn_loop<T : Transport, P, TC : TransportCache<T,P>>(transport : T, mut cache : TC) -> Result<PollSender<LoopMessage<T>>> {
  let (send_reg, send_setready) = Registration::new2();
  let (sc, receiver) = channel();
  let sender = PollSender(sc, send_setready);
  let poll = MioPoll::new()?;
  poll.register(&send_reg, MPSC_TOKEN, Ready::readable(), PollOpt::edge())?;
  // register on loop
  let is_reg = transport.register(&poll, LISTENER, Ready::readable(), PollOpt::edge())?;
  let otransport = if !is_reg {
    let s2 = sender.clone();
    thread::spawn(move ||{
      loop {
        match transport.accept() {
          Ok((rs, ows, ad)) => if let Err(e) = s2.send(LoopMessage::NewAddressR(ad,rs,ows)) {
            error!("Mpsc failure on client reception, ignoring : {}",e);
          },
          Err(e) => if e.level() != MdhtErrorLevel::Ignore {
            break;
          } else if e.level() == MdhtErrorLevel::Panic {
            panic!("Mpsc failure on client reception, ignoring : {}",e);
          },
        }
      }
    });
    None
  } else {
    Some(transport)
  };
  thread::spawn(move || match inner_loop(poll, otransport, cache) {
    Ok(()) => (),
    Err(e) => panic!("Unexpected end of transport loop : {}",e),
  });
  Ok(sender)
}

/// last bool true if asynch and registered
trait TransportCache<T : Transport, P> : 'static + Send + KVCache<usize, (StreamType<T>, Option<usize>, Option<Arc<P>>, bool)> {
  fn insert(&mut self, val: (StreamType<T>, Option<usize>, Option<Arc<P>>,bool)) -> usize;
}
//impl<T : Transport, P, K : 'static + Send + KVCache<usize, (StreamType<T>, Option<usize>, Option<Arc<P>>)>> TransportCache<T,P> for K {}
enum StreamType<T : Transport> {
  Read(<T as Transport>::ReadStream),
  Write(<T as Transport>::WriteStream),
  PendingRead,// TODO on poll : launch future (if sync launch future directly)
  PendingWrite,//TODO future stuf with possible queue (see combinator possibility on running future)
//  LoopWrite, // TODO no cpupool instead
}
#[inline]
fn inner_loop<T : Transport, P, TC : TransportCache<T,P>>(poll : MioPoll, otransport : Option<T>, mut cache : TC) -> Result<()> {
  let reg = otransport.is_some();
  let mut events = Events::with_capacity(POOL_EVENT_SLAB_SIZE);
  let rotransport = otransport.as_ref();
  loop {
    poll.poll(&mut events, None)?;
    for event in events.iter() {
      match event.token() {
        LISTENER => {
          // new connection
          let (rs, ows, ad) = match rotransport.as_ref() {
            Some(t) => t.accept()?,
            None => unreachable!(), // not registered
          };
          // should be cleaner with VacantEntry from slab trait : if we consider removing trait
          // abstraction
          let tread = Token(cache.insert((StreamType::Read(rs), None,None, false)));
          let regr = reg_stream(&poll, tread, &mut cache)?;
          ows.map(|ws| -> Result<()> {
            let twrite = Token(cache.insert((StreamType::Write(ws),Some(tread.0),None, false)));
            reg_stream(&poll, tread, &mut cache)?;
            cache.update_val_c(&tread.0,|rc| {rc.1 = Some(twrite.0);Ok(())})?;
            Ok(())
          }).unwrap_or(Ok(()))?;

          if !regr {
            // TODO spawn of blocking listener using spawner
          }

          let reregt = rotransport.map(|transport|           // TODO test case to check if needed + same for mpsc (in theory yes)
            transport.reregister(&poll, LISTENER, Ready::readable(), PollOpt::edge())).unwrap_or(Ok(false))?;
        },
        MPSC_TOKEN => {
          // new connection message
        },
        _ => unreachable!(),
      }
    }
  }
}
 
fn reg_stream<T : Transport, P, TC : TransportCache<T,P>>(poll : &MioPoll, tok : Token, cache : &mut TC) -> Result<bool> {

  // TODO kvcache : use a get_mut or return something on fold_c
  let r = match cache.get_val_c(&tok.0) {
    Some(v) => {
      match v.0 {
        StreamType::Read(ref rs) => rs.register(&poll, tok, Ready::readable(), PollOpt::edge())?,
        StreamType::Write(ref ws) => ws.register(&poll, tok, Ready::readable(), PollOpt::edge())?,
        _ => false,
      }
    },
    None => false,
  };
  if r { 
    cache.update_val_c(&tok.0,|rc| {rc.3 = r;Ok(())})?;
  }
  Ok(r)
}
/// Spawn a future, current implementaitions are mainly inline, cpupool and unlimitthread. The
/// trait should be a future trait but at the time it is easier this way (close to executor
/// wrapper but allowing cpupools run)
trait Spawner {
  type Item;
  type Inner : FnOnce() -> StdResult<Self::Item,Error>;
  type SpawnFuture : Future<Item = Self::Item,Error = Error>;
  fn spawn(&mut self, Self::Inner) -> Self::SpawnFuture;
}
/// facto over spawner (mainly to single cpu pool)
/// We could make spawner clone but it is semantically odd.
trait SpawnerFactory<S> {
  fn new_spawn(&mut self) -> S;
}
/*where
    F: Future + Send + 'static,
    F::Item: Send + 'static,
    F::Error: Send + 'static, 
Executor<F: Future<Item = (), Error = ()>> {
    fn execute(&self, future: F) -> Result<(), ExecuteError<F>>;
*/
/// unbound spawner (no limit), without thread creation for each future
pub struct NewThreadSpawner<R,F> (PhantomData<(R,F)>);
impl<R,F> Spawner for NewThreadSpawner<R,F> 
  where
  R : Send + 'static,
  F : FnOnce() -> StdResult<R,Error> + Send + 'static, 
{
  type Item = R;
  type Inner = F;
  type SpawnFuture = ThreadFuture<R,Error>;
  fn spawn(&mut self, f: Self::Inner) -> Self::SpawnFuture {
    ThreadFuture::new(f) // TODO thread new and possibly sync fer rep as future impl
  }
}

pub struct CpuPoolSpawner<R,F> {
  pool : CpuPool,
  _ph : PhantomData<(R,F)>,
}

impl<R,F> CpuPoolSpawner<R,F> {
  pub fn new_num_cpus () -> Self {
    CpuPoolSpawner {
      pool : CpuPool::new_num_cpus(),
      _ph : PhantomData,
    }
  }

  pub fn new (s : usize) -> Self {
    CpuPoolSpawner {
      pool : CpuPool::new(s),
      _ph : PhantomData,
    }
  }
}

impl<R : Send + 'static,F : FnOnce() -> StdResult<R,Error> + Send + 'static> Spawner for CpuPoolSpawner<R,F> {
  type Item = R;
  type Inner = F;
  type SpawnFuture = CpuFuture<Self::Item,Error>;
   fn spawn(&mut self, f: Self::Inner) -> Self::SpawnFuture {
     self.pool.spawn(NoFuture::new(f))
  }

}

/// unbound spawner (no limit), without thread or coroutine usage : will block!!, must be use only
/// with sync transport
struct BlockingSpawner<R,F> (PhantomData<(R,F)>);
impl<R,F : FnOnce() -> StdResult<R,Error>> Spawner for BlockingSpawner<R,F> {
  type Item = R;
  type Inner = F;
  type SpawnFuture = NoFuture<F>;
   fn spawn(&mut self, f: Self::Inner) -> Self::SpawnFuture {
    NoFuture::new(f)
  }
}
/// no future : blocking future
/// For single context pools (Sync) TODO refactor to wrap Fn and IOErr : same for thread future
struct NoFuture<F> {
  inner : Option<F>,
}

impl<F> NoFuture<F> {
  pub fn new(f : F) -> Self {
    NoFuture {
      inner : Some(f)
    }
  }
}
/*impl<F : Future> Future for NoFuture<F> 
{
  type Item = F::Item;
  type Error = F::Error;
  
  fn poll(&mut self) -> Poll<Self::Item, Self::Error> {

    let of = mem::replace(&mut self.inner, None);
    self.inner = None;
    match of {
      Some(f) => {
        let r = f.wait()?;
        Ok(Async::Ready(r))
      },
      None => panic!("NoFuture already consumed"),
    }
  }
}*/
impl<F, R> Future for NoFuture<F> 
  where F : FnOnce() -> StdResult<R,Error>
{
  type Item = R;
  type Error = Error;
  
  fn poll(&mut self) -> Poll<Self::Item, Self::Error> {

    let of = mem::replace(&mut self.inner, None);
    self.inner = None;
    match of {
      Some(f) => {
        let r = f.call_once(())?;
        Ok(Async::Ready(r))
      },
      None => panic!("NoFuture already consumed"),
    }
  }
}

struct ThreadFuture<Item,Error> {
  finnish : Arc<(AtomicBool,Thread)>,
  thr : Option<JoinHandle<Poll<Item,Error>>>,
}
impl<I : Send + 'static, E : Send + 'static> ThreadFuture<I,E> {
  pub fn new<F : FnOnce() -> StdResult<I,E> + Send + 'static>(f : F) -> Self {


    let ab = Arc::new((AtomicBool::new(false), thread::current()));
    let abc = ab.clone();
    let jh = thread::spawn(move || {
/*      let f2 = f.and_then(poll_fn(|r|{ 
        *ab.get_mut() = true; 
        r 
      }));*/
      let r = f.call_once(())?;
      abc.0.store(true,Ordering::Relaxed);
      abc.1.unpark();
      Ok(Async::Ready(r))
    });
    ThreadFuture {
      finnish : ab,
      thr : Some(jh),
    }
  }
/*
  pub fn new_f<F : Future<Item = I,Error = E> + Send + 'static>(f : F) -> Self {


    let ab = Arc::new(AtomicBool::new(false));
    let abc = ab.clone();
    let jh = thread::spawn(move || {
/*      let f2 = f.and_then(poll_fn(|r|{ 
        *ab.get_mut() = true; 
        r 
      }));*/
      let r = f.wait()?;
      abc.store(true,Ordering::Relaxed);
      Ok(Async::Ready(r))
    });
    ThreadFuture {
      finnish : ab,
      thr : Some(jh),
    }
  }*/
}
impl<I,E> Future for ThreadFuture<I,E> 
{
  type Item = I;
  type Error = E;
  
  fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
  /*  while self.finnish.load(Ordering::Relaxed) == false {
    }*/
    if self.finnish.0.load(Ordering::Relaxed) == true {
//      self.thr.join()?
      let thr = mem::replace(&mut self.thr,None);
      match thr.expect("already consume future").join() {
        Ok(r) => r,
        Err(e) => panic!("error : {:?}",e),
      }
    } else {
//      thread::sleep_ms(100);
      Ok(Async::NotReady)
    }
  }
}

pub struct PollRead<'a,R>(R,&'a mut [u8]);

pub struct PollWrite<'a,W>(W,&'a [u8]);
//pub type Poll<T, E> = Result<Async<T>, E>;
impl<'a, R : Read + 'a> Future for PollRead<'a,R> {

  type Item = usize;
  type Error = Error;
 
  fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
    match self.0.read(self.1) {
      Ok(r) => Ok(Async::Ready(r)),
      Err(e) => if IoErrorKind::WouldBlock == e.kind() {
        Ok(Async::NotReady)
      } else {
        Err(e.into())
      },
    }
  }
}
impl<'a, W : Write + 'a> Future for PollWrite<'a,W> {

  type Item = usize;
  type Error = Error;
 
  fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
    match self.0.write(self.1) {
      Ok(r) => Ok(Async::Ready(r)),
      Err(e) => if IoErrorKind::WouldBlock == e.kind() {
        Ok(Async::NotReady)
      } else {
        Err(e.into())
      },
    }
  }
}
#[test]
fn test_spawner_into() {
  // could implement nofuture with call on new (instead of poll)
  let f : Result<usize> = Ok(1+2);
  let f1 = f.into_future().and_then(|r|Ok(r * 2));
  assert!(f1.wait().unwrap() == 6)
}

#[test]
fn test_spawner_nt() {
  let mut s = NewThreadSpawner(PhantomData);
  let f1 = s.spawn(||Ok(1 + 2)).and_then(|r|Ok(r * 2));
  assert!(f1.wait().unwrap() == 6)
}
#[test]
fn test_spawner_block() {
  let mut s = BlockingSpawner(PhantomData);
  let f1 = s.spawn(||Ok(1 + 2)).and_then(|r|Ok(r * 2));
  assert!(f1.wait().unwrap() == 6)
}
#[test]
fn test_spawner_cpup() {
  let mut s = CpuPoolSpawner::new(1);
  let f1 = s.spawn(||Ok(1 + 2)).and_then(|r|Ok(r * 2));
  assert!(f1.wait().unwrap() == 6)
}

