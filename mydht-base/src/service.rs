//! Service trait : this trait simply translate the idea of a query and an asynch reply.
//!
//! Service Failure is returend as result to the spawner : the inner service process returning `Result` is here
//! for this purpose only. It should respawn or close.
//!
//! Command reply and error are send to another spawnhandle (as a Command).
//!
//! Spawner can loop (for instance a receive stream on an established transport connection) or stop and restart using `State parameter` and command remaining. Or combine both approach, reducing cost of state usage and avoiding stalled loop.
//!
//! Reply and Error are inherent to the command used
//! Service can be spawn using different strategies thrus the Spawn trait.
//!
//! For KVStore state would be initialization function (not function once as must be return ), or
//! builder TODO see kvstore refactoring
//! For peer reader, it would be its stream.
//!

extern crate coroutine;
extern crate spin;
extern crate futures_cpupool;
extern crate futures;

use self::futures_cpupool::CpuPool as FCpuPool;
use self::futures_cpupool::CpuFuture;
use self::futures::Async;
use self::futures::Future;
use self::futures::future::ok as okfuture;
use self::futures::future::err as errfuture;
use self::coroutine::Error as CoroutError;
use self::coroutine::asymmetric::{
  Handle as CoRHandle,
  Coroutine as CoroutineC,
};
use std::rc::Rc;
use std::cell::RefCell;
use std::sync::{
  Arc,
};
use std::sync::atomic::{
  AtomicBool,
  Ordering,
};
use self::spin::{
  Mutex,
};
use std::thread;
use std::thread::JoinHandle;
use std::marker::PhantomData;
use mydhtresult::{
  Result,
  Error,
  ErrorKind,
  ErrorLevel,
  ErrorLevel as MdhtErrorLevel,
};
use std::io::{
  Result as IoResult,
  ErrorKind as IoErrorKind,
  Read,
  Write,
};
use std::mem::replace;
use std::collections::vec_deque::VecDeque;


pub struct ReadYield<'a,R : 'a + Read,Y : 'a + SpawnerYield> (&'a mut R,&'a mut Y);
impl<'a,R : Read,Y : SpawnerYield> Read for ReadYield<'a,R,Y> {
  fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
    loop {
      match self.0.read(buf) {
        Ok(r) => return Ok(r),
        Err(e) => if let IoErrorKind::WouldBlock = e.kind() {
          match self.1.spawn_yield() {
            YieldReturn::Return => return Err(e),
            YieldReturn::Loop => (), 
          }
        } else {
          return Err(e);
        },
      }
    }
  }
}
pub struct WriteYield<'a,W : 'a + Write, Y : 'a + SpawnerYield> (&'a mut W, &'a mut Y);
impl<'a,W : Write, Y : SpawnerYield> Write for WriteYield<'a,W,Y> {
 fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
   loop {
     match self.0.write(buf) {
       Ok(r) => return Ok(r),
       Err(e) => if let IoErrorKind::WouldBlock = e.kind() {
          match self.1.spawn_yield() {
            YieldReturn::Return => return Err(e),
            YieldReturn::Loop => (), 
          }
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
          match self.1.spawn_yield() {
            YieldReturn::Return => return Err(e),
            YieldReturn::Loop => (), 
          }
       } else {
         return Err(e);
       },
     }
   }
 }
} 



/// TODO duration before restart (in conjonction with nb loop)
/// The service struct can have an inner State (mutable in call).
pub trait Service {
  /// if ignore level error (eg async wouldblock) is send should we report as a failure or ignore and restart with
  /// state later (for instance if a register async stream token is available again in a epoll).
  /// This depend on the inner implementation : if it could suspend on error and restart latter
  /// with same state parameter.
  const RESTARTABLE : bool;
  /// If state contains all info needed to restart the service at the point it return a mydhterror
  /// of level ignore (wouldblock), set this to true; most of the time it should be set to false
  /// (safer).
  type CommandIn;
  type CommandOut;

  /// Allways Return a CommandOut, Error should be a command TODO consider returning error and
  /// having another SpawnSend to manage it (to avoid composing SpawnSend in enum).
  /// Returning Error for async would block or error which should shut the spawner (the main loop).
  /// If no command is 
  /// For restartable service no command in can be send, if no command in is send and it is not
  /// restartable, an error is returned (if input is expected) : use const for testing before call
  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, async_yield : S) -> Result<Self::CommandOut>;
}

