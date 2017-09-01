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
use std::marker::PhantomData;
use mydhtresult::{
  Result,
  Error,
  ErrorKind,
  ErrorLevel,
  ErrorLevel as MdhtErrorLevel,
};
use std::mem::replace;
use std::collections::vec_deque::VecDeque;

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
  fn call<S : SpawnerYield>(&mut self, req: Option<Self::CommandIn>, async_yield : S) -> Result<Self::CommandOut>;
}

pub trait Spawner<S : Service, D : SpawnSend<<S as Service>::CommandOut>> {
  type Send : SpawnSend<<S as Service>::CommandIn>;
  type Handle : SpawnHandle<S>;
  type Yield : SpawnerYield;
  /// use 0 as nb loop if service should loop forever (for instance on a read receiver or a kvstore
  /// service)
  fn spawn (
    &mut self,
    service : S,
    spawn_out : D,
//    state : Option<<S as Service>::State>,
    Option<<S as Service>::CommandIn>,
    nb_loop : usize // infinite if 0
  ) -> Result<(Self::Handle,Self::Send)>;
}

/// send command to spawn
pub trait SpawnSend<Command> : Sized {
  /// if send is disable set to false, this can be test before calling `send`
  const CAN_SEND : bool;
  /// mut on self is currently not needed by impl TODO if easier without at some point, could be remove
  fn send(&mut self, Command) -> Result<()>;
}

/// Handle use to send command to get back state
/// State in the handle is simply the Service struct
pub trait SpawnHandle<State> {
  fn is_finished(&self) -> bool;
  fn unyield(&mut self) -> Result<()>;
  fn unwrap_state(self) -> Result<State>;
}

/// manages asynch call by possibly yielding process (yield a coroutine if same thread, park or
/// yield a thread, do nothing (block)
pub trait SpawnerYield {
  fn spawn_yield(&self) -> YieldReturn;
}

#[derive(Clone,Copy)]
pub enum YieldReturn {
  /// end spawn, transmit state to handle. Some Service may be restartable if enough info in state.
  /// This return an ignore Mydht level error.
  Return,
  /// retry at the same point (stack should be the same)
  Loop,
}

pub struct NoSend;
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
  fn spawn_yield(&self) -> YieldReturn {
    self.0
  }
}

pub struct BlockingSameThread<S>(S);
impl<S> SpawnHandle<S> for BlockingSameThread<S> {
  #[inline]
  fn is_finished(&self) -> bool {
    true // if called it must be finished
  }
  #[inline]
  fn unyield(&mut self) -> Result<()> {
    Ok(())
  }
  #[inline]
  fn unwrap_state(self) -> Result<S> {
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

impl<S : Service, D : SpawnSend<<S as Service>::CommandOut>> Spawner<S,D> for Blocker {
  type Send = NoSend;
  type Handle = BlockingSameThread<S>;
  type Yield = NoYield;
  fn spawn (
    &mut self,
    mut service : S,
    mut spawn_out : D,
    mut ocin : Option<<S as Service>::CommandIn>,
    mut nb_loop : usize
  ) -> Result<(Self::Handle,Self::Send)> {
    loop {
      let cmd_out = service.call(ocin, NoYield(YieldReturn::Loop))?;
      ocin = None;
      if D::CAN_SEND {
        spawn_out.send(cmd_out)?;
      }
      if nb_loop > 0 {
        nb_loop -= 1;
        if nb_loop == 0 {
          break;
        }
      }
    }
    return Ok((BlockingSameThread(service),NoSend))
  }
}



pub struct RestartSpawn<S : Service,D : SpawnSend<<S as Service>::CommandOut>, SP : Spawner<S,D>> {
  spawner : SP,
  service : S,
  spawn_out : D,
  nb_loop : usize,
}

pub enum RestartSameThread<S : Service,D : SpawnSend<<S as Service>::CommandOut>, SP : Spawner<S,D>> {
  ToRestart(RestartSpawn<S,D,SP>),
  Ended(S),
  // technical
  Empty,
}

impl<S : Service,D : SpawnSend<<S as Service>::CommandOut>>
  SpawnHandle<S> for 
  RestartSameThread<S,D,RestartOrError> {
  #[inline]
  fn is_finished(&self) -> bool {
    if let &RestartSameThread::Ended(_) = self {
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
        nb_loop,
      }) = tr {
        let (rs,_) = spawner.spawn(service,spawn_out,None,nb_loop)?;
        replace(self, rs);
      } else {
        unreachable!();
      }
    }
    Ok(())
  }

  #[inline]
  fn unwrap_state(mut self) -> Result<S> {
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

impl<S : Service, D : SpawnSend<<S as Service>::CommandOut>> Spawner<S,D> for RestartOrError {
  type Send = NoSend;
  type Handle = RestartSameThread<S,D,Self>;
  type Yield = NoYield;
  fn spawn (
    &mut self,
    mut service : S,
    mut spawn_out : D,
    mut ocin : Option<<S as Service>::CommandIn>,
    mut nb_loop : usize
  ) -> Result<(Self::Handle,Self::Send)> {
    loop {
      let cmd_out_r = service.call(ocin, NoYield(YieldReturn::Return));
      ocin = None;
      if let Some(cmd_out) = try_ignore!(cmd_out_r, "In restart on Error spawner : {:?}") {
        if D::CAN_SEND {
          spawn_out.send(cmd_out)?;
        }
      } else {
        return  Ok((RestartSameThread::ToRestart(
              RestartSpawn {
                spawner : RestartOrError,
                service : service,
                spawn_out : spawn_out,
                nb_loop : nb_loop,
              }
              ),NoSend))
      }
      if nb_loop > 0 {
        nb_loop -= 1;
        if nb_loop == 0 {
          break;
        }
      }
    }
    return Ok((RestartSameThread::Ended(service),NoSend))
  }
}



pub struct Coroutine;

pub struct ThreadBlock;

pub struct ThreadPark;

pub struct CpuPool;
