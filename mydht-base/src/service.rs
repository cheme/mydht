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
extern crate mio;

use self::mio::{
  Registration,
  SetReadiness,
  Poll,
  Token,
  Ready,
  PollOpt,
};
use transport::{
  Registerable,
};
use std::sync::mpsc::{
  Receiver as MpscReceiver,
  TryRecvError,
  Sender as MpscSender,
  channel as mpsc_channel,
};
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


pub struct ReadYield<'a,R : 'a + Read,Y : 'a + SpawnerYield> (pub &'a mut R,pub &'a mut Y);
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
pub struct WriteYield<'a,W : 'a + Write, Y : 'a + SpawnerYield> (pub &'a mut W, pub &'a mut Y);
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



/// marker trait for service allowing restart (yield should return `YieldReturn::Return`).
/// if ignore level error (eg async wouldblock) is send should we report as a failure or ignore and restart with
/// state later (for instance if a register async stream token is available again in a epoll).
/// This depend on the inner implementation : if it could suspend on error and restart latter
/// with same state parameter.
pub trait ServiceRestartable {}

/// TODO duration before restart (in conjonction with nb loop)
/// The service struct can have an inner State (mutable in call).
pub trait Service {
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
  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, async_yield : &mut S) -> Result<Self::CommandOut>;
}