pub trait Spawner<S : Service, D : SpawnSend<<S as Service>::CommandOut>, R : SpawnRecv<<S as Service>::CommandIn>> {
  type Handle : SpawnHandle<S,R>;
  type Yield : SpawnerYield;
  /// use 0 as nb loop if service should loop forever (for instance on a read receiver or a kvstore
  /// service)
  fn spawn (
    &mut self,
    service : S,
    spawn_out : D,
//    state : Option<<S as Service>::State>,
    Option<<S as Service>::CommandIn>,
    rec : R,
    nb_loop : usize // infinite if 0
  ) -> Result<Self::Handle>;
}
/*
pub SpawnChannel<Command> {
  type Send : SpawnSend<Command>;
  type Recv : SpawnRecv<Command>;
  fn new() -> Result<(Self::Send,Self::Recv)>;
}*/

/// send command to spawn
pub trait SpawnSend<Command> : Sized {
  /// if send is disable set to false, this can be test before calling `send`
  const CAN_SEND : bool;
  /// mut on self is currently not needed by impl
  fn send(&mut self, Command) -> Result<()>;
}
/// send command to spawn
pub trait SpawnRecv<Command> : Sized {
  /// mut on self is currently not needed by impl
  fn recv(&mut self) -> Result<Option<Command>>;
}


/// Handle use to send command to get back state
/// State in the handle is simply the Service struct
pub trait SpawnHandle<Service,Recv> {
  /// self is mut as most of the time this function is used in context where unwrap_state should be
  /// use so it allows more possibility for implementating
  fn is_finished(&mut self) -> bool;
  fn unyield(&mut self) -> Result<()>;
  fn unwrap_state(self) -> Result<(Service,Recv)>;
}

/// manages asynch call by possibly yielding process (yield a coroutine if same thread, park or
/// yield a thread, do nothing (block)
pub trait SpawnerYield {
  fn spawn_yield(&mut self) -> YieldReturn;
}

#[derive(Clone,Copy)]
pub enum YieldReturn {
  /// end spawn, transmit state to handle. Some Service may be restartable if enough info in state.
  /// This return an ignore Mydht level error.
  Return,
  /// retry at the same point (stack should be the same)
  Loop,
}

pub struct NoRecv;
pub struct NoSend;
/*pub struct NoChannel;
impl<C> SpawnChannel<C> for NoChannel {
  type Send = NoSend;
  type Recv = NoRecv;
  fn new() -> Result<(Self::Send,Self::Recv)> {
    Ok((NoSend,NoRecv))
  }
}*/
impl<C> SpawnRecv<C> for NoRecv {
  #[inline]
  fn recv(&mut self) -> Result<Option<C>> {
    Ok(None)
  }
}
impl<C> SpawnSend<C> for NoSend {
  const CAN_SEND : bool = false;
  #[inline]
  fn send(&mut self, _ : C) -> Result<()> {
    // return as Bug : code should check CAN_SEND before
    Err(Error("Spawner does not support send".to_string(), ErrorKind::Bug, None))
  }
}

pub struct NoYield(YieldReturn);
impl SpawnerYield for NoYield {
  #[inline]
  fn spawn_yield(&mut self) -> YieldReturn {
    self.0
  }
}

pub struct BlockingSameThread<S,R>((S,R));
impl<S,R> SpawnHandle<S,R> for BlockingSameThread<S,R> {
  #[inline]
  fn is_finished(&mut self) -> bool {
    true // if called it must be finished
  }
  #[inline]
  fn unyield(&mut self) -> Result<()> {
    Ok(())
  }
  #[inline]
  fn unwrap_state(self) -> Result<(S,R)> {
    Ok(self.0,)
  }
}

/// Local spawn send, could be use for non thread spawn (not Send)
/// Not for local coroutine as Rc is required in this case
impl<'a, C> SpawnSend<C> for &'a mut VecDeque<C> {
  const CAN_SEND : bool = true;
  fn send(&mut self, c : C) -> Result<()> {
    self.push_front(c);
    Ok(())
  }
}
/// Should not be use, except for very specific case or debugging.
/// It blocks the main loop during process (run on the same thread).
//pub struct Blocker<S : Service>(PhantomData<S>);
pub struct Blocker;

