//! Main loop for mydht. This is the main dht loop, see default implementation of MyDHT main_loop
//! method.
//! Usage of mydht library requires to create a struct implementing the MyDHTConf trait, by linking with suitable inner trait implementation and their requires component.

use time::Duration as CrateDuration;
use std::marker::PhantomData;
use std::mem::replace;
use mydhtresult::{
  Result,
  Error,
  ErrorKind,
  ErrorLevel as MdhtErrorLevel,
};
use service::{
  Service,
  Spawner,
  SpawnSend,
  SpawnRecv,
  SpawnHandle,
  SpawnChannel,
  MioChannel,
  MioSend,
  MioRecv,
  NoYield,
  YieldReturn,
  SpawnerYield,
  WriteYield,
  ReadYield,
  DefaultRecv,
  DefaultRecvChannel,
  NoRecv,
  NoSend,
};
use std::rc::Rc;
use std::cell::Cell;
use std::thread;
use std::thread::{
  Builder as ThreadBuilder,
};
use std::sync::atomic::{
  AtomicUsize,
};
use std::sync::Arc;

use std::sync::mpsc::{
  Sender,
  Receiver,
  channel,
};
use peer::{
  Peer,
  PeerMgmtMeths,
};
use kvcache::{
  SlabCache,
  KVCache,
};
use keyval::KeyVal;
use rules::DHTRules;
use msgenc::MsgEnc;
use transport::{
  Transport,
  Address,
  Address as TransportAddress,
  SlabEntry,
  SlabEntryState,
  Registerable,
};
use utils::{
  Ref,
};
use std::io::{
  Read,
  Write,
};
use mio::{
  SetReadiness,
  Registration,
  Events,
  Poll,
  Ready,
  PollOpt,
  Token
};
macro_rules! register_state {($self:ident,$rs:ident,$os:expr,$entrystate:path) => {
  {
    let token = $self.slab_cache.insert(SlabEntry {
      state : $entrystate($rs),
      os : $os,
      peer : None,
    });
    if let $entrystate(ref mut rs) = $self.slab_cache.get_mut(token).unwrap().state {
      assert!(true == rs.register(&$self.poll, Token(token + START_STREAM_IX), Ready::readable(),
        PollOpt::edge())?);
    } else {
      unreachable!();
    }
    token
  }
}}
macro_rules! register_state_w {($self:ident,$pollopt:expr,$rs:ident,$wb:expr,$os:expr,$entrystate:path) => {
  {
    let token = $self.slab_cache.insert(SlabEntry {
      state : $entrystate($rs,$wb),
      os : $os,
      peer : None,
    });
    if let $entrystate(ref mut rs,_) = $self.slab_cache.get_mut(token).unwrap().state {
      assert!(true == rs.register(&$self.poll, Token(token + START_STREAM_IX), Ready::writable(),
      $pollopt )?);
    } else {
      unreachable!();
    }
    token
  }
}}



pub const CACHE_NO_STREAM : usize = 0;
pub const CACHE_CLOSED_STREAM : usize = 1;
const LISTENER : Token = Token(0);
const LOOP_COMMAND : Token = Token(1);
/// Must be more than registered token (cf other constant) and more than atomic cache status for
/// PeerCache const
const START_STREAM_IX : usize = 2;

/*
pub fn boot_server
 <T : Route<RT::A,RT::P,RT::V,RT::T>, 
  QC : QueryCache<RT::P,RT::V>, 
  S : KVStore<RT::V>,
  F1 : FnOnce() -> Option<S> + Send + 'static,
  F2 : FnOnce() -> Option<T> + Send + 'static,
  F3 : FnOnce() -> Option<QC> + Send + 'static,
 >
 (rc : ArcRunningContext<RT>, 
  route : F2, 
  querycache : F3, 
  kvst : F1,
  cached_nodes : Vec<Arc<RT::P>>, 
  boot_nodes : Vec<Arc<RT::P>>,
  ) 
 -> MDHTResult<DHT<RT>> {
*/
/// implementation of api TODO move in its own module
/// Allow call from Rust method or Foreign library
pub struct MyDHT<MC : MyDHTConf>(MainLoopSendIn<MC>);