pub trait Spawner<
  S : Service,
  D : SpawnSend<<S as Service>::CommandOut>,
  R : SpawnRecv<<S as Service>::CommandIn>> {
  type Handle : SpawnHandle<S,D,R>;
  type Yield : SpawnerYield;
  /// use 0 as nb loop if service should loop forever (for instance on a read receiver or a kvstore
  /// service)
  /// TODO nb_loop is not enought to manage spawn, some optional duration could be usefull (that
  /// could also be polled out of the spawner : with a specific suspend command
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

/// Send/Recv service Builder
pub trait SpawnChannel<Command> {
  type Send : SpawnSend<Command>;
  type Recv : SpawnRecv<Command>;
  fn new(&mut self) -> Result<(Self::Send,Self::Recv)>;
}

/// send command to spawn, this is a blocking send currently : should evolve to non blocking.
pub trait SpawnSend<Command> : Sized {
  /// if send is disable set to false, this can be test before calling `send`
  const CAN_SEND : bool;
  /// mut on self is currently not needed by impl
  fn send(&mut self, Command) -> Result<()>;
}
/// send command to spawn, it is non blocking (None returned by recv on not ready) : cf macro
/// spawn_loop or others implementation
pub trait SpawnRecv<Command> : Sized {
  /// mut on self is currently not needed by impl
  fn recv(&mut self) -> Result<Option<Command>>;
//  fn new_sender(&mut self) -> Result<Self::SpawnSend>;
}

/// mio registrable channel receiver
pub struct MioRecv<R> {
  mpsc : R,
  reg : Registration,
}
impl<C,R : SpawnRecv<C>> SpawnRecv<C> for MioRecv<R> {
  fn recv(&mut self) -> Result<Option<C>> {
    self.mpsc.recv()
  }
}
impl<C> SpawnRecv<C> for MpscReceiver<C> {
  fn recv(&mut self) -> Result<Option<C>> {
    match self.try_recv() {
      Ok(t) => Ok(Some(t)),
      Err(e) => if let TryRecvError::Empty = e {
        Ok(None)
      } else {
        Err(Error(format!("{}",e), ErrorKind::ChannelRecvError,Some(Box::new(e))))
      },
    }
  }
}

impl<R> Registerable for MioRecv<R> {
  fn register(&self, p : &Poll, t : Token, r : Ready, po : PollOpt) -> Result<bool> {
    p.register(&self.reg,t,r,po)?;
    Ok(true)
  }
  fn reregister(&self, p : &Poll, t : Token, r : Ready, po : PollOpt) -> Result<bool> {
    p.reregister(&self.reg,t,r,po)?;
    Ok(true)
  }
}
/// mio registerable channel sender
pub struct MioSend<S> {
  mpsc : S,
  set_ready : SetReadiness,
}
impl<C,S : SpawnSend<C>> SpawnSend<C> for MioSend<S> {
  const CAN_SEND : bool = true;
  fn send(&mut self, t : C) -> Result<()> {
    self.mpsc.send(t)?;
    self.set_ready.set_readiness(Ready::readable())?;
    Ok(())
  }
}
impl<C> SpawnSend<C> for MpscSender<C> {
  const CAN_SEND : bool = true;
  fn send(&mut self, t : C) -> Result<()> {
    <MpscSender<C>>::send(self,t)?;
    Ok(())
  }
}

/// Mpsc channel as service send/recv
pub struct MpscChannel;

impl<C> SpawnChannel<C> for MpscChannel {
  type Send = MpscSender<C>;
  type Recv = MpscReceiver<C>;
  /// new use a struct as input to allow construction over a MyDht for a Peer :
  /// TODO MDht peer chan : first ensure peer is connected (or not) then run new channel -> send in
  /// sync send of loop -> loop proxy to peer asynchronously -> dist peer receive command if
  /// connected, then send in Read included service dest (their spawn impl is same as ours) -> des
  /// service send to our peer send which send to us -> the recv got a dest to this recv
  /// => this for this case we create two under lying channel from out of loop to loop and from
  /// readpeer to out of loop. Second channel is needed to start readservice : after connect ->
  /// connect is done with this channel creation (cf slab cache)
  fn new(&mut self) -> Result<(Self::Send,Self::Recv)> {
    let chan = mpsc_channel();
    Ok(chan)
  }
}
/// channel register on mio poll in service (service constrains Registrable on 
/// This allows running mio loop as service, note that the mio loop is reading this channel, not
/// the spawner loop (it can for commands like stop or restart yet it is not the current usecase
/// (no need for service yield right now).
pub struct MioChannel<CH>(pub CH);

impl<C,CH : SpawnChannel<C>> SpawnChannel<C> for MioChannel<CH> {
  type Send = MioSend<CH::Send>;
  type Recv = MioRecv<CH::Recv>;
  fn new(&mut self) -> Result<(Self::Send,Self::Recv)> {
    let (s,r) = self.0.new()?;
    let (reg,sr) = Registration::new2();
    Ok((MioSend {
      mpsc : s,
      set_ready : sr,
    }, MioRecv {
      mpsc : r,
      reg : reg,
    }))
  }
}

/// Handle use to send command to get back state
/// State in the handle is simply the Service struct
pub trait SpawnHandle<Service,Sen,Recv> {
  /// self is mut as most of the time this function is used in context where unwrap_state should be
  /// use so it allows more possibility for implementating
  fn is_finished(&mut self) -> bool;
  /// unyield implementation must take account of the fact that it is called frequently even if the
  /// spawn is not yield (no is_yield function here it is implementation dependant).
  fn unyield(&mut self) -> Result<()>;
  fn unwrap_state(self) -> Result<(Service,Sen,Recv)>;
  // TODO add a kill command and maybe a yield command
  //
  // TODO add a get_technical_error command : right now we do not know if we should restart on
  // finished or drop it!!!! -> plus solve the question of handling panic and error management
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

/// set a default value to receiver (spawn loop will therefore not yield on receiver
pub struct DefaultRecv<C,SR>(pub SR, pub C);

impl<C : Clone, SR : SpawnRecv<C>> SpawnRecv<C> for DefaultRecv<C,SR> {
  #[inline]
  fn recv(&mut self) -> Result<Option<C>> {
    let r = self.0.recv();
    if let Ok(None) = r {
      return Ok(Some(self.1.clone()))
    }
    r
  }
}


/// set a default value to receiver (spawn loop will therefore not yield on receiver
pub struct DefaultRecvChannel<C,CH>(pub CH,pub C);

impl<C : Clone, CH : SpawnChannel<C>> SpawnChannel<C> for DefaultRecvChannel<C,CH> {
  type Send = CH::Send;
  type Recv = DefaultRecv<C,CH::Recv>;

  fn new(&mut self) -> Result<(Self::Send,Self::Recv)> {
    let (s,r) = self.0.new()?;
    Ok((s,DefaultRecv(r,self.1.clone())))
  }
}
#[derive(Clone)]
pub struct NoChannel;
#[derive(Clone)]
pub struct NoRecv;
#[derive(Clone)]
pub struct NoSend;

impl<C> SpawnChannel<C> for NoChannel {
  type Send = NoSend;
  type Recv = NoRecv;
  fn new(&mut self) -> Result<(Self::Send,Self::Recv)> {
    Ok((NoSend,NoRecv))
  }
}
impl<C> SpawnRecv<C> for NoRecv {
  #[inline]
  fn recv(&mut self) -> Result<Option<C>> {
    Ok(None)
  }

  //TODO fn close(&mut self) -> Result<()> : close receiver, meaning senders will fail on send and
  //could be drop TODO also require is_close function (same for send) TODO this mus be last
}
impl<C> SpawnSend<C> for NoSend {
  const CAN_SEND : bool = false;
  #[inline]
  fn send(&mut self, _ : C) -> Result<()> {
    // return as Bug : code should check CAN_SEND before
    Err(Error("Spawner does not support send".to_string(), ErrorKind::Bug, None))
  }
}

pub struct NoYield(pub YieldReturn);
impl SpawnerYield for NoYield {
  #[inline]
  fn spawn_yield(&mut self) -> YieldReturn {
    self.0
  }
}

pub struct BlockingSameThread<S,D,R>((S,D,R));
impl<S,D,R> SpawnHandle<S,D,R> for BlockingSameThread<S,D,R> {
  #[inline]
  fn is_finished(&mut self) -> bool {
    true // if called it must be finished
  }
  #[inline]
  fn unyield(&mut self) -> Result<()> {
    Ok(())
  }
  #[inline]
  fn unwrap_state(self) -> Result<(S,D,R)> {
    Ok(self.0)
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

macro_rules! spawn_loop {($service:ident,$spawn_out:ident,$ocin:ident,$r:ident,$nb_loop:ident,$yield_build:ident,$return_build:expr) => {
    loop {
      match $ocin {
        Some(cin) => {
          let cmd_out_r = $service.call(cin, &mut $yield_build);
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
  type Handle = BlockingSameThread<S,D,R>;
  type Yield = NoYield;
  fn spawn (
    &mut self,
    mut service : S,
    mut spawn_out : D,
    mut ocin : Option<<S as Service>::CommandIn>,
    mut r : R,
    mut nb_loop : usize
  ) -> Result<Self::Handle> {
    let mut yiel = NoYield(YieldReturn::Loop);
    spawn_loop!(service,spawn_out,ocin,r,nb_loop,yiel,Err(Error("Blocking spawn service return would block when should loop".to_string(), ErrorKind::Bug, None)));
    return Ok((BlockingSameThread((service,spawn_out,r))))
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
  Ended((S,D,R)),
  // technical
  Empty,
}

impl<S : Service + ServiceRestartable,D : SpawnSend<<S as Service>::CommandOut>, R : SpawnRecv<S::CommandIn>>
  SpawnHandle<S,D,R> for 
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
  fn unwrap_state(mut self) -> Result<(S,D,R)> {
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

/// Only for Restartable service
impl<S : Service + ServiceRestartable, D : SpawnSend<<S as Service>::CommandOut>, R : SpawnRecv<S::CommandIn>> Spawner<S,D,R> for RestartOrError {
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
    let mut yiel = NoYield(YieldReturn::Return);
    spawn_loop!(service,spawn_out,ocin,recv,nb_loop,yiel,
        Ok(RestartSameThread::ToRestart(
              RestartSpawn {
                spawner : RestartOrError,
                service : service,
                spawn_out : spawn_out,
                recv : recv,
                nb_loop : nb_loop,
              }))
      );
    return Ok(RestartSameThread::Ended((service,spawn_out,recv)))
  }
}

/// TODO move in its own crate or under feature in mydht : coroutine dependency in mydht-base is
/// bad
pub struct Coroutine<'a>(PhantomData<&'a()>);
/// common send/recv for coroutine local usage (not Send, but clone)
pub type LocalRc<C> = Rc<RefCell<VecDeque<C>>>;
/// not type alias as will move from this crate
pub struct CoroutHandle<S,D,R>(Rc<Option<(S,D,R)>>,CoRHandle);
pub struct CoroutYield<'a>(&'a mut CoroutineC);

impl<'a> SpawnerYield for CoroutYield<'a> {
  #[inline]
  fn spawn_yield(&mut self) -> YieldReturn {
    self.0.yield_with(0);
    YieldReturn::Loop
  }
}


impl<S,D,R> SpawnHandle<S,D,R> for CoroutHandle<S,D,R> {
  #[inline]
  fn is_finished(&mut self) -> bool {
    self.1.is_finished()
  }
  #[inline]
  fn unyield(&mut self) -> Result<()> {
    self.1.resume(0).map_err(|e|{
      match e {
        CoroutError::Panicked => panic!("Spawned coroutine has panicked"),
        CoroutError::Panicking(c) => panic!(c),
      };
    }).unwrap();
    Ok(())
  }
  #[inline]
  fn unwrap_state(self) -> Result<(S,D,R)> {
    let s = match Rc::try_unwrap(self.0) {
      Ok(s) => s,
      Err(_) => return Err(Error("Rc not accessible, might have read an unfinished corouthandle".to_string(), ErrorKind::Bug, None)),
    };
    match s {
      Some(r) => Ok(r),
      None => Err(Error("Read an unfinished corouthandle".to_string(), ErrorKind::Bug, None)),
    }
  }
}
pub struct LocalRcChannel;

impl<C> SpawnChannel<C> for LocalRcChannel {
  type Send = LocalRc<C>;
  type Recv = LocalRc<C>;
  fn new(&mut self) -> Result<(Self::Send,Self::Recv)> {
    let lr = Rc::new(RefCell::new(VecDeque::new()));
    Ok((lr.clone(),lr))
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


impl<'a,S : 'static + Service, D : 'static + SpawnSend<<S as Service>::CommandOut>,R : 'static + SpawnRecv<S::CommandIn>> 
  Spawner<S,D,R> for Coroutine<'a> {
  type Handle = CoroutHandle<S,D,R>;
  type Yield = CoroutYield<'a>;
  fn spawn (
    &mut self,
    mut service : S,
    mut spawn_out : D,
    mut ocin : Option<<S as Service>::CommandIn>,
    mut recv : R,
    mut nb_loop : usize
  ) -> Result<Self::Handle> {

    let rcs = Rc::new(None);
    let rcs2 = rcs.clone();
    let co_handle = CoroutineC::spawn(move |corout,_|{
      move || -> Result<()> {
        let mut rcs = rcs;
        let mut yiel = CoroutYield(corout);
        spawn_loop!(service,spawn_out,ocin,recv,nb_loop,yiel,Ok(()));
        let dest = Rc::get_mut(&mut rcs).unwrap();
        replace(dest,Some((service,spawn_out,recv)));
        Ok(())
      }().unwrap(); // TODO error management on technical failure
      0
    });
    return Ok(CoroutHandle(rcs2,co_handle));
  }
}


pub struct ThreadBlock;
pub struct ThreadHandleBlock<S,D,R>(Arc<Mutex<Option<(S,D,R)>>>,JoinHandle<Result<()>>);
pub struct ThreadYieldBlock;
macro_rules! thread_handle {($name:ident,$yield:expr) => {

impl<S,D,R> SpawnHandle<S,D,R> for $name<S,D,R> {
  #[inline]
  fn is_finished(&mut self) -> bool {
    self.0.lock().is_some()
  }
  #[inline]
  fn unyield(&mut self) -> Result<()> {
    $yield(self)
  }

  #[inline]
  fn unwrap_state(mut self) -> Result<(S,D,R)> {
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

thread_handle!(ThreadHandleBlock, |_ : &mut SpawnHandle<S,D,R>|{
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
  type Handle = $handle<S,D,R>;
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
      *data = Some((service,spawn_out,recv));
      Ok(())
    })?;
    return Ok($handle(finished2,join_handle));
  }
}

}}


thread_spawn!(ThreadBlock,ThreadHandleBlock,ThreadYieldBlock);


pub struct ThreadPark;
pub struct ThreadHandlePark<S,D,R>(Arc<Mutex<Option<(S,D,R)>>>,JoinHandle<Result<()>>);
pub struct ThreadYieldPark;

thread_handle!(ThreadHandlePark, |a : &mut ThreadHandlePark<S,D,R>|{
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
pub struct CpuPoolHandle<S,D,R>(CpuFuture<(S,D,R),Error>,Option<(S,D,R)>);
pub struct CpuPoolYield;

impl<S : Send + 'static, D : Send + 'static, R : Send + 'static> SpawnHandle<S,D,R> for CpuPoolHandle<S,D,R> {
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
  fn unwrap_state(mut self) -> Result<(S,D,R)> {
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
  type Handle = CpuPoolHandle<S,D,R>;
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
      match move || -> Result<(S,D,R)> {
        spawn_loop!(service,spawn_out,ocin,recv,nb_loop,CpuPoolYield,Ok((service,spawn_out,recv)));
        Ok((service,spawn_out,recv))
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
pub struct CpuPoolHandleFuture<S,D,R>(CpuFuture<(S,D),Error>,Option<(S,D)>,R);

impl<S : Send + 'static + Service,R, D : 'static + Send + SpawnSend<S::CommandOut>> SpawnHandle<S,D,R> for CpuPoolHandleFuture<S,D,R> {
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
  fn unwrap_state(mut self) -> Result<(S,D,R)> {
    if self.1.is_some() {
      if let Some((s,d)) = replace(&mut self.1,None) {
        return Ok((s,d,self.2))
      } else {
        unreachable!()
      }
    }
    let res = self.0.wait()?;
    Ok((res.0,res.1,self.2))
  }
}


impl<S : 'static + Send + Service, D : 'static + Send + SpawnSend<S::CommandOut>, R : SpawnRecv<S::CommandIn>> 
  Spawner<S,D,R> for CpuPoolFuture
  where S::CommandIn : Send
{
  type Handle = CpuPoolHandleFuture<S,D,R>;
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
            match service.call(cin, &mut CpuPoolYield) {
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


