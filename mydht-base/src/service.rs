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
extern crate parking_lot;
extern crate futures_cpupool;
extern crate futures;
extern crate mio;

use utils::{
  SRef,
  SToRef,
};
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
/*use std::sync::atomic::{
  AtomicBool,
  AtomicUsize,
  Ordering,
};*/
use self::parking_lot::{
  Mutex,
  Condvar,
};
use std::thread;
use std::thread::JoinHandle;
use std::marker::PhantomData;
use mydhtresult::{
  Result,
  Error,
  ErrorKind,
  ErrorLevel,
};
use std::io::{
  Result as IoResult,
  ErrorKind as IoErrorKind,
  Error as IoError,
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
        Ok(r) => {
          if r == 0 {
            continue;
          }
          return Ok(r)
        },
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
  /// Variant of default read_exact where we block on read returning 0 instead of returning an
  /// error (for a transport stream it makes more sense)
  fn read_exact(&mut self, mut buf: &mut [u8]) -> IoResult<()> {
    while !buf.is_empty() {
      match self.read(buf) {
        Ok(0) => {
          /*match self.1.spawn_yield() {
            YieldReturn::Return => return Err(IoError::new(IoErrorKind::WouldBlock,
         "from read_exact")),
            YieldReturn::Loop => (), 
          }*/
        },
        Ok(n) => { let tmp = buf; buf = &mut tmp[n..]; }
        Err(ref e) if e.kind() == IoErrorKind::Interrupted => { }
        Err(e) => return Err(e),
      }
    }
    Ok(())
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

  /*fn write_all(&mut self, mut buf: &[u8]) -> IoResult<()> {
      while !buf.is_empty() {
          match self.write(buf) {
              Ok(0) => {
                return Err(IoError::new(IoErrorKind::WriteZero,
                                             "failed to write whole buffer"))
              },
              Ok(n) => buf = &buf[n..],
              Err(ref e) if e.kind() == IoErrorKind::Interrupted => {}
              Err(e) => return Err(e),
          }
      }
      Ok(())
  }*/
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

pub struct NoService<I,O>(PhantomData<(I,O)>);
impl<CIN,COUT> NoService<CIN,COUT> {
  pub fn new() -> Self { NoService(PhantomData) }
}
impl<CIN,COUT> Clone for NoService<CIN,COUT> {
  fn clone(&self) -> Self { NoService(PhantomData) }
}
impl<CIN,COUT> Service for NoService<CIN,COUT> {
  type CommandIn = CIN;
  type CommandOut = COUT;

  fn call<S : SpawnerYield>(&mut self, _: Self::CommandIn, _ : &mut S) -> Result<Self::CommandOut> {
    panic!("No service called : please use a dummy spawner with no service")
  }
}

/// 
/// SpawnHandle allow management of service from outside.
/// SpawnerYield allows inner service management, and only for yield (others action could be
/// managed by Spawner implementation (catching errors and of course ending spawn).
///
/// With parallel execution of spawn and caller, SpawnHandle and SpawnerYield must be synchronized.
/// A typical concurrency issue being a yield call and an unyield happenning during this yield
/// execution, for instance with an asynch read on a stream : a read would block leading to
/// unyield() call, during unyield the read stream becomes available again and unyield is called :
/// yield being not finished unyield may be seen as useless and after yield occurs it could be
/// stuck. An atomic state is therefore required, to skip a yield (returning YieldSpawn::Loop
/// even if it is normally a YieldSpawn::Return) is fine but skipping a unyield is bad.
/// Therefore unyield may add a atomic yield skip to its call when this kind of behaviour is expected.
/// Skiping a yield with loop returning being not an issue.
/// Same thing for channel read and subsequent yield.
/// Yield prototype does not include blocking call or action so the skip once is the preffered synch
/// mecanism (cost one loop : a second channel read call or write/read stream call), no protected
/// section.
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
  type WeakSend : SpawnSend<Command> + Clone;
  type Send : SpawnSend<Command> + Clone;
  type Recv : SpawnRecv<Command>;
  fn new(&mut self) -> Result<(Self::Send,Self::Recv)>;
  // SRef trait on send could be use but SToRef is useless : unneeded here
  fn get_weak_send(&Self::Send) -> Option<Self::WeakSend>;
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
#[derive(Clone)]
pub struct MioSend<S> {
  mpsc : S,
  set_ready : SetReadiness,
}
impl<C,S : SpawnSend<C>> SpawnSend<C> for MioSend<S> {
  const CAN_SEND : bool = <S as SpawnSend<C>>::CAN_SEND;
  fn send(&mut self, t : C) -> Result<()> {
    self.mpsc.send(t)?;
    self.set_ready.set_readiness(Ready::readable())?;
    Ok(())
  }
}

/// return the command if handle is finished
#[inline]
pub fn send_with_handle<S : SpawnSend<C>, H : SpawnUnyield, C> (s : &mut S, h : &mut H, command : C) -> Result<Option<C>> {
//Service,Sen,Recv
  Ok(if h.is_finished() || !<S as SpawnSend<C>>::CAN_SEND {
    Some(command)
  } else {
    s.send(command)?;
    h.unyield()?;
    None
  })
}


#[derive(Clone)]
pub struct HandleSend<S,H : SpawnUnyield>(pub S, pub H);

impl<C,S : SpawnSend<C>, H : SpawnUnyield> SpawnSend<C> for HandleSend<S,H> {
  const CAN_SEND : bool = <S as SpawnSend<C>>::CAN_SEND;
  fn send(&mut self, t : C) -> Result<()> {
    self.0.send(t)?;
    self.1.unyield()?;
    Ok(())
  }
}

/// tech trait only for implementing send with handle as member of a struct (avoid unconstrained
/// parameter for HandleSend but currently use only for this struct)
pub trait SpawnSendWithHandle<C> {
  fn send_with_handle(&mut self, C) -> Result<Option<C>>;
}

impl<C,S : SpawnSend<C>, H : SpawnUnyield> SpawnSendWithHandle<C> for HandleSend<S,H> {
  #[inline]
  fn send_with_handle(&mut self, command : C) -> Result<Option<C>> {
    send_with_handle(&mut self.0,&mut self.1,command)
  }
}

impl<C> SpawnSend<C> for MpscSender<C> {
  const CAN_SEND : bool = true;
  fn send(&mut self, t : C) -> Result<()> {
    <MpscSender<C>>::send(self,t)?;
    Ok(())
  }
}

/// Mpsc channel as service send/recv, to use on Ref (non sendable content)
pub struct MpscChannelRef;
pub struct MpscSenderRef<C : SRef>(MpscSender<C::Send>);
pub struct MpscReceiverRef<C : SRef>(MpscReceiver<C::Send>);
pub struct MpscSenderToRef<CS>(MpscSender<CS>);
pub struct MpscReceiverToRef<CS>(MpscReceiver<CS>);

impl<C : SRef> SpawnSend<C> for MpscSenderRef<C> {
  const CAN_SEND : bool = <MpscSender<C::Send>>::CAN_SEND;
  fn send(&mut self, t : C) -> Result<()> {
    <MpscSender<C::Send> as SpawnSend<C::Send>>::send(&mut self.0,t.get_sendable())?;
    Ok(())
  }
}

impl<C : SRef> SpawnSend<C> for MpscSenderToRef<C::Send> {
//impl<C : Ref<C>> ToRef<MpscSenderRef<C>,MpscSenderRef<C>> for MpscSender<C::Send> {
  const CAN_SEND : bool = true;
  fn send(&mut self, t : C) -> Result<()> {
    <MpscSender<C::Send>>::send(&mut self.0,t.get_sendable())?;
    Ok(())
  }
}

impl<C : SRef> SpawnRecv<C> for MpscReceiverRef<C> {
  fn recv(&mut self) -> Result<Option<C>> {
    let r = <MpscReceiver<C::Send> as SpawnRecv<C::Send>>::recv(&mut self.0)?;
    Ok(r.map(|tr|tr.to_ref()))
  }
}

impl<C : SRef> SpawnRecv<C> for MpscReceiverToRef<C::Send> {
  fn recv(&mut self) -> Result<Option<C>> {
    let r = <MpscReceiver<C::Send> as SpawnRecv<C::Send>>::recv(&mut self.0)?;
    Ok(r.map(|tr|tr.to_ref()))
  }
}


impl<C : SRef> Clone for MpscSenderRef<C> {
  fn clone(&self) -> Self {
    MpscSenderRef(self.0.clone())
  }
}

impl<C : SRef> SRef for MpscSenderRef<C> {
  type Send = MpscSenderToRef<C::Send>;
  #[inline]
  fn get_sendable(self) -> Self::Send {
    MpscSenderToRef(self.0)
  }
}

impl<C : SRef> SToRef<MpscSenderRef<C>> for MpscSenderToRef<C::Send> {
  #[inline]
  fn to_ref(self) -> MpscSenderRef<C> {
    MpscSenderRef(self.0)
  }
}

impl<C : SRef> SRef for MpscReceiverRef<C> {
  type Send = MpscReceiverToRef<C::Send>;
  #[inline]
  fn get_sendable(self) -> Self::Send {
    MpscReceiverToRef(self.0)
  }
}

impl<C : SRef> SToRef<MpscReceiverRef<C>> for MpscReceiverToRef<C::Send> {
  #[inline]
  fn to_ref(self) -> MpscReceiverRef<C> {
    MpscReceiverRef(self.0)
  }
}



impl<C : SRef> SpawnChannel<C> for MpscChannelRef {
  type WeakSend = MpscSenderRef<C>;
  type Send = MpscSenderRef<C>;
  type Recv = MpscReceiverRef<C>;
  fn new(&mut self) -> Result<(Self::Send,Self::Recv)> {
    let (s,r) = mpsc_channel();
    Ok((MpscSenderRef(s),MpscReceiverRef(r)))
  }
  fn get_weak_send(s : &Self::Send) -> Option<Self::WeakSend> {
    Some(s.clone())
  }
}


/// Mpsc channel as service send/recv
pub struct MpscChannel;

impl<C> SpawnChannel<C> for MpscChannel {
  type WeakSend = MpscSender<C>;
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
  fn get_weak_send(s : &Self::Send) -> Option<Self::WeakSend> {
    Some(s.clone())
  }
}

/// channel register on mio poll in service (service constrains Registrable on 
/// This allows running mio loop as service, note that the mio loop is reading this channel, not
/// the spawner loop (it can for commands like stop or restart yet it is not the current usecase
/// (no need for service yield right now).
pub struct MioChannel<CH>(pub CH);

impl<C,CH : SpawnChannel<C>> SpawnChannel<C> for MioChannel<CH> {
  type WeakSend = MioSend<CH::WeakSend>;
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
  fn get_weak_send(s : &Self::Send) -> Option<Self::WeakSend> {
    <CH as SpawnChannel<C>>::get_weak_send(&s.mpsc).map(|ms|
    MioSend {
      mpsc : ms,
      set_ready : s.set_ready.clone(),
    })
  }
}

#[derive(Clone)]
pub struct NoWeakHandle;
impl SpawnUnyield for NoWeakHandle {
  fn is_finished(&mut self) -> bool {
    unreachable!()
  }
  fn unyield(&mut self) -> Result<()> {
    unreachable!()
  }
}

/// Handle use to send command to get back state
/// State in the handle is simply the Service struct
pub trait SpawnHandle<Service,Sen,Recv> : SpawnUnyield {
  type WeakHandle : SpawnUnyield + Clone;

  /// if finished (implementation should panic if methode call if not finished), error management
  /// through last result and restart service through 3 firt items.
  /// Error are only technical : service error should send errormessge in sender.
  fn unwrap_state(self) -> Result<(Service,Sen,Recv,Result<()>)>;

  /// not all implementation allow weakhandle
  fn get_weak_handle(&self) -> Option<Self::WeakHandle>;
  // TODO add a kill command and maybe a yield command
  //
  // TODO add a get_technical_error command : right now we do not know if we should restart on
  // finished or drop it!!!! -> plus solve the question of handling panic and error management
  // This goes with is_finished redesign : return state
}

pub trait SpawnUnyield {
  /// self is mut as most of the time this function is used in context where unwrap_state should be
  /// use so it allows more possibility for implementating
  ///
  /// TODO error management for is_finished plus is_finished in threads implementation is currently
  /// extra racy -> can receive message while finishing  : not a huge issue as the receiver remains
  /// into spawnhandle : after testing is_finished to true the receiver if not empty could be
  /// reuse. -> TODO some logging in case it is relevant and requires a synch.
  ///
  /// is_finished is not included in unwrap_state because it should be optimized by implementation
  /// (called frequently return single bool).
  fn is_finished(&mut self) -> bool;
  /// unyield implementation must take account of the fact that it is called frequently even if the
  /// spawn is not yield (no is_yield function here it is implementation dependant).
  /// For parrallel spawner (threading), unyield should position a skip atomic to true in case
  /// where it does not actually unyield.
  fn unyield(&mut self) -> Result<()>;

}

/// manages asynch call by possibly yielding process (yield a coroutine if same thread, park or
/// yield a thread, do nothing (block)
pub trait SpawnerYield {
  /// For parrallel spawner (threading), it is ok to skip yield once in case of possible racy unyield
  /// (unyield may not be racy : unyield on spawnchannel instead of asynch stream or others), see
  /// threadpark implementation
  /// At the time no SpawnerYield return both variant of YieldReturn and it could be a constant
  /// (use of both mode is spawner is not very likely but possible).
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
pub struct DefaultRecvRef<C : SRef,SR : SRef>(DefaultRecv<<C as SRef>::Send, <SR as SRef>::Send>);

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

impl<C : SRef, SR : SRef> SRef for DefaultRecv<C,SR> 
{ 
  type Send = DefaultRecvRef<C,SR>;
  #[inline]
  fn get_sendable(self) -> Self::Send {
    DefaultRecvRef(DefaultRecv(self.0.get_sendable(),self.1.get_sendable()))
  }
}

impl<C : SRef, SR : SRef> SToRef<DefaultRecv<C,SR>> for DefaultRecvRef<C,SR> {
  #[inline]
  fn to_ref(self) -> DefaultRecv<C,SR> {
    DefaultRecv((self.0).0.to_ref(),(self.0).1.to_ref())
  }
}
impl<C : SRef, SR : SRef> SpawnRecv<C> for DefaultRecvRef<C,SR> where 
  <C as SRef>::Send : Clone,
  <SR as SRef>::Send : SpawnRecv<<C as SRef>::Send>,
  {
  #[inline]
  fn recv(&mut self) -> Result<Option<C>> {
    let a = self.0.recv()?;
    Ok(a.map(|a|a.to_ref()))
  }
}


/// set a default value to receiver (spawn loop will therefore not yield on receiver
pub struct DefaultRecvChannel<C,CH>(pub CH,pub C);

impl<C : Clone, CH : SpawnChannel<C>> SpawnChannel<C> for DefaultRecvChannel<C,CH> {
  type WeakSend = CH::WeakSend;
  type Send = CH::Send;
  type Recv = DefaultRecv<C,CH::Recv>;

  fn new(&mut self) -> Result<(Self::Send,Self::Recv)> {
    let (s,r) = self.0.new()?;
    Ok((s,DefaultRecv(r,self.1.clone())))
  }
  fn get_weak_send(s : &Self::Send) -> Option<Self::WeakSend> {
    <CH as SpawnChannel<C>>::get_weak_send(s)
  }
}
#[derive(Clone,Debug)]
pub struct NoChannel;
#[derive(Clone,Debug)]
pub struct NoRecv;
#[derive(Clone,Debug)]
pub struct NoSend;
#[derive(Clone,Debug)]
pub struct NoSpawn;
#[derive(Clone,Debug)]
pub struct NoHandle;

sref_self!(NoSend);



impl<S : Service,
  D : SpawnSend<<S as Service>::CommandOut>,
  R : SpawnRecv<<S as Service>::CommandIn>>
  Spawner<S,D,R> for NoSpawn {
  type Handle = NoHandle;
  type Yield = NoYield;
  fn spawn (
    &mut self,
    _ : S,
    _ : D,
    _ : Option<<S as Service>::CommandIn>,
    _ : R,
    _ : usize // infinite if 0
  ) -> Result<Self::Handle> {
    Ok(NoHandle)
  }
}

impl SpawnUnyield for NoHandle {
  fn is_finished(&mut self) -> bool {
    false
  }
  fn unyield(&mut self) -> Result<()> {
    Ok(())
  }
}

impl<S : Service,
  D : SpawnSend<<S as Service>::CommandOut>,
  R : SpawnRecv<<S as Service>::CommandIn>>
  SpawnHandle<S,D,R> for NoHandle {
  type WeakHandle = NoHandle;
  fn unwrap_state(self) -> Result<(S,D,R,Result<()>)> {
    unreachable!("check is_finished before : No spawn is never finished");
  }
  fn get_weak_handle(&self) -> Option<Self::WeakHandle> {
    Some(NoHandle)
  }
}

impl<C> SpawnChannel<C> for NoChannel {
  type WeakSend = NoSend;
  type Send = NoSend;
  type Recv = NoRecv;
  fn new(&mut self) -> Result<(Self::Send,Self::Recv)> {
    Ok((NoSend,NoRecv))
  }
  fn get_weak_send(_ : &Self::Send) -> Option<Self::WeakSend> {
    None
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

sref_self!(NoRecv);

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

pub struct BlockingSameThread<S,D,R>((S,D,R,Result<()>));
impl<S,D,R> SpawnUnyield for BlockingSameThread<S,D,R> {
  #[inline]
  fn is_finished(&mut self) -> bool {
    true // if called it must be finished
  }
  #[inline]
  fn unyield(&mut self) -> Result<()> {
    Ok(())
  }

}
impl<S,D,R> SpawnHandle<S,D,R> for BlockingSameThread<S,D,R> {
  type WeakHandle = NoWeakHandle;
  #[inline]
  fn unwrap_state(self) -> Result<(S,D,R,Result<()>)> {
    Ok(self.0)
  }
  #[inline]
  fn get_weak_handle(&self) -> Option<Self::WeakHandle> {
    None
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


#[derive(Clone)]
/// Should not be use, except for very specific case or debugging.
/// It blocks the main loop during process (run on the same thread).
//pub struct Blocker<S : Service>(PhantomData<S>);
pub struct Blocker;


// TODO make it a function returning result of type parameter
macro_rules! spawn_loop {($service:ident,$spawn_out:ident,$ocin:ident,$r:ident,$nb_loop:ident,$yield_build:ident,$result:ident,$return_build:expr) => {
  loop {
    match $ocin {
      Some(cin) => {
        match $service.call(cin, &mut $yield_build) {
          Ok(r) => {
            if D::CAN_SEND {
              $spawn_out.send(r)?;
            }
          },
          Err(e) => if e.level() == ErrorLevel::Ignore {
            // suspend with YieldReturn 
            return $return_build;
          } else if e.level() == ErrorLevel::Panic {
            panic!("In spawner : {:?}",e);
          } else {
            $result = Err(e);
            break;
          },
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
        } else {
          continue;
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
    let mut err = Ok(());
    spawn_loop!(service,spawn_out,ocin,r,nb_loop,yiel,err,Err(Error("Blocking spawn service return would return when should loop".to_string(), ErrorKind::Bug, None)));
    return Ok((BlockingSameThread((service,spawn_out,r,err))))
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
  Ended((S,D,R,Result<()>)),
  // technical
  Empty,
}

impl<S : Service + ServiceRestartable,D : SpawnSend<<S as Service>::CommandOut>, R : SpawnRecv<S::CommandIn>>
  SpawnUnyield for
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


}
impl<S : Service + ServiceRestartable,D : SpawnSend<<S as Service>::CommandOut>, R : SpawnRecv<S::CommandIn>>
  SpawnHandle<S,D,R> for 
  RestartSameThread<S,D,RestartOrError,R> {
  type WeakHandle = NoWeakHandle;
  #[inline]
  fn unwrap_state(mut self) -> Result<(S,D,R,Result<()>)> {
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
  #[inline]
  fn get_weak_handle(&self) -> Option<Self::WeakHandle> {
    None
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
    let mut err = Ok(());
    spawn_loop!(service,spawn_out,ocin,recv,nb_loop,yiel,err,
        Ok(RestartSameThread::ToRestart(
              RestartSpawn {
                spawner : RestartOrError,
                service : service,
                spawn_out : spawn_out,
                recv : recv,
                nb_loop : nb_loop,
              }))
      );
      Ok(RestartSameThread::Ended((service,spawn_out,recv,err)))
  }
}

/// TODO move in its own crate or under feature in mydht : coroutine dependency in mydht-base is
/// bad
pub struct Coroutine;
/// common send/recv for coroutine local usage (not Send, but clone)
pub type LocalRc<C> = Rc<RefCell<VecDeque<C>>>;
/// not type alias as will move from this crate
pub struct CoroutHandle<S,D,R>(Rc<RefCell<Option<(S,D,R,Result<()>)>>>,CoRHandle);
//pub struct CoroutYield<'a>(&'a mut CoroutineC);

/*impl<'a> SpawnerYield for CoroutYield<'a> {
  #[inline]
  fn spawn_yield(&mut self) -> YieldReturn {
    self.0.yield_with(0);
    YieldReturn::Loop
  }
}*/
impl<'a,A : SpawnerYield> SpawnerYield for &'a mut A {
  #[inline]
  fn spawn_yield(&mut self) -> YieldReturn {
    (*self).spawn_yield()
  }
}
impl SpawnerYield for CoroutineC {
  #[inline]
  fn spawn_yield(&mut self) -> YieldReturn {
    self.yield_with(0);
    YieldReturn::Loop
  }
}




impl<S,D,R> SpawnUnyield for CoroutHandle<S,D,R> {
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
}
impl<S,D,R> SpawnHandle<S,D,R> for CoroutHandle<S,D,R> {
  type WeakHandle = NoWeakHandle;
  #[inline]
  fn unwrap_state(self) -> Result<(S,D,R,Result<()>)> {
    match self.0.replace(None) {
      Some(s) => Ok(s),
      None => Err(Error("Read an unfinished corouthandle".to_string(), ErrorKind::Bug, None)),
    }
  }
  #[inline]
  fn get_weak_handle(&self) -> Option<Self::WeakHandle> {
    None
  }
}

pub struct LocalRcChannel;

impl<C> SpawnChannel<C> for LocalRcChannel {
  type WeakSend = NoSend;
  type Send = LocalRc<C>;
  type Recv = LocalRc<C>;
  fn new(&mut self) -> Result<(Self::Send,Self::Recv)> {
    let lr = Rc::new(RefCell::new(VecDeque::new()));
    Ok((lr.clone(),lr))
  }
  fn get_weak_send(_ : &Self::Send) -> Option<Self::WeakSend> { None }
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


impl<S : 'static + Service, D : 'static + SpawnSend<<S as Service>::CommandOut>,R : 'static + SpawnRecv<S::CommandIn>> 
  Spawner<S,D,R> for Coroutine {
  type Handle = CoroutHandle<S,D,R>;
  type Yield = CoroutineC;
  //type Yield = CoroutYield<'a>;
  fn spawn (
    &mut self,
    mut service : S,
    mut spawn_out : D,
    mut ocin : Option<<S as Service>::CommandIn>,
    mut recv : R,
    mut nb_loop : usize
  ) -> Result<Self::Handle> {

    let rcs = Rc::new(RefCell::new(None));
    let rcs2 = rcs.clone();
    let co_handle = CoroutineC::spawn(move |corout,_|{
      move || -> Result<()> {
        let mut err = Ok(());
        let rcs = rcs;
        let mut yiel = corout;
        spawn_loop!(service,spawn_out,ocin,recv,nb_loop,yiel,err,Err(Error("Coroutine spawn service return would return when should loop".to_string(), ErrorKind::Bug, None)));
        rcs.replace(Some((service,spawn_out,recv,err)));
        //replace(dest,Some((service,spawn_out,recv,err)));
        Ok(())
      }().unwrap();
      0
    });
    let mut handle = CoroutHandle(rcs2,co_handle); 
    // start it
    handle.unyield()?;
    return Ok(handle);
  }
}


pub struct ThreadBlock;
pub struct ThreadHandleBlock<S,D,R>(Arc<Mutex<Option<(S,D,R,Result<()>)>>>,JoinHandle<Result<()>>);
pub struct ThreadHandleBlockWeak<S,D,R>(Arc<Mutex<Option<(S,D,R,Result<()>)>>>);
impl<S,D,R> Clone for ThreadHandleBlockWeak<S,D,R> {
  fn clone(&self) -> Self {
    ThreadHandleBlockWeak(self.0.clone())
  }
}
pub struct ThreadYieldBlock;

impl<S,D,R> SpawnUnyield for ThreadHandleBlockWeak<S,D,R> {
  #[inline]
  fn is_finished(&mut self) -> bool {
    self.0.lock().is_some()
  }
  #[inline]
  fn unyield(&mut self) -> Result<()> {
    Ok(())
  }
}

impl<S,D,R> SpawnUnyield for ThreadHandleBlock<S,D,R> {
  #[inline]
  fn is_finished(&mut self) -> bool {
    self.0.lock().is_some()
  }
  #[inline]
  fn unyield(&mut self) -> Result<()> {
    Ok(())
  }
}
impl<S,D,R> SpawnHandle<S,D,R> for ThreadHandleBlock<S,D,R> {
  type WeakHandle = ThreadHandleBlockWeak<S,D,R>;
  #[inline]
  fn unwrap_state(self) -> Result<(S,D,R,Result<()>)> {
    let mut mlock = self.0.lock();
    if mlock.is_some() {
      //let ost = mutex.into_inner();
      let ost = replace(&mut (*mlock),None);
      Ok(ost.unwrap())
    } else {
      Err(Error("unwrap state on unfinished thread".to_string(), ErrorKind::Bug, None))
    }
  }

  #[inline]
  fn get_weak_handle(&self) -> Option<Self::WeakHandle> {
    Some(ThreadHandleBlockWeak(self.0.clone()))
  }
}




impl SpawnerYield for ThreadYieldBlock {
  #[inline]
  fn spawn_yield(&mut self) -> YieldReturn {
    thread::yield_now();
    YieldReturn::Loop
  }
}


//macro_rules! thread_spawn {($spawner:ident,$handle:ident,$yield:ident) => {

impl<S : 'static + Send + Service, D : 'static + Send + SpawnSend<S::CommandOut>, R : 'static + Send + SpawnRecv<S::CommandIn>> 
  Spawner<S,D,R> for ThreadBlock
  where S::CommandIn : Send
{
  type Handle = ThreadHandleBlock<S,D,R>;
  type Yield = ThreadYieldBlock;
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
      let mut err = Ok(());
      spawn_loop!(service,spawn_out,ocin,recv,nb_loop,ThreadYieldBlock,err,Err(Error("Thread block spawn service return would return when should loop".to_string(), ErrorKind::Bug, None)));
      let mut data = finished.lock();
      *data = Some((service,spawn_out,recv,err));
      Ok(())
    })?;
    return Ok(ThreadHandleBlock(finished2,join_handle));
  }
}




/// park thread on yield, unpark with Handle.
/// Std thread park not used for parking_lot mutex usage, but implementation
/// is the same.
/// It must be notice that after a call to unyield, yield will skip (avoid possible lock).
pub struct ThreadPark;
/// variant of thread park using command and channel as Ref to obtain send versio 
pub struct ThreadParkRef;
pub struct ThreadHandleParkWeak<S,D,R>(Arc<Mutex<Option<(S,D,R,Result<()>)>>>,(),Arc<(Mutex<bool>,Condvar)>);
impl<S,D,R> Clone for ThreadHandleParkWeak<S,D,R> {
  fn clone(&self) -> Self {
    ThreadHandleParkWeak(self.0.clone(),(),self.2.clone())
  }
}

pub struct ThreadHandlePark<S,D,R>(Arc<Mutex<Option<(S,D,R,Result<()>)>>>,JoinHandle<Result<()>>,Arc<(Mutex<bool>,Condvar)>);
pub struct ThreadHandleParkRef<S : SRef,D : SRef,R : SRef>(Arc<Mutex<Option<(S::Send,D::Send,R::Send,Result<()>)>>>,JoinHandle<Result<()>>,Arc<(Mutex<bool>,Condvar)>);

pub struct ThreadYieldPark(Arc<(Mutex<bool>,Condvar)>);

macro_rules! thread_parkunyield(() => (
  #[inline]
  fn is_finished(&mut self) -> bool {
    self.0.lock().is_some()
  }
  #[inline]
  fn unyield(&mut self) -> Result<()> {
    let mut guard = (self.2).0.lock();
    if !*guard {
      *guard = true;
      (self.2).1.notify_one();
    }
    Ok(())
  }

));



impl<S,D,R> SpawnUnyield for ThreadHandleParkWeak<S,D,R> {
  thread_parkunyield!();
}

impl<S : SRef,D : SRef,R : SRef> SpawnUnyield for ThreadHandleParkRef<S,D,R> {
  thread_parkunyield!();
}


impl<S,D,R> SpawnUnyield for ThreadHandlePark<S,D,R> {
  thread_parkunyield!();
}
impl<S : SRef,D : SRef,R : SRef> SpawnHandle<S,D,R> for ThreadHandleParkRef<S,D,R> {
//  type WeakHandle = NoWeakHandle;
  type WeakHandle = ThreadHandleParkWeak<<S as SRef>::Send,<D as SRef>::Send,<R as SRef>::Send>;
  #[inline]
  fn unwrap_state(self) -> Result<(S,D,R,Result<()>)> {
    let mut mlock = self.0.lock();
    if mlock.is_some() {
      //let ost = mutex.into_inner();
      let ost = replace(&mut (*mlock),None);
      let (s,d,r,rr) = ost.unwrap();
      Ok((s.to_ref(),d.to_ref(),r.to_ref(),rr))
    } else {
      Err(Error("unwrap state on unfinished thread".to_string(), ErrorKind::Bug, None))
    }
  }

  #[inline]
  fn get_weak_handle(&self) -> Option<Self::WeakHandle> {
    Some(ThreadHandleParkWeak(self.0.clone(),(),self.2.clone()))
  }
}


impl<S,D,R> SpawnHandle<S,D,R> for ThreadHandlePark<S,D,R> {
  type WeakHandle = ThreadHandleParkWeak<S,D,R>;
  #[inline]
  fn unwrap_state(self) -> Result<(S,D,R,Result<()>)> {
    let mut mlock = self.0.lock();
    if mlock.is_some() {
      //let ost = mutex.into_inner();
      let ost = replace(&mut (*mlock),None);
      Ok(ost.unwrap())
    } else {
      Err(Error("unwrap state on unfinished thread".to_string(), ErrorKind::Bug, None))
    }
  }

  #[inline]
  fn get_weak_handle(&self) -> Option<Self::WeakHandle> {
    Some(ThreadHandleParkWeak(self.0.clone(),(),self.2.clone()))
  }
}


impl SpawnerYield for ThreadYieldPark {
  #[inline]
  fn spawn_yield(&mut self) -> YieldReturn {
    //if self.0.compare_and_swap(true, false, Ordering::Acquire) != true {
  //  if let Err(status) = (self.0).0.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed) {
   // } else {
      // status 0, we yield only if running, in other state we simply loop and will yield at next
      // call TODO could optimize by using mutex directly 
      
      // park (thread::park();)
    {
      let mut guard = (self.0).0.lock();
//      (self.0).0.store(2,Ordering::Release);
      while !*guard {
        (self.0).1.wait(&mut guard);
        //guard = (self.0).1.wait(guard);
      }
      *guard = false;
    }
    //}
    YieldReturn::Loop
  }
}


impl<S : 'static + Send + Service, D : 'static + Send + SpawnSend<S::CommandOut>, R : 'static + Send + SpawnRecv<S::CommandIn>> 
  Spawner<S,D,R> for ThreadPark
  where S::CommandIn : Send
{
  type Handle = ThreadHandlePark<S,D,R>;
  type Yield = ThreadYieldPark;
  fn spawn (
    &mut self,
    mut service : S,
    mut spawn_out : D,
    mut ocin : Option<<S as Service>::CommandIn>,
    mut recv : R,
    mut nb_loop : usize
  ) -> Result<Self::Handle> {
    let skip = Arc::new((Mutex::new(false),Condvar::new()));
    let skip2 = skip.clone();
    let finished = Arc::new(Mutex::new(None));
    let finished2 = finished.clone();
    let join_handle = thread::Builder::new().spawn(move ||{
      let mut y = ThreadYieldPark(skip2);

      let mut err = Ok(());
      spawn_loop!(service,spawn_out,ocin,recv,nb_loop,y,err,Err(Error("Thread park spawn service return would return when should loop".to_string(), ErrorKind::Bug, None)));
      let mut data = finished.lock();
      *data = Some((service,spawn_out,recv,err));
      Ok(())
    })?;
    return Ok(ThreadHandlePark(finished2,join_handle,skip));
  }
}

//impl<C : Ref<C>> Ref<MpscSenderRef<C>> for MpscSenderRef<C> {
impl<S : 'static + Service + SRef, D : SRef + 'static + SpawnSend<S::CommandOut>, R : 'static + SRef + SpawnRecv<S::CommandIn>> 
  Spawner<S,D,R> for ThreadParkRef
  where D::Send : SpawnSend<S::CommandOut>,
        R::Send : SpawnRecv<S::CommandIn>,
        <S as Service>::CommandIn : SRef,
{
 
/*impl<S : 'static + Send + Service, D : 'static + Send + SpawnSend<S::CommandOut>, R : 'static + Send + SpawnRecv<
<S::CommandIn as Ref<S::CommandIn>>::ToRef<S::CommandIn>
>> 
  Spawner<S,D,R> for ThreadParkRef
  where S::CommandIn : Ref<S::CommandIn>
{*/
  type Handle = ThreadHandleParkRef<S,D,R>;
  type Yield = ThreadYieldPark;
  fn spawn (
    &mut self,
    service : S,
    spawn_out : D,
    ocin : Option<<S as Service>::CommandIn>,
    recv : R,
    mut nb_loop : usize
  ) -> Result<Self::Handle> {
    let skip = Arc::new((Mutex::new(false),Condvar::new()));
    let skip2 = skip.clone();
    let finished = Arc::new(Mutex::new(None));
    let finished2 = finished.clone();
    let service_ref = service.get_sendable();
    let mut spawn_out_s = spawn_out.get_sendable();
    let mut recv_s = recv.get_sendable();
    let ocins = ocin.map(|cin|cin.get_sendable());
    let join_handle = thread::Builder::new().spawn(move ||{
      let mut service = service_ref.to_ref();
      let mut y = ThreadYieldPark(skip2);
      let mut err = Ok(());
      let mut ocin = ocins.map(|cin|cin.to_ref());
      spawn_loop!(service,spawn_out_s,ocin,recv_s,nb_loop,y,err,Err(Error("Thread park spawn service return would return when should loop".to_string(), ErrorKind::Bug, None)));
      let mut data = finished.lock();
      *data = Some((service.get_sendable(),spawn_out_s,recv_s,err));
      Ok(())
    })?;
    return Ok(ThreadHandleParkRef(finished2,join_handle,skip));
  }
}







/// TODO move in its own crate or under feature in mydht : cpupool dependency in mydht-base is
/// bad (future may be in api so fine)
/// This use CpuPool but not really the future abstraction
pub struct CpuPool(FCpuPool);
// TODO simpliest type with Result<() everywhere).
pub struct CpuPoolHandle<S,D,R>(CpuFuture<(S,D,R,Result<()>),Error>,Option<(S,D,R,Result<()>)>);
pub struct CpuPoolYield;

impl<S : Send + 'static, D : Send + 'static, R : Send + 'static> SpawnUnyield for CpuPoolHandle<S,D,R> {
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
}
impl<S : Send + 'static, D : Send + 'static, R : Send + 'static> SpawnHandle<S,D,R> for CpuPoolHandle<S,D,R> {
  // weakHandle is doable but require some sync overhead (arc mutex)
  type WeakHandle = NoWeakHandle;
  #[inline]
  fn unwrap_state(mut self) -> Result<(S,D,R,Result<()>)> {
    if self.1.is_some() {
      let r = replace(&mut self.1,None);
      return Ok(r.unwrap())
    }
    self.0.wait()
  }
  #[inline]
  fn get_weak_handle(&self) -> Option<Self::WeakHandle> {
    None
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
      match move || -> Result<(S,D,R,Result<()>)> {
        let mut err = Ok(());
        spawn_loop!(service,spawn_out,ocin,recv,nb_loop,CpuPoolYield,err,Err(Error("CpuPool spawn service return would return when should loop".to_string(), ErrorKind::Bug, None)));
        Ok((service,spawn_out,recv,err))
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
pub struct CpuPoolHandleFuture<S,D,R>(CpuFuture<(S,D,Result<()>),Error>,Option<(S,D,Result<()>)>,R);

impl<S : Send + 'static + Service,R, D : 'static + Send + SpawnSend<S::CommandOut>> SpawnUnyield for CpuPoolHandleFuture<S,D,R> {
  #[inline]
  fn is_finished(&mut self) -> bool {
    match self.0.poll() {
      Ok(Async::Ready(r)) => {
        self.1 = Some((r.0,r.1,Ok(())));
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
}
impl<S : Send + 'static + Service,R, D : 'static + Send + SpawnSend<S::CommandOut>> SpawnHandle<S,D,R> for CpuPoolHandleFuture<S,D,R> {
  // TODO could be implemented with arc mutex our content
  type WeakHandle = NoWeakHandle;
  #[inline]
  fn unwrap_state(mut self) -> Result<(S,D,R,Result<()>)> {
    if self.1.is_some() {
      if let Some((s,d,res)) = replace(&mut self.1,None) {
        return Ok((s,d,self.2,res))
      } else {
        unreachable!()
      }
    }
    match self.0.wait() {
      Ok((s,d,r)) => Ok((s,d,self.2,r)),
      Err(e) => Err(e),
    }
  }
  #[inline]
  fn get_weak_handle(&self) -> Option<Self::WeakHandle> {
    None
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
    let mut future = self.0.spawn(okfuture((service,spawn_out,Ok(()))));
    loop {
      match ocin {
        Some(cin) => {
          future = self.0.spawn(future.and_then(|(mut service, mut spawn_out,_)| {
            match service.call(cin, &mut CpuPoolYield) {
              Ok(r) => {
                if D::CAN_SEND {
                  match spawn_out.send(r) {
                    Ok(_) => (),
                    Err(e) => return errfuture(e),
                  }
                }
                return okfuture((service,spawn_out,Ok(())));
              },
              Err(e) => if e.level() == ErrorLevel::Ignore {
                panic!("This should only yield loop, there is an issue with implementation");
              } else if e.level() == ErrorLevel::Panic {
                panic!("In spawner cpufuture panic : {:?} ",e);
              } else {
                return okfuture((service,spawn_out,Err(e)));
//                return errfuture((service,spawn_out,e))
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