/// command supported by MyDHT loop
pub enum MainLoopCommand<MC : MyDHTConf> {
  Start,
  TryConnect(<MC::Transport as Transport>::Address),
}

pub type RWSlabEntry<MDC : MyDHTConf> = SlabEntry<
  MDC::Transport,
//  (),
  //SpawnerRefsRead2<ReadService<MDC>, MDC::ReadDest, MDC::ReadChannelIn, MDC::ReadSpawn>,
  SpawnerRefsDefRecv2<ReadService<MDC>,ReadServiceCommand, MDC::ReadDest, MDC::ReadChannelIn, MDC::ReadSpawn>,
//  SpawnerRefs<ReadService<MDC>,MDC::ReadDest,DefaultRecvChannel<ReadServiceCommand,MDC::ReadChannelIn>,MDC::ReadSpawn>,
  SpawnerRefs2<WriteService<MDC>,WriteServiceCommand,MDC::WriteDest,MDC::WriteChannelIn,MDC::WriteSpawn>,
  // bool is has_connect
  (<MDC::WriteChannelIn as SpawnChannel<WriteServiceCommand>>::Send,<MDC::WriteChannelIn as SpawnChannel<WriteServiceCommand>>::Recv,bool),
  MDC::PeerRef>;

type SpawnerRefs<S : Service,D,CI : SpawnChannel<S::CommandIn>,SP : Spawner<S,D,CI::Recv>> = (SP::Handle,CI::Send); 
type SpawnerRefs2<S : Service,COM_IN, D,CI : SpawnChannel<COM_IN>,SP : Spawner<S,D,CI::Recv>> = (SP::Handle,CI::Send); 
type SpawnerRefsDefRecv2<S : Service,COM_IN,D, CI : SpawnChannel<COM_IN>, RS : Spawner<S,D,DefaultRecv<COM_IN,CI::Recv>>> = (RS::Handle,CI::Send);
type SpawnerRefsRead2<S : Service,D, CI : SpawnChannel<ReadServiceCommand>, RS : Spawner<S,D,DefaultRecv<ReadServiceCommand,CI::Recv>>> = (RS::Handle,CI::Send);

/*pub trait Spawner<
  S : Service,
  D : SpawnSend<<S as Service>::CommandOut>,
  R : SpawnRecv<<S as Service>::CommandIn>> {
*/ 
  //CI::Send); 
type MainLoopRecvIn<MDC : MyDHTConf> = MioRecv<<MDC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MDC>>>::Recv>;
type MainLoopSendIn<MDC : MyDHTConf> = MioSend<<MDC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MDC>>>::Send>;

pub enum MainLoopReply {
  /// TODO
  Ended,
}
pub trait MyDHTConf : 'static + Send + Sized {

  /// Name of the main thread
  const loop_name : &'static str = "MyDHT Main Loop";
  /// number of events to poll (size of mio `Events`)
  const events_size : usize = 1024;
  /// number of iteration before send loop return, 1 is suggested, but if thread are involve a
  /// little more should be better, in a pool infinite (0) could be fine to.
  const send_nb_iter : usize;
  /// Spawner for main loop
  type MainloopSpawn : Spawner<
    MyDHTService<Self>,
    //<Self::MainLoopChannelOut as SpawnChannel<MainLoopReply>>::Send,
    NoSend,
    //MioRecv<<Self::MainLoopChannelIn as SpawnChannel<MainLoopCommand<Self>>>::Recv>
    NoRecv
      >;
      

  /// channel for input
  type MainLoopChannelIn : SpawnChannel<MainLoopCommand<Self>>;
  /// TODO out mydht command
  type MainLoopChannelOut : SpawnChannel<MainLoopReply>;
  /// low level transport
  type Transport : Transport;
  /// Message encoding
  type MsgEnc : MsgEnc;
  /// Peer struct (with key and address)
  type Peer : Peer<Address = <Self::Transport as Transport>::Address>;
  /// most of the time Arc, if not much threading or smal peer description, RcCloneOnSend can be use, or AllwaysCopy
  /// or Copy.
  type PeerRef : Ref<Self::Peer>;
  /// shared info
  type KeyVal : KeyVal;
  /// Peer management methods 
  type PeerMgmtMeths : PeerMgmtMeths<Self::Peer, Self::KeyVal>;
  /// Dynamic rules for the dht
  type DHTRules : DHTRules;
  /// loop slab implementation
  type Slab : SlabCache<RWSlabEntry<Self>>;
  /// local cache for peer
  type PeerCache : KVCache<<Self::Peer as KeyVal>::Key,PeerCacheEntry<Self::PeerRef>>;
  
  type ReadChannelIn : SpawnChannel<ReadServiceCommand>;
  //type ReadChannelIn : SpawnChannel<<Self::ReadService as Service>::CommandIn>;