macro_rules! spawn_loop {($service:ident,$spawn_out:ident,$ocin:ident,$r:ident,$nb_loop:ident,$yield_build:expr,$return_build:expr) => {
    loop {
      match $ocin {
        Some(cin) => {
          let cmd_out_r = $service.call(cin, $yield_build);
          if let Some(cmd_out) = try_ignore!(cmd_out_r, "In spawner : {:?}") {
            if D::CAN_SEND {
              $spawn_out.send(cmd_out)?;
            }
          } else {
            return $return_build;
          }
          if $nb_loop > 0 {
            $nb_loop -= 1;
            if $nb_loop == 0 {
              break;
            }
          }
        },
        None => {
          $ocin = $r.recv()?;
          if $ocin.is_none() {
            if let YieldReturn::Return = $yield_build.spawn_yield() {
              return $return_build;
            } else {
              continue;
            }
          }
        },
      }
      $ocin = None;
    }
}}

impl<S : Service, D : SpawnSend<<S as Service>::CommandOut>, R : SpawnRecv<S::CommandIn>> Spawner<S,D,R> for Blocker {
  type Handle = BlockingSameThread<S,R>;
  type Yield = NoYield;
  fn spawn (
    &mut self,
    mut service : S,
    mut spawn_out : D,
    mut ocin : Option<<S as Service>::CommandIn>,
    mut r : R,
    mut nb_loop : usize
  ) -> Result<Self::Handle> {
    spawn_loop!(service,spawn_out,ocin,r,nb_loop,NoYield(YieldReturn::Loop),Err(Error("Blocking spawn service return would block when should loop".to_string(), ErrorKind::Bug, None)));
    return Ok((BlockingSameThread((service,r))))
  }
}



pub struct RestartSpawn<S : Service,D : SpawnSend<<S as Service>::CommandOut>, SP : Spawner<S,D,R>, R : SpawnRecv<S::CommandIn>> {
  spawner : SP,
  service : S,
  spawn_out : D,
  recv : R,
  nb_loop : usize,
}

pub enum RestartSameThread<S : Service,D : SpawnSend<<S as Service>::CommandOut>, SP : Spawner<S,D,R>, R : SpawnRecv<S::CommandIn>> {
  ToRestart(RestartSpawn<S,D,SP,R>),
  Ended((S,R)),
  // technical
  Empty,
}

impl<S : Service,D : SpawnSend<<S as Service>::CommandOut>, R : SpawnRecv<S::CommandIn>>
  SpawnHandle<S,R> for 
  RestartSameThread<S,D,RestartOrError,R> {
  #[inline]
  fn is_finished(&mut self) -> bool {
    if let &mut RestartSameThread::Ended(_) = self {
      true
    } else {
      false
    }
  }
  #[inline]
  fn unyield(&mut self) -> Result<()> {
    if let &mut RestartSameThread::ToRestart(_) = self {
      let tr = replace(self, RestartSameThread::Empty);
      if let RestartSameThread::ToRestart(RestartSpawn{
        mut spawner,
        service,
        spawn_out,
        recv,
        nb_loop,
      }) = tr {
        let rs = spawner.spawn(service,spawn_out,None,recv,nb_loop)?;
        replace(self, rs);
      } else {
        unreachable!();
      }
    }
    Ok(())
  }

  #[inline]
  fn unwrap_state(mut self) -> Result<(S,R)> {
    loop {
      match self {
        RestartSameThread::ToRestart(_) => {
          self.unyield()?;
        },
        RestartSameThread::Ended(t) => {
          return Ok(t);
        },
        _ => unreachable!(),
      }
    }
  }
}


/// For restartable service, running in the same thread, restart on service state
/// Notice that restart is done by spawning again : this kind of spawn can only be use at place
/// where it is known to restart
pub struct RestartOrError;

