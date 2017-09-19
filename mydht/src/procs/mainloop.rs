//! Main loop for mydht. This is the main dht loop, see default implementation of MyDHT main_loop
//! method.
//! Usage of mydht library requires to create a struct implementing the MyDHTConf trait, by linking with suitable inner trait implementation and their requires component.
use std::clone::Clone;
use std::borrow::Borrow;
use super::{
  MyDHTConf,
  MainLoopSendIn,
  RWSlabEntry,
  ChallengeEntry,
};
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
  HandleSend,
  Service,
  Spawner,
  SpawnSend,
  SpawnRecv,
  SpawnHandle,
  SpawnUnyield,
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
  PeerPriority,
  PeerMgmtMeths,
};
use kvcache::{
  SlabCache,
  KVCache,
  Cache,
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
  ToRef,
  SRef,
  SToRef,
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
use super::server2::{
  ReadCommand,
  ReadService,
  ReadDest,
};
use super::client2::{
  WriteCommand,
  WriteCommandSend,
  WriteReply,
  WriteService,
};
use super::api::{
  ApiReply,
};
use super::peermgmt::{
  PeerMgmtCommand,
};

macro_rules! register_state_r {($self:ident,$rs:ident,$os:expr,$entrystate:path,$with:expr) => {
  {
    let token = $self.slab_cache.insert(SlabEntry {
      state : $entrystate($rs,$with),
      os : $os,
      peer : None,
    });
    if let $entrystate(ref mut rs,_) = $self.slab_cache.get_mut(token).unwrap().state {
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

pub type WriteHandleSend<MDC : MyDHTConf> = HandleSend<<MDC::WriteChannelIn as SpawnChannel<WriteCommand<MDC>>>::Send,
  <<
    MDC::WriteSpawn as Spawner<WriteService<MDC>,MDC::WriteDest,<MDC::WriteChannelIn as SpawnChannel<WriteCommand<MDC>>>::Recv>>::Handle as 
    SpawnHandle<WriteService<MDC>,MDC::WriteDest,<MDC::WriteChannelIn as SpawnChannel<WriteCommand<MDC>>>::Recv>
    >::WeakHandle
    >;

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
  /// reject stream for this token and Address
  RejectReadSpawn(usize),
  /// reject a peer (accept fail), usize are write stream token and read stream token
  RejectPeer(<MC::Peer as KeyVal>::Key,Option<usize>,Option<usize>),
  /// New peer after a ping
  NewPeerChallenge(MC::PeerRef,PeerPriority,usize,Vec<u8>),
  /// New peer after first or second pong
  NewPeerUncheckedChallenge(MC::PeerRef,PeerPriority,usize,Vec<u8>,Option<Vec<u8>>),
  /// new peer accepted with optionnal read stream token TODO remove (replace by NewPeerChallenge)
  NewPeer(MC::PeerRef,PeerPriority,Option<usize>),
  ProxyWrite(WriteCommand<MC>),
}
/// Send variant (to use when inner ref are not Sendable)
pub enum MainLoopCommandSend<MC : MyDHTConf> {
  Start,
  TryConnect(<MC::Transport as Transport>::Address),
  /// reject stream for this token and Address
  RejectReadSpawn(usize),
  /// reject a peer (accept fail), usize are write stream token and read stream token
  RejectPeer(<MC::Peer as KeyVal>::Key,Option<usize>,Option<usize>),
  /// new peer accepted with optionnal read stream token
  NewPeer(<MC::PeerRef as Ref<MC::Peer>>::Send,PeerPriority,Option<usize>),
  NewPeerChallenge(<MC::PeerRef as Ref<MC::Peer>>::Send,PeerPriority,usize,Vec<u8>),
  NewPeerUncheckedChallenge(<MC::PeerRef as Ref<MC::Peer>>::Send,PeerPriority,usize,Vec<u8>,Option<Vec<u8>>),
  ProxyWrite(WriteCommandSend<MC>),
}

impl<MC : MyDHTConf> Clone for MainLoopCommand<MC> {
  fn clone(&self) -> Self {
    match *self {
      MainLoopCommand::Start => MainLoopCommand::Start,
      MainLoopCommand::TryConnect(ref a) => MainLoopCommand::TryConnect(a.clone()),
      MainLoopCommand::RejectReadSpawn(s) => MainLoopCommand::RejectReadSpawn(s),
      MainLoopCommand::RejectPeer(ref k,ref os,ref os2) => MainLoopCommand::RejectPeer(k.clone(),os.clone(),os2.clone()),
      MainLoopCommand::NewPeer(ref rp,ref pp,ref os) => MainLoopCommand::NewPeer(rp.clone(),pp.clone(),os.clone()),
      MainLoopCommand::NewPeerChallenge(ref rp,ref pp,rtok,ref chal) => MainLoopCommand::NewPeerChallenge(rp.clone(),pp.clone(),rtok,chal.clone()),
      MainLoopCommand::NewPeerUncheckedChallenge(ref rp,ref pp,rtok,ref chal,ref nextchal) => MainLoopCommand::NewPeerUncheckedChallenge(rp.clone(),pp.clone(),rtok,chal.clone(),nextchal.clone()),
      MainLoopCommand::ProxyWrite(ref rcs) => MainLoopCommand::ProxyWrite(rcs.clone()),
    }
  }
}

impl<MC : MyDHTConf> SRef for MainLoopCommand<MC> {
  type Send = MainLoopCommandSend<MC>;
  fn get_sendable(&self) -> Self::Send {
    match *self {
      MainLoopCommand::Start => MainLoopCommandSend::Start,
      MainLoopCommand::TryConnect(ref a) => MainLoopCommandSend::TryConnect(a.clone()),
      MainLoopCommand::RejectReadSpawn(s) => MainLoopCommandSend::RejectReadSpawn(s),
      MainLoopCommand::RejectPeer(ref k,ref os,ref os2) => MainLoopCommandSend::RejectPeer(k.clone(),os.clone(),os2.clone()),
      MainLoopCommand::NewPeer(ref rp,ref pp,ref os) => MainLoopCommandSend::NewPeer(rp.get_sendable(),pp.clone(),os.clone()),
      MainLoopCommand::NewPeerChallenge(ref rp,ref pp,rtok,ref chal) => MainLoopCommandSend::NewPeerChallenge(rp.get_sendable(),pp.clone(),rtok,chal.clone()),
      MainLoopCommand::NewPeerUncheckedChallenge(ref rp,ref pp,rtok,ref chal,ref nextchal) => MainLoopCommandSend::NewPeerUncheckedChallenge(rp.get_sendable(),pp.clone(),rtok,chal.clone(),nextchal.clone()),
      MainLoopCommand::ProxyWrite(ref rcs) => MainLoopCommandSend::ProxyWrite(rcs.get_sendable()),
    }
  }
}

impl<MC : MyDHTConf> SToRef<MainLoopCommand<MC>> for MainLoopCommandSend<MC> {
  fn to_ref(self) -> MainLoopCommand<MC> {
    match self {
      MainLoopCommandSend::Start => MainLoopCommand::Start,
      MainLoopCommandSend::TryConnect(a) => MainLoopCommand::TryConnect(a),
      MainLoopCommandSend::RejectReadSpawn(s) => MainLoopCommand::RejectReadSpawn(s),
      MainLoopCommandSend::RejectPeer(k,os,os2) => MainLoopCommand::RejectPeer(k,os,os2),
      MainLoopCommandSend::NewPeer(rp,pp,os) => MainLoopCommand::NewPeer(rp.to_ref(),pp,os),
      MainLoopCommandSend::NewPeerChallenge(rp,pp,rtok,chal) => MainLoopCommand::NewPeerChallenge(rp.to_ref(),pp,rtok,chal),
      MainLoopCommandSend::NewPeerUncheckedChallenge(rp,pp,rtok,chal,nchal) => MainLoopCommand::NewPeerUncheckedChallenge(rp.to_ref(),pp,rtok,chal,nchal),
      MainLoopCommandSend::ProxyWrite(rcs) => MainLoopCommand::ProxyWrite(rcs.to_ref()),
    }
  }
}

pub struct MDHTState<MDC : MyDHTConf> {
  me : MDC::PeerRef,
  transport : MDC::Transport,
  slab_cache : MDC::Slab,
  peer_cache : MDC::PeerCache,
  challenge_cache : MDC::ChallengeCache,
  events : Option<Events>,
  poll : Poll,
  read_spawn : MDC::ReadSpawn,
  read_channel_in : DefaultRecvChannel<ReadCommand, MDC::ReadChannelIn>,
  write_spawn : MDC::WriteSpawn,
  write_channel_in : MDC::WriteChannelIn,
  peermgmt_channel_in : MDC::PeerMgmtChannelIn,
  enc_proto : MDC::MsgEnc,
  /// currently locally use only for challenge TODO rename (proto not ok) or split trait to have a
  /// challenge only instance
  peermgmt_proto : MDC::PeerMgmtMeths,
  dhtrules_proto : MDC::DHTRules,
  pub mainloop_send : MioSend<<MDC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MDC>>>::Send>,

}

impl<MDC : MyDHTConf> MDHTState<MDC> {
  pub fn init_state(conf : &mut MDC,mlsend : &mut MioSend<<MDC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MDC>>>::Send>) -> Result<MDHTState<MDC>> {
    let events = Events::with_capacity(MDC::events_size);
    let poll = Poll::new()?;
    let transport = conf.init_transport()?;
    assert!(true == transport.register(&poll, LISTENER, Ready::readable(),
                    PollOpt::edge())?);
    let me = conf.init_ref_peer()?;
    let read_spawn = conf.init_read_spawner()?;
    let write_spawn = conf.init_write_spawner()?;
    let read_channel_in = DefaultRecvChannel(conf.init_read_channel_in()?,ReadCommand::Run);
    let write_channel_in = conf.init_write_channel_in()?;
    let enc_proto = conf.init_enc_proto()?;
    let s = MDHTState {
      me : me,
      transport : transport,
      slab_cache : conf.init_main_loop_slab_cache()?,
      peer_cache : conf.init_main_loop_peer_cache()?,
      challenge_cache : conf.init_main_loop_challenge_cache()?,
      events : None,
      poll : poll,
      read_spawn : read_spawn,
      read_channel_in : read_channel_in,
      write_spawn : write_spawn,
      write_channel_in : write_channel_in,
      peermgmt_channel_in : conf.init_peermgmt_channel_in()?,
      enc_proto : enc_proto,
      peermgmt_proto : conf.init_peermgmt_proto()?,
      dhtrules_proto : conf.init_dhtrules_proto()?,
      mainloop_send : mlsend.clone(),
    };
    Ok(s)
  }


  fn connect_with(&mut self, dest_address : &<MDC::Peer as Peer>::Address) -> Result<(usize, Option<usize>)> {
    // TODO duration will be removed
    // first check cache if there is a connect already : no (if peer yes)
    let (ws,ors) = self.transport.connectwith(&dest_address, CrateDuration::seconds(1000))?;

      let (s,r) = self.write_channel_in.new()?;

      // register writestream
      let write_token = register_state_w!(self,PollOpt::edge(),ws,(s,r,false),None,SlabEntryState::WriteStream);

      // register readstream for multiplex transport
      let ort = ors.map(|rs| -> Result<Option<usize>> {
        let read_token = register_state_r!(self,rs,Some(write_token),SlabEntryState::ReadStream,None);
        // update read reference
        self.slab_cache.get_mut(write_token).map(|r|r.os = Some(read_token));
        Ok(Some(read_token))
      }).unwrap_or(Ok(None))?;

    Ok((write_token,ort))
  }

  fn update_peer(&mut self, pr : MDC::PeerRef, pp : PeerPriority, owtok : Option<usize>, owread : Option<usize>) -> Result<()> {
    println!("upd pee\n");
    let pk = pr.borrow().get_key();
    // TODO conflict for existing peer (close )
    // TODO useless has_val_c : u
    if let Some(p_entry) = self.peer_cache.get_val_c(&pk) {
      // TODO update peer, if xisting write should be drop (new one should be use) and read could
      // receive a message for close on timeout (read should stay open as at the other side on
      // double connect the peer can keep sending on this read : the new read could timeout in
      // fact
      // TODO add a logg for this removeal and one for double read_token
      // TODO clean close??
      // TODO check if read is finished and remove if finished
      if p_entry.write.get() != CACHE_NO_STREAM {
        self.slab_cache.remove(p_entry.write.get());
      }
    };
    self.peer_cache.add_val_c(pk, PeerCacheEntry {
      peer : pr,
      read : Rc::new(Cell::new(owread.unwrap_or(CACHE_NO_STREAM))),
      write : Rc::new(Cell::new(owtok.unwrap_or(CACHE_NO_STREAM))),
      prio : Rc::new(Cell::new(pp)),
    });
    // TODO peer_service update ??? through peer cache ???
    Ok(())
  }

 
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
        let (write_token,ort) = self.connect_with(&dest_address)?;

        let chal = self.peermgmt_proto.challenge(self.me.borrow());
        self.challenge_cache.add_val_c(chal.clone(),ChallengeEntry {
          write_tok : write_token,
          read_tok : ort,
        });
        // send a ping
        self.write_stream_send(write_token,WriteCommand::Ping(chal), <MDC>::init_write_spawner_out()?, None)?;

      },
      MainLoopCommand::NewPeerChallenge(pr,pp,rtok,chal) => {
        let owtok = self.slab_cache.get(rtok).map(|r|r.os);
        let wtok = match owtok {
          Some(Some(tok)) => tok,
          Some(None) => {
            let (write_token,ort) = self.connect_with(pr.borrow().get_address())?;
            assert!(ort.is_none()); // TODO change to a warning log (might mean half multiplex transport)
            write_token
          },

          None => {
            // TODO log inconsistency certainly related to peer removal
            return Ok(())
          },
        };

        let chal2 = self.peermgmt_proto.challenge(self.me.borrow());
        self.challenge_cache.add_val_c(chal2.clone(),ChallengeEntry {
          write_tok : wtok,
          read_tok : Some(rtok),
        });
  
        // do not store new peer as it is not authentified with a fresh challenge (could be replay)
//         self.update_peer(pr.clone(),pp,Some(wtok),Some(rtok))?;
        // send pong with 2nd challeng
        let pongmess = WriteCommand::Pong(pr,chal,rtok,Some(chal2));
        // with is not used because the command will init it
        self.write_stream_send(wtok, pongmess, <MDC>::init_write_spawner_out()?, None)?; // TODO remove peer on error
      },
 
      MainLoopCommand::NewPeerUncheckedChallenge(pr,pp,rtok,chal,nextchal) => {
        // check chal
        match self.challenge_cache.get_val_c(&chal).map(|c|c.write_tok) {
          Some(wtok) => {
            self.update_peer(pr.clone(),pp,Some(wtok),Some(rtok))?;
            if let Some(nchal) = nextchal {
              let pongmess = WriteCommand::Pong(pr,nchal,rtok,None);
              self.write_stream_send(wtok, pongmess, <MDC>::init_write_spawner_out()?, None)?; // TODO remove peer on error
            }
          },
          None => {
            // TODO log inconsistence, do not panic as it can be forge or replay
            panic!("TODO same code as RejectReadSpawn plus RejectPeer with pr and rtok : transport must be close at least, peer??");
          },
        };

      },
      MainLoopCommand::RejectReadSpawn(..) => panic!("TODO"),
      MainLoopCommand::RejectPeer(..) => panic!("TODO"),
      MainLoopCommand::ProxyWrite(..) => panic!("TODO"),
      MainLoopCommand::NewPeer(..) => panic!("TODO"), // TODO send to peermgmt

    }

    Ok(())
  }

pub fn main_loop<S : SpawnerYield>(&mut self,rec : &mut MioRecv<<MDC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MDC>>>::Recv>, req: MainLoopCommand<MDC>, async_yield : &mut S) -> Result<()> {
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

    //(self.mainloop_send).send(super::server2::ReadReply::MainLoop(MainLoopCommand::Start))?;
    //(self.mainloop_send).send((MainLoopCommand::Start))?;
    // TODO start other service
    let (smgmt,rmgmt) = self.peermgmt_channel_in.new()?; // TODO persistence bad for suspend -> put smgmt into self plus add handle to send 

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
              let read_token = register_state_r!(self,rs,None,SlabEntryState::ReadStream,None);

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
            let (sp_read, o_sp_write,os) =  if let Some(ca) = self.slab_cache.get_mut(tok.0 - START_STREAM_IX) {
              let os = ca.os;
              match ca.state {
                SlabEntryState::ReadStream(_,_) => {

                  (true,None,os)
                },
                SlabEntryState::WriteStream(ref mut ws,(_,ref mut write_r_in,ref mut has_connect)) => {
                  // case where spawn reach its nb_loop and return, should not happen as yield is
                  // only out of service call (nb_loop test is out of nb call) for receiver which is not registered.
                  // Yet if WriteService could resume which is actually not the case we could have a
                  // spawneryield return and would need to resume with a resume command.
                  // self.write_stream_send(write_token,WriteCommand::Resume , <MDC>::init_write_spawner_out()?)?;
                  // also case when multiplex transport and write stream is ready but nothing to
                  // send : spawn of write happens on first write.
                  //TODO a debug log??
                  if *has_connect == false {
                    *has_connect = true;
                    // reregister at a edge level
                    //assert!(true == ws.reregister(&self.poll, tok, Ready::writable(),PollOpt::edge())?);
                  }
                  let oc = write_r_in.recv()?;

                  (false,oc,os)
                },
                SlabEntryState::ReadSpawned((ref mut handle,_)) => {
                  handle.unyield()?;
                  (false,None,os)
                },
                SlabEntryState::WriteSpawned((ref mut handle,_)) => {
                  handle.unyield()?;
                  (false,None,os)
                },
                SlabEntryState::Empty => {
                  unreachable!()
                },
              }
            } else {
              // TODO replace by logging as it should happen depending on transport implementation
              // (or spurrious poll), keep it now for testing purpose
              panic!("Unregistered token polled");
              (false,None,None)
            };
            if o_sp_write.is_some() {
              self.write_stream_send(tok.0 - START_STREAM_IX, o_sp_write.unwrap(), <MDC>::init_write_spawner_out()?, None)?;
            }
            if sp_read {
              let owrite_send = match os {
                Some(s) => {
                  self.get_write_handle_send(s)
                },
                None => None,
              };

    //(self.mainloop_send).send(super::server2::ReadReply::MainLoop(MainLoopCommand::Start))?;
    //(self.mainloop_send).send((MainLoopCommand::Start))?;
              let mut rd = ReadDest {
                mainloop : self.mainloop_send.clone(),
                peermgmt : smgmt.clone(),
                write : owrite_send,
              };
    //(self.mainloop_send).send((MainLoopCommand::Start))?;
    //(self.mainloop_send).send(super::server2::ReadReply::MainLoop(MainLoopCommand::Start))?;
              self.start_read_stream_listener(tok.0 - START_STREAM_IX, rd)?;
            }


          },
        }
      }
    }
 
    Ok(())
  }
  fn get_write_handle_send(&self, wtoken : usize) -> Option<WriteHandleSend<MDC>> {
       let ow = self.slab_cache.get(wtoken);
        if let Some(ca) = ow {
          if let SlabEntryState::WriteSpawned((ref write_handle,ref write_s_in)) = ca.state {
            if let Some (wh) = write_handle.get_weak_handle() {
              Some(HandleSend(write_s_in.clone(),wh))
            } else {None}
          } else {None}
        } else {None}
    }

  /// The state of the slab entry must be checked before, return error on wrong state
  fn start_read_stream_listener(&mut self, read_token : usize,
    mut read_out : ReadDest<MDC>,
                                ) -> Result<()> {
    //read_out.send(super::server2::ReadReply::MainLoop(MainLoopCommand::Start))?;
    let se = self.slab_cache.get_mut(read_token);
    if let Some(entry) = se {
      if let SlabEntryState::ReadStream(_,_) = entry.state {
        let state = replace(&mut entry.state,SlabEntryState::Empty);
        let (rs,with) = match state {
          SlabEntryState::ReadStream(rs,with) => (rs,with),
          _ => unreachable!(),
        };
        let (read_s_in,read_r_in) = self.read_channel_in.new()?;
        // spawn reader TODO try with None (default to Run)
        let read_handle = self.read_spawn.spawn(ReadService::new(
            read_token,
            rs,
            self.me.get_sendable(),
            with.map(|w|w.get_sendable()),
            self.enc_proto.clone(),
            self.peermgmt_proto.clone()
            ), read_out, Some(ReadCommand::Run), read_r_in, 0)?;
        let state = SlabEntryState::ReadSpawned((read_handle,read_s_in));
        replace(&mut entry.state,state);
        return Ok(())
      } else if let SlabEntryState::ReadSpawned(_) = entry.state {
        return Ok(())
      }
    }

    Err(Error("Call of read listener on wrong state".to_string(), ErrorKind::Bug, None))
  }


  fn remove_writestream(&mut self, write_token : usize) -> Result<()> {
    // TODO remove ws and rs (test if rs spawn is finished before : could still be running) if in ws plus update peer cache to remove peer
    panic!("TODO");
    Ok(())
  }

  /// The state of the slab entry must be checked before, return error on wrong state
  ///  TODO replace dest with channel spawner mut ref
  fn write_stream_send(&mut self, write_token : usize, command : WriteCommand<MDC>, write_out : MDC::WriteDest, with : Option<MDC::PeerRef>) -> Result<()> {
    let rem = {
    let se = self.slab_cache.get_mut(write_token);
    if let Some(entry) = se {
      let finished = match entry.state {
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
            // dest is unknown TODO check if still relevant with other use case (read side we allow
            // dest peer as rs could result from a connection with an identified peer)
            let write_service = WriteService::new(write_token,ws,self.me.get_sendable(),with.map(|w|w.get_sendable()), self.enc_proto.clone(),self.peermgmt_proto.clone());
            let write_handle = self.write_spawn.spawn(write_service, write_out.clone(), ocin, write_r_in, MDC::send_nb_iter)?;
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
      if finished.is_some() {
        let state = replace(&mut entry.state, SlabEntryState::Empty);
        if let SlabEntryState::WriteSpawned((handle,sender)) = state {
          let (serv,mut sen,recv,result) = handle.unwrap_state()?;
          if result.is_ok() {
            // restart
            let write_handle = self.write_spawn.spawn(serv, sen, finished, recv, MDC::send_nb_iter)?;
            replace(&mut entry.state, SlabEntryState::WriteSpawned((write_handle,sender)));
            false
          } else {
            // error TODO error management
            // TODO log
            // TODO sen writeout error
            sen.send(WriteReply::Api(ApiReply::Failure(finished.unwrap())))?;
            true
          }
        } else {
          unreachable!()
        }
      } else {
        false
      }
    } else {
      return Err(Error("Call of write listener on no state".to_string(), ErrorKind::Bug, None))
    }
    };
    if rem {
      // TODO remove write stream !!!
      self.remove_writestream(write_token)?;
    }

    Ok(())

  }
}



/// Rc Cell seems useless TODO remove if unused or use it by using them in slabcache too
pub struct PeerCacheEntry<RP> {
  /// ref peer
  peer : RP,
  ///  if not initialized CACHE_NO_STREAM, if needed in sub process could switch to arc atomicusize
  read : Rc<Cell<usize>>,
  ///  if not initialized CACHE_NO_STREAM, if 
  write : Rc<Cell<usize>>,
  /// peer priority
  prio : Rc<Cell<PeerPriority>>,
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