//  type ReadFrom : SpawnRecv<<Self::ReadService as Service>::CommandIn>;
  //type ReadDest : SpawnSend<<Self::ReadService as Service>::CommandOut>;
  type ReadDest : SpawnSend<()> + Clone;
  type ReadSpawn : Spawner<
    ReadService<Self>,
    Self::ReadDest,
    DefaultRecv<ReadServiceCommand,
      <Self::ReadChannelIn as SpawnChannel<ReadServiceCommand>>::Recv>>;

  type WriteDest : SpawnSend<()> + Clone;
  type WriteChannelIn : SpawnChannel<WriteServiceCommand>;
//  type WriteFrom : SpawnRecv<<Self::WriteService as Service>::CommandIn>;
  type WriteSpawn : Spawner<WriteService<Self>,Self::WriteDest,<Self::WriteChannelIn as SpawnChannel<WriteServiceCommand>>::Recv>;

  // TODO temp for type check , hardcode it after on transport service (not true for kvstore service) the service contains the read stream!!
//  type ReadService : Service;
  // TODO call to read and write in service will use ReadYield and WriteYield wrappers containing
  // the spawn yield (cf sample implementation in transport tests).
  //type WriteService : Service;


  /// Start the main loop
  #[inline]
  fn start_loop(mut self : Self) -> Result<(
    MioSend<<Self::MainLoopChannelIn as SpawnChannel<MainLoopCommand<Self>>>::Send>, 
    <Self::MainLoopChannelOut as SpawnChannel<MainLoopReply>>::Recv
    )> {
    let (s,r) = MioChannel(self.init_main_loop_channel_in()?).new()?;
    let (so,ro) = self.init_main_loop_channel_out()?.new()?;

    let mut sp = self.get_main_spawner()?;
    //let read_handle = self.read_spawn.spawn(ReadService(rs), read_out.clone(), Some(ReadServiceCommand::Run), read_r_in, 0)?;
    let service = MyDHTService(self,r,so);
    // the  spawn loop is not use, the poll loop is : here we run a single loop without receive
    sp.spawn(service, NoSend, Some(MainLoopCommand::Start), NoRecv, 1)?; 
    // TODO replace this shit by a spawner then remove constraint on MDht trait where
  /*  ThreadBuilder::new().name(Self::loop_name.to_string()).spawn(move || {
      let mut state = self.init_state(r)?;
      let mut yield_spawn = NoYield(YieldReturn::Loop);
      let r = state.main_loop(&mut yield_spawn);
      if r.is_err() {
        panic!("mainloop err : {:?}",&r);
      }
      r
    })?;*/
    Ok((s,ro))
  }

  fn init_state(self : &mut Self) -> Result<MDHTState<Self>> {
    let events = Events::with_capacity(Self::events_size);
    let poll = Poll::new()?;
    let transport = self.init_transport()?;
    assert!(true == transport.register(&poll, LISTENER, Ready::readable(),
                    PollOpt::edge())?);

    let read_spawn = self.init_read_spawner()?;
    let write_spawn = self.init_write_spawner()?;
    let read_channel_in = DefaultRecvChannel(self.init_read_channel_in()?,ReadServiceCommand::Run);
    let write_channel_in = self.init_write_channel_in()?;
    let s = MDHTState {
      transport : transport,
      slab_cache : self.init_main_loop_slab_cache()?,
      peer_cache : self.init_main_loop_peer_cache()?,
      events : None,
      poll : poll,
      read_spawn : read_spawn,
      read_channel_in : read_channel_in,
      write_spawn : write_spawn,
      write_channel_in : write_channel_in,
    };
    Ok(s)
  }


  /// for start_loop usage
  fn get_main_spawner(&mut self) -> Result<Self::MainloopSpawn>;
  /// cache initializing for main loop slab cache
  fn init_main_loop_slab_cache(&mut self) -> Result<Self::Slab>;

  /// Peer cache initialization
  fn init_main_loop_peer_cache(&mut self) -> Result<Self::PeerCache>;

  /// Main loop channel input builder
  fn init_main_loop_channel_in(&mut self) -> Result<Self::MainLoopChannelIn>;
  fn init_main_loop_channel_out(&mut self) -> Result<Self::MainLoopChannelOut>;

  /// instantiate read spawner
  fn init_read_spawner(&mut self) -> Result<Self::ReadSpawn>;
  fn init_write_spawner(&mut self) -> Result<Self::WriteSpawn>;

  /// TODO replace with channel
  fn init_read_spawner_out() -> Result<Self::ReadDest>;
  fn init_write_spawner_out() -> Result<Self::WriteDest>;
  fn init_read_channel_in(&mut self) -> Result<Self::ReadChannelIn>;
  fn init_write_channel_in(&mut self) -> Result<Self::WriteChannelIn>;

  /// Transport initialization
  fn init_transport(&mut self) -> Result<Self::Transport>;



}