impl<S : Service, D : SpawnSend<<S as Service>::CommandOut>, R : SpawnRecv<S::CommandIn>> Spawner<S,D,R> for RestartOrError {
  type Handle = RestartSameThread<S,D,Self,R>;
  type Yield = NoYield;
  fn spawn (
    &mut self,
    mut service : S,
    mut spawn_out : D,
    mut ocin : Option<<S as Service>::CommandIn>,
    mut recv : R,
    mut nb_loop : usize
  ) -> Result<Self::Handle> {
    spawn_loop!(service,spawn_out,ocin,recv,nb_loop,NoYield(YieldReturn::Return),
        Ok(RestartSameThread::ToRestart(
              RestartSpawn {
                spawner : RestartOrError,
                service : service,
                spawn_out : spawn_out,
                recv : recv,
                nb_loop : nb_loop,
              }))
      );
    return Ok(RestartSameThread::Ended((service,recv)))
  }
}

/// TODO move in its own crate or under feature in mydht : coroutine dependency in mydht-base is
/// bad
pub struct Coroutine<'a>(PhantomData<&'a()>);
/// common send/recv for coroutine local usage (not Send, but clone)
pub type LocalRc<C> = Rc<RefCell<VecDeque<C>>>;
/// not type alias as will move from this crate
pub struct CoroutHandle<S,R>(Rc<Option<S>>,R,CoRHandle);
pub struct CoroutYield<'a>(&'a mut CoroutineC);

impl<'a> SpawnerYield for CoroutYield<'a> {
  #[inline]
  fn spawn_yield(&mut self) -> YieldReturn {
    self.0.yield_with(0);
    YieldReturn::Loop
  }
}


impl<S,R> SpawnHandle<S,R> for CoroutHandle<S,R> {
  #[inline]
  fn is_finished(&mut self) -> bool {
    self.2.is_finished()
  }
  #[inline]
  fn unyield(&mut self) -> Result<()> {
    self.2.resume(0).map_err(|e|{
      match e {
        CoroutError::Panicked => panic!("Spawned coroutine has panicked"),
        CoroutError::Panicking(c) => panic!(c),
      };
    }).unwrap();
    Ok(())
  }
  #[inline]
  fn unwrap_state(self) -> Result<(S,R)> {
    let s = match Rc::try_unwrap(self.0) {
      Ok(s) => s,
      Err(_) => return Err(Error("Rc not accessible, might have read an unfinished corouthandle".to_string(), ErrorKind::Bug, None)),
    };
    match s {
      Some(r) => Ok((r,self.1)),
      None => Err(Error("Read an unfinished corouthandle".to_string(), ErrorKind::Bug, None)),
    }
  }
}

impl<C> SpawnSend<C> for LocalRc<C> {
  const CAN_SEND : bool = true;
  fn send(&mut self, c : C) -> Result<()> {
    self.borrow_mut().push_front(c);
    Ok(())
  }
}
impl<C> SpawnRecv<C> for LocalRc<C> {
  fn recv(&mut self) -> Result<Option<C>> {
    Ok(self.borrow_mut().pop_back())
  }
}


impl<'a,S : 'static + Service, D : 'static + SpawnSend<<S as Service>::CommandOut>,R : 'static + Clone + SpawnRecv<S::CommandIn>> 
  Spawner<S,D,R> for Coroutine<'a> {
  type Handle = CoroutHandle<S,R>;
  type Yield = CoroutYield<'a>;
  fn spawn (
    &mut self,
    mut service : S,
    mut spawn_out : D,
    mut ocin : Option<<S as Service>::CommandIn>,
    mut recv : R,
    mut nb_loop : usize
  ) -> Result<Self::Handle> {

    let recvr = recv.clone();
    let rcs = Rc::new(None);
    let rcs2 = rcs.clone();
    let co_handle = CoroutineC::spawn(move |corout,_|{
      move || -> Result<()> {
        let mut rcs = rcs;
        spawn_loop!(service,spawn_out,ocin,recv,nb_loop,CoroutYield(corout),Ok(()));
        let dest = Rc::get_mut(&mut rcs).unwrap();
        replace(dest,Some(service));
        Ok(())
      }().unwrap(); // TODO error management on technical failure
      0
    });
    return Ok(CoroutHandle(rcs2,recvr,co_handle));
  }
}


pub struct ThreadBlock;
pub struct ThreadHandleBlock<S,R>(Arc<Mutex<Option<(S,R)>>>,JoinHandle<Result<()>>);
pub struct ThreadYieldBlock;
macro_rules! thread_handle {($name:ident,$yield:expr) => {

impl<S,R> SpawnHandle<S,R> for $name<S,R> {
  #[inline]
  fn is_finished(&mut self) -> bool {
    self.0.lock().is_some()
  }
  #[inline]
  fn unyield(&mut self) -> Result<()> {
    $yield(self)
  }

  #[inline]
  fn unwrap_state(mut self) -> Result<(S,R)> {
    let mut mlock = self.0.lock();
    if mlock.is_some() {
      //let ost = mutex.into_inner();
      let ost = replace(&mut (*mlock),None);
      Ok(ost.unwrap())
    } else {
      Err(Error("unwrap state on unfinished thread".to_string(), ErrorKind::Bug, None))
    }
  }
}

}}

thread_handle!(ThreadHandleBlock, |_ : &mut SpawnHandle<S,R>|{
  Ok(())
});



impl SpawnerYield for ThreadYieldBlock {
  #[inline]
  fn spawn_yield(&mut self) -> YieldReturn {
    thread::yield_now();
    YieldReturn::Loop
  }
}


macro_rules! thread_spawn {($spawner:ident,$handle:ident,$yield:ident) => {

impl<S : 'static + Send + Service, D : 'static + Send + SpawnSend<S::CommandOut>, R : 'static + Send + SpawnRecv<S::CommandIn>> 
  Spawner<S,D,R> for $spawner
  where S::CommandIn : Send
{
  type Handle = $handle<S,R>;
  type Yield = $yield;
  fn spawn (
    &mut self,
    mut service : S,
    mut spawn_out : D,
    mut ocin : Option<<S as Service>::CommandIn>,
    mut recv : R,
    mut nb_loop : usize
  ) -> Result<Self::Handle> {
    let finished = Arc::new(Mutex::new(None));
    let finished2 = finished.clone();
    let join_handle = thread::Builder::new().spawn(move ||{
      spawn_loop!(service,spawn_out,ocin,recv,nb_loop,$yield,Ok(()));
      let mut data = finished.lock();
      *data = Some((service,recv));
      Ok(())
    })?;
    return Ok($handle(finished2,join_handle));
  }
}

}}


thread_spawn!(ThreadBlock,ThreadHandleBlock,ThreadYieldBlock);


pub struct ThreadPark;
pub struct ThreadHandlePark<S,R>(Arc<Mutex<Option<(S,R)>>>,JoinHandle<Result<()>>);
pub struct ThreadYieldPark;

thread_handle!(ThreadHandlePark, |a : &mut ThreadHandlePark<S,R>|{
  a.1.thread().unpark();
  Ok(())
});

impl SpawnerYield for ThreadYieldPark {
  #[inline]
  fn spawn_yield(&mut self) -> YieldReturn {
    thread::park();
    YieldReturn::Loop
  }
}

thread_spawn!(ThreadPark,ThreadHandlePark,ThreadYieldPark);






/// TODO move in its own crate or under feature in mydht : cpupool dependency in mydht-base is
/// bad (future may be in api so fine)
/// This use CpuPool but not really the future abstraction
pub struct CpuPool(FCpuPool);
pub struct CpuPoolHandle<S,R>(CpuFuture<(S,R),Error>,Option<(S,R)>);
pub struct CpuPoolYield;

impl<S : Send + 'static,R : Send + 'static> SpawnHandle<S,R> for CpuPoolHandle<S,R> {
  #[inline]
  fn is_finished(&mut self) -> bool {
    match self.0.poll() {
      Ok(Async::Ready(r)) => {
        self.1 = Some(r);
        true
      },
      Ok(Async::NotReady) => false,
      Err(_) => true, // the error will be reported throug call to unwrap_starte TODO log it before
    }
  }
  #[inline]
  fn unyield(&mut self) -> Result<()> {
    if !self.1.is_some() {
      self.is_finished();
    }
    Ok(())
  }

  #[inline]
  fn unwrap_state(mut self) -> Result<(S,R)> {
    if self.1.is_some() {
      let r = replace(&mut self.1,None);
      return Ok(r.unwrap())
    }
    self.0.wait()
  }
}

impl SpawnerYield for CpuPoolYield {
  #[inline]
  fn spawn_yield(&mut self) -> YieldReturn {
    thread::park(); // TODO check without (may block)
    YieldReturn::Loop
  }
}