pub struct MDHTState<MDC : MyDHTConf> {
  transport : MDC::Transport,
  slab_cache : MDC::Slab,
  peer_cache : MDC::PeerCache,
  events : Option<Events>,
  poll : Poll,
  read_spawn : MDC::ReadSpawn,
  read_channel_in : DefaultRecvChannel<ReadServiceCommand, MDC::ReadChannelIn>,
  write_spawn : MDC::WriteSpawn,
  write_channel_in : MDC::WriteChannelIn,
}

impl<MDC : MyDHTConf> MDHTState<MDC> {

  /// sub call for service (actually mor relevant service implementation : the call will only
  /// return one CommandOut to spawner : finished or fail).
  /// Async yield being into self.
  #[inline]
  fn call_inner_loop<S : SpawnerYield>(&mut self, req: MainLoopCommand<MDC>, async_yield : &mut S) -> Result<()> {
    match req {
      MainLoopCommand::Start => {
        // do nothing it has start if the function was called
      },
      MainLoopCommand::TryConnect(dest_address) => {
        // TODO duration will be removed
        let (ws,ors) = self.transport.connectwith(&dest_address, CrateDuration::seconds(1000))?;

        let (s,r) = self.write_channel_in.new()?;

        // register writestream
        let write_token = register_state_w!(self,PollOpt::edge(),ws,(s,r,false),None,SlabEntryState::WriteStream);

        // register readstream for multiplex transport
        ors.map(|rs| -> Result<()> {

          let read_token = register_state!(self,rs,Some(write_token),SlabEntryState::ReadStream);
          // update read reference
          self.slab_cache.get_mut(write_token).map(|r|r.os = Some(read_token));
          Ok(())
        }).unwrap_or(Ok(()))?;

        // TODO send a ping not this shiet testing command
        self.write_stream_send(write_token,WriteServiceCommand::Write , <MDC>::init_write_spawner_out()?)?;

      },
    }

    Ok(())
  }

//  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, async_yield : &mut S) -> Result<Self::CommandOut>;
  fn main_loop<S : SpawnerYield>(&mut self,rec : &mut MioRecv<<MDC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MDC>>>::Recv>, req: MainLoopCommand<MDC>, async_yield : &mut S) -> Result<()> {
   let mut events = if self.events.is_some() {
     let oevents = replace(&mut self.events,None);
     oevents.unwrap_or(Events::with_capacity(MDC::events_size))
   } else {
     Events::with_capacity(MDC::events_size)
   };
   self.inner_main_loop(rec, req, async_yield, &mut events).map_err(|e|{
     self.events = Some(events);
     e
   })
  }
  fn inner_main_loop<S : SpawnerYield>(&mut self, receiver : &mut MioRecv<<MDC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MDC>>>::Recv>, req: MainLoopCommand<MDC>, async_yield : &mut S, events : &mut Events) -> Result<()> {

    assert!(true == receiver.register(&self.poll, LOOP_COMMAND, Ready::readable(),
                      PollOpt::edge())?);

    let cout = self.call_inner_loop(req,async_yield)?;
    // TODO cout in sender WHen implementing spawne
    loop {
      self.poll.poll(events, None)?;
      for event in events.iter() {
        match event.token() {
          LOOP_COMMAND => {
            let ocin = receiver.recv()?;
            if let Some(cin) = ocin {
              let cout = self.call_inner_loop(cin,async_yield)?;
              // TODO cout in sender WHen implementing spawne
            }
            //  continue;
              // Do not yield on empty receive (could be spurrious), yield from loop is only
              // allowed for suspend/stop commands
              // yield_spawn.spawn_yield();
          },
          LISTENER => {
              let read_token = try_breakloop!(self.transport.accept(), "Transport accept failure : {}", 
              |(rs,ows,ad) : (<MDC::Transport as Transport>::ReadStream,Option<<MDC::Transport as Transport>::WriteStream>,<MDC::Transport as Transport>::Address)| -> Result<usize> {
/*              let wad = if ows.is_some() {
                  Some(ad.clone())
              } else {
                  None
              };*/
              // register readstream
              let read_token = register_state!(self,rs,None,SlabEntryState::ReadStream);

              // register writestream for multiplex transport
              ows.map(|ws| -> Result<()> {

                let (s,r) = self.write_channel_in.new()?;
                // connection done on listener so edge immediatly and state has_connect to true
                let write_token = register_state_w!(self,PollOpt::edge(),ws,(s,r,true),Some(read_token),SlabEntryState::WriteStream);
                // update read reference
                self.slab_cache.get_mut(read_token).map(|r|r.os = Some(write_token));
                Ok(())
              }).unwrap_or(Ok(()))?;

              // do not start listening on read, it will start when poll trigger readable on
              // connect
              //Self::start_read_stream_listener(self, read_token, <MDC>::init_read_spawner_out()?)?;
 

              Ok(read_token)
            });

 
          },
          tok => {
            let (sp_read, o_sp_write) =  if let Some(ca) = self.slab_cache.get_mut(tok.0 - START_STREAM_IX) {
              match ca.state {
                SlabEntryState::ReadStream(_) => {
                  (true,None)
                },
                SlabEntryState::WriteStream(ref mut ws,(_,ref mut write_r_in,ref mut has_connect)) => {
                  // case where spawn reach its nb_loop and return, should not happen as yield is
                  // only out of service call (nb_loop test is out of nb call) for receiver which is not registered.
                  // Yet if WriteService could resume which is actually not the case we could have a
                  // spawneryield return and would need to resume with a resume command.
                  // self.write_stream_send(write_token,WriteServiceCommand::Resume , <MDC>::init_write_spawner_out()?)?;
                  
                  // also case when multiplex transport and write stream is ready but nothing to
                  // send : spawn of write happens on first write.
                  //TODO a debug log??
                  if *has_connect == false {
                    *has_connect = true;
                    // reregister at a edge level
                    //assert!(true == ws.reregister(&self.poll, tok, Ready::readable(),PollOpt::edge())?);
                  }
                  let oc = write_r_in.recv()?;

                  (false,oc)
                },
                SlabEntryState::ReadSpawned((ref mut handle,_)) => {
                  handle.unyield()?;
                  (false,None)
                },
                SlabEntryState::WriteSpawned((ref mut handle,_)) => {
                  handle.unyield()?;
                  (false,None)
                },
                SlabEntryState::Empty => {
                  unreachable!()
                },
              }
            } else {
              // TODO replace by logging as it should happen depending on transport implementation
              // (or spurrious poll), keep it now for testing purpose
              panic!("Unregistered token polled");
              (false,None)
            };
            if sp_read {
              self.start_read_stream_listener(tok.0 - START_STREAM_IX, <MDC>::init_read_spawner_out()?)?;
            }
            if o_sp_write.is_some() {
              self.write_stream_send(tok.0 - START_STREAM_IX, o_sp_write.unwrap(), <MDC>::init_write_spawner_out()?)?;
            }

          },
        }
      }
    }
 
    Ok(())
  }
  /// The state of the slab entry must be checked before, return error on wrong state
  fn start_read_stream_listener(&mut self, read_token : usize,
    read_out : MDC::ReadDest
                                ) -> Result<()> {
    let se = self.slab_cache.get_mut(read_token);
    if let Some(entry) = se {
      if let SlabEntryState::ReadStream(_) = entry.state {
        let state = replace(&mut entry.state,SlabEntryState::Empty);
        let rs = match state {
          SlabEntryState::ReadStream(rs) => rs,
          _ => unreachable!(),
        };
        let (read_s_in,read_r_in) = self.read_channel_in.new()?;
        // spawn reader TODO try with None (default to Run)
        let read_handle = self.read_spawn.spawn(ReadService(rs), read_out.clone(), Some(ReadServiceCommand::Run), read_r_in, 0)?;
        let state = SlabEntryState::ReadSpawned((read_handle,read_s_in));
        replace(&mut entry.state,state);
        return Ok(())
      } else if let SlabEntryState::ReadSpawned(_) = entry.state {
        return Ok(())
      }
    }

    Err(Error("Call of read listener on wrong state".to_string(), ErrorKind::Bug, None))
  }



  /// The state of the slab entry must be checked before, return error on wrong state
  ///  TODO replace dest with channel spawner mut ref
  fn write_stream_send(&mut self, write_token : usize, command : WriteServiceCommand, write_out : MDC::WriteDest) -> Result<()> {
    let se = self.slab_cache.get_mut(write_token);
    if let Some(entry) = se {
      let restart = match entry.state {
        // connected write stream : spawn
        ref mut e @ SlabEntryState::WriteStream(_,(_,_,true)) => {
          let state = replace(e, SlabEntryState::Empty);
          if let SlabEntryState::WriteStream(ws,(mut write_s_in,mut write_r_in,_)) = state {
 //          let (write_s_in,write_r_in) = self.write_channel_in.new()?;
            let oc = write_r_in.recv()?;
            let ocin = if oc.is_some() {
              // for order sake
              write_s_in.send(command)?;
              oc
            } else {
              // empty
              Some(command)
            };
            let write_handle = self.write_spawn.spawn(WriteService(ws), write_out.clone(), ocin, write_r_in, MDC::send_nb_iter)?;
            let state = SlabEntryState::WriteSpawned((write_handle,write_s_in));
            replace(e,state);
            None
          } else {unreachable!()}
        },
        SlabEntryState::WriteStream(_,(ref mut send,_,false)) => {
          // TODO size limit of channel bef connected -> error on send ~= to connection failure
          send.send(command)?;
          None
        },
        SlabEntryState::WriteSpawned((ref mut handle, ref mut sender)) => {
          // TODO refactor to not check at every send
          //
          // TODO  panic!("TODO possible error droprestart"); do it at each is_finished state
          if handle.is_finished() {

            //TODO either drop or start restart?? Need to add functionnality to handle
            //right now we consider restart (send can be short or at least short lived thread)
            Some(command)
          } else {
            sender.send(command)?;
            // unyield everytime
            handle.unyield()?;
            None
          }
        },
        _ => return Err(Error("Call of write listener on wrong state".to_string(), ErrorKind::Bug, None)),
      };
      if restart.is_some() {
        let state = replace(&mut entry.state, SlabEntryState::Empty);
        if let SlabEntryState::WriteSpawned((handle,sender)) = state {
          let (serv,sen,recv) = handle.unwrap_state()?;
          let write_handle = self.write_spawn.spawn(serv, sen, restart, recv, MDC::send_nb_iter)?;
          replace(&mut entry.state, SlabEntryState::WriteSpawned((write_handle,sender)));
        } else {
          unreachable!()
        }
      }
    } else {
      return Err(Error("Call of write listener on no state".to_string(), ErrorKind::Bug, None))
    }

    Ok(())

  }
}