impl<S : 'static + Send + Service, D : 'static + Send + SpawnSend<S::CommandOut>, R : 'static + Send + SpawnRecv<S::CommandIn>> 
  Spawner<S,D,R> for CpuPool
  where S::CommandIn : Send
{
  type Handle = CpuPoolHandle<S,R>;
  type Yield = CpuPoolYield;
  fn spawn (
    &mut self,
    mut service : S,
    mut spawn_out : D,
    mut ocin : Option<<S as Service>::CommandIn>,
    mut recv : R,
    mut nb_loop : usize
  ) -> Result<Self::Handle> {
    let future = self.0.spawn_fn(move || {
      match move || -> Result<(S,R)> {
        spawn_loop!(service,spawn_out,ocin,recv,nb_loop,CpuPoolYield,Ok((service,recv)));
        Ok((service,recv))
      }() {
        Ok(r) => okfuture(r),
        Err(e) => errfuture(e),
      }
    });
    return Ok(CpuPoolHandle(future,None));
  }
}



/// This cpu pool use future internally TODO test it , the imediate impact upon CpuPool is that the
/// receiver for spawning is local (no need to be send)
pub struct CpuPoolFuture(FCpuPool);
pub struct CpuPoolHandleFuture<S,R,D>(CpuFuture<(S,D),Error>,Option<(S,D)>,R);

impl<S : Send + 'static + Service,R, D : 'static + Send + SpawnSend<S::CommandOut>> SpawnHandle<S,R> for CpuPoolHandleFuture<S,R,D> {
  #[inline]
  fn is_finished(&mut self) -> bool {
    match self.0.poll() {
      Ok(Async::Ready(r)) => {
        self.1 = Some(r);
        true
      },
      Ok(Async::NotReady) => false,
      Err(_) => true, // the error will be reported throug call to unwrap_starte TODO log it before
    }
  }
  #[inline]
  fn unyield(&mut self) -> Result<()> {
    if !self.1.is_some() {
      self.is_finished();
    }
    Ok(())
  }

  #[inline]
  fn unwrap_state(mut self) -> Result<(S,R)> {
    if self.1.is_some() {
      let r = replace(&mut self.1,None);
      return Ok((r.unwrap().0,self.2))
    }
    let res = self.0.wait()?;
    Ok((res.0,self.2))
  }
}


impl<S : 'static + Send + Service, D : 'static + Send + SpawnSend<S::CommandOut>, R : SpawnRecv<S::CommandIn>> 
  Spawner<S,D,R> for CpuPoolFuture
  where S::CommandIn : Send
{
  type Handle = CpuPoolHandleFuture<S,R,D>;
  type Yield = CpuPoolYield;
  fn spawn (
    &mut self,
    service : S,
    spawn_out : D,
    mut ocin : Option<<S as Service>::CommandIn>,
    mut recv : R,
    mut nb_loop : usize
  ) -> Result<Self::Handle> {
    let mut future = self.0.spawn(okfuture((service,spawn_out)));
    loop {
      match ocin {
        Some(cin) => {
          future = self.0.spawn(future.and_then(|(mut service, mut spawn_out)| {
            match service.call(cin, CpuPoolYield) {
              Ok(r) => {
                if D::CAN_SEND {
                  match spawn_out.send(r) {
                    Ok(_) => (),
                    Err(e) => return errfuture(e),
                  }
                }
                return okfuture((service,spawn_out));
              },
              Err(e) => if e.level() == MdhtErrorLevel::Ignore {
                panic!("This should only yield loop, there is an issue with implementation");
              } else if e.level() == MdhtErrorLevel::Panic {
                panic!("In spawner cpufuture panic : {:?} ",e);
              } else {
                return errfuture(e)
              },
            };
          }));
          if nb_loop > 0 {
            nb_loop -= 1;
            if nb_loop == 0 {
              break;
            }
          }
        },
        None => {
            ocin = recv.recv()?;
            if ocin.is_none() {
              CpuPoolYield.spawn_yield();
            } else {
              continue;
            }
        },
      }
      ocin = None;
    }
    return Ok(CpuPoolHandleFuture(future,None,recv));
  }
}