pub struct MyDHTService<MDC : MyDHTConf>(pub MDC, pub MioRecv<<MDC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MDC>>>::Recv>, pub <MDC::MainLoopChannelOut as SpawnChannel<MainLoopReply>>::Send);

impl<MDC : MyDHTConf> Service for MyDHTService<MDC> {
  type CommandIn = MainLoopCommand<MDC>;
  type CommandOut = MainLoopReply;

  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, async_yield : &mut S) -> Result<Self::CommandOut> {
    let mut state = self.0.init_state()?;
    let mut yield_spawn = NoYield(YieldReturn::Loop);
    let r = state.main_loop(&mut self.1, req, &mut yield_spawn);
    if r.is_err() {
      panic!("mainloop err : {:?}",&r);
    }
    Ok(MainLoopReply::Ended) 
  }
}


pub struct PeerCacheEntry<P> {
  peer : P,
  ///  if not initialized CACHE_NO_STREAM, if needed in sub process could switch to arc atomicusize
  read : Rc<Cell<usize>>,
  ///  if not initialized CACHE_NO_STREAM, if 
  write : Rc<Cell<usize>>,
}

impl<P> PeerCacheEntry<P> {
  pub fn get_read_token(&self) -> Option<usize> {
    let v = self.read.get();
    if v < START_STREAM_IX {
      None
    } else {
      Some(v)
    }
  }
  pub fn get_write_token(&self) -> Option<usize> {
    let v = self.write.get();
    if v < START_STREAM_IX {
      None
    } else {
      Some(v)
    }
  }
  pub fn set_read_token(&self, v : usize) {
    self.read.set(v)
  }
  pub fn set_write_token(&self, v : usize) {
    self.write.set(v)
  }
}

// TODO put in its own module
pub struct ReadService<MC : MyDHTConf>(pub <MC::Transport as Transport>::ReadStream);

impl<MDC : MyDHTConf> Service for ReadService<MDC> {
  type CommandIn = ReadServiceCommand;
  type CommandOut = ();

  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, async_yield : &mut S) -> Result<Self::CommandOut> {
    match req {
      ReadServiceCommand::Run => {
        // for initial testing only TODO replace by deser
        let mut buf = vec![0;4];
        let mut r = ReadYield(&mut self.0, async_yield);
        r.read_exact(&mut buf).unwrap(); // unwrap for testring only
//        panic!("{:?}",&buf[..]);
        println!("{:?}",&buf[..]);
        assert!(&[1,2,3,4] == &buf[..]);
        buf[0]=9;
      },
    }
    Ok(())
  }
}
/// command for readservice
#[derive(Clone)]
pub enum ReadServiceCommand {
  Run,
}

// TODO put in its own module
pub struct WriteService<MC : MyDHTConf>(pub <MC::Transport as Transport>::WriteStream);

impl<MDC : MyDHTConf> Service for WriteService<MDC> {
  type CommandIn = WriteServiceCommand;
  type CommandOut = ();

  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, async_yield : &mut S) -> Result<Self::CommandOut> {
    match req {
      WriteServiceCommand::Write => {
        // for initial testing only TODO replace by deser
        let buf = &[1,2,3,4];
        let mut w = WriteYield(&mut self.0, async_yield);
        w.write_all(buf).unwrap(); // unwrap for testing only (thread without error catching
      },
    }
    Ok(())
  }
}
/// command for readservice
#[derive(Clone)]
pub enum WriteServiceCommand {
  Write,
}


