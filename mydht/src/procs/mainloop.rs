//! Main loop for mydht. This is the main dht loop, see default implementation of MyDHT main_loop
//! method.
//! Usage of mydht library requires to create a struct implementing the MyDHTConf trait, by linking with suitable inner trait implementation and their requires component.
use std::clone::Clone;
use log;
use std::borrow::Borrow;
use query::{
  QueryPriority,
};
use procs::storeprop::{
  KVStoreCommand,
  KVStoreCommandSend,
  KVStoreReply,
  KVStoreService,
  OptPeerGlobalDest,
};
use super::deflocal::{
  GlobalCommand,
  GlobalCommandSend,
  GlobalReply,
  GlobalDest,
};

use super::api::{
  ApiQueryId,
  ApiCommand,
  ApiReply,
  ApiDest,
  ApiQueriable,
  ApiRepliable,
  ApiReturn,
};
use super::{
  MyDHTConf,
  MCCommand,
  MCReply,
  MainLoopReply,
  MainLoopSendIn,
  RWSlabEntry,
  ChallengeEntry,
  GlobalHandle,
  GlobalHandleSend,
  ApiHandle,
  PeerStoreHandle,
  ApiHandleSend,
  Route,
  send_with_handle,
  ShadowAuthType,
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


pub type WriteHandleSend<MC : MyDHTConf> = HandleSend<<MC::WriteChannelIn as SpawnChannel<WriteCommand<MC>>>::Send,
  <<
    MC::WriteSpawn as Spawner<WriteService<MC>,MC::WriteDest,<MC::WriteChannelIn as SpawnChannel<WriteCommand<MC>>>::Recv>>::Handle as 
    SpawnHandle<WriteService<MC>,MC::WriteDest,<MC::WriteChannelIn as SpawnChannel<WriteCommand<MC>>>::Recv>
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
//  ForwardServiceLocal(MC::LocalServiceCommand,usize),
  ForwardService(Option<Vec<MC::PeerRef>>,Option<Vec<(<MC::Peer as KeyVal>::Key,<MC::Peer as Peer>::Address)>>,usize,MCCommand<MC>),
  /// usize is only for mccommand type local to set the number of local to target
  ForwardApi(MCCommand<MC>,usize,MC::ApiReturn),
  /// received peer store command from peer : almost named ProxyPeerStore but it does a local peer
  /// cache check first so it is not named Proxy
  PeerStore(GlobalCommand<MC::PeerRef,KVStoreCommand<MC::Peer,MC::Peer,MC::PeerRef>>),
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
  /// first field is read token ix, write is obtain from os or a connection
  ProxyWrite(usize,WriteCommand<MC>),
  ProxyGlobal(GlobalCommand<MC::PeerRef,MC::GlobalServiceCommand>),
//  GlobalApi(GlobalCommand<MC::PeerRef,MC::GlobalServiceCommand>,MC::ApiReturn),
  ProxyApiReply(MCReply<MC>),
}

/// Send variant (to use when inner ref are not Sendable)
pub enum MainLoopCommandSend<MC : MyDHTConf>
 where MC::GlobalServiceCommand : SRef,
       MC::LocalServiceCommand : SRef,
       MC::GlobalServiceReply : SRef,
       MC::LocalServiceReply : SRef {
  Start,
  TryConnect(<MC::Transport as Transport>::Address),
//  ForwardServiceLocal(<MC::LocalServiceCommand as SRef>::Send,usize),
  ForwardService(Option<Vec<<MC::PeerRef as SRef>::Send>>,Option<Vec<(<MC::Peer as KeyVal>::Key,<MC::Peer as Peer>::Address)>>,usize,<MCCommand<MC> as SRef>::Send),
  ForwardApi(<MCCommand<MC> as SRef>::Send,usize,MC::ApiReturn),
  PeerStore(GlobalCommandSend<<MC::PeerRef as SRef>::Send,KVStoreCommandSend<MC::Peer,MC::Peer,MC::PeerRef>>),
  /// reject stream for this token and Address
  RejectReadSpawn(usize),
  /// reject a peer (accept fail), usize are write stream token and read stream token
  RejectPeer(<MC::Peer as KeyVal>::Key,Option<usize>,Option<usize>),
  /// new peer accepted with optionnal read stream token
  NewPeer(<MC::PeerRef as SRef>::Send,PeerPriority,Option<usize>),
  NewPeerChallenge(<MC::PeerRef as SRef>::Send,PeerPriority,usize,Vec<u8>),
  NewPeerUncheckedChallenge(<MC::PeerRef as SRef>::Send,PeerPriority,usize,Vec<u8>,Option<Vec<u8>>),
  ProxyWrite(usize,WriteCommandSend<MC>),
  ProxyGlobal(GlobalCommandSend<<MC::PeerRef as SRef>::Send,<MC::GlobalServiceCommand as SRef>::Send>),
//  GlobalApi(GlobalCommandSend<<MC::PeerRef as SRef>::Send,<MC::GlobalServiceCommand as SRef>::Send>,MC::ApiReturn),
  ProxyApiReply(<MCReply<MC> as SRef>::Send),
//  ProxyApiGlobalReply(<MC::GlobalServiceReply as SRef>::Send),
}

impl<MC : MyDHTConf> Clone for MainLoopCommand<MC> 
 where MC::GlobalServiceCommand : Clone,
       MC::LocalServiceCommand : Clone,
       MC::GlobalServiceReply : Clone,
       MC::LocalServiceReply : Clone {
  fn clone(&self) -> Self {
    match *self {
      MainLoopCommand::Start => MainLoopCommand::Start,
      MainLoopCommand::TryConnect(ref a) => MainLoopCommand::TryConnect(a.clone()),
//      MainLoopCommand::ForwardServiceLocal(ref gc,nb) => MainLoopCommand::ForwardServiceLocal(gc.clone(),nb),
      MainLoopCommand::ForwardService(ref ovp,ref okad,nb_for,ref c) => MainLoopCommand::ForwardService(ovp.clone(),okad.clone(),nb_for,c.clone()),
      MainLoopCommand::ForwardApi(ref gc,nb_for,ref ret) => MainLoopCommand::ForwardApi(gc.clone(),nb_for,ret.clone()),
      MainLoopCommand::PeerStore(ref cmd) => MainLoopCommand::PeerStore(cmd.clone()),
      MainLoopCommand::RejectReadSpawn(s) => MainLoopCommand::RejectReadSpawn(s),
      MainLoopCommand::RejectPeer(ref k,ref os,ref os2) => MainLoopCommand::RejectPeer(k.clone(),os.clone(),os2.clone()),
      MainLoopCommand::NewPeer(ref rp,ref pp,ref os) => MainLoopCommand::NewPeer(rp.clone(),pp.clone(),os.clone()),
      MainLoopCommand::NewPeerChallenge(ref rp,ref pp,rtok,ref chal) => MainLoopCommand::NewPeerChallenge(rp.clone(),pp.clone(),rtok,chal.clone()),
      MainLoopCommand::NewPeerUncheckedChallenge(ref rp,ref pp,rtok,ref chal,ref nextchal) => MainLoopCommand::NewPeerUncheckedChallenge(rp.clone(),pp.clone(),rtok,chal.clone(),nextchal.clone()),
      MainLoopCommand::ProxyWrite(rt, ref rcs) => MainLoopCommand::ProxyWrite(rt, rcs.clone()),
      MainLoopCommand::ProxyGlobal(ref rcs) => MainLoopCommand::ProxyGlobal(rcs.clone()),
//      MainLoopCommand::GlobalApi(ref rcs,ref ret) => MainLoopCommand::GlobalApi(rcs.clone(),ret.clone()),
      MainLoopCommand::ProxyApiReply(ref rcs) => MainLoopCommand::ProxyApiReply(rcs.clone()),
//      MainLoopCommand::ProxyApiGlobalReply(ref rcs) => MainLoopCommand::ProxyApiGlobalReply(rcs.clone()),
    }
  }
}

impl<MC : MyDHTConf> SRef for MainLoopCommand<MC>
 where MC::GlobalServiceCommand : SRef,
       MC::LocalServiceCommand : SRef,
       MC::GlobalServiceReply : SRef,
       MC::LocalServiceReply : SRef {
  type Send = MainLoopCommandSend<MC>;
  fn get_sendable(&self) -> Self::Send {
    match *self {
      MainLoopCommand::Start => MainLoopCommandSend::Start,
      MainLoopCommand::TryConnect(ref a) => MainLoopCommandSend::TryConnect(a.clone()),
//      MainLoopCommand::ForwardServiceLocal(ref gc,nb) => MainLoopCommandSend::ForwardServiceLocal(gc.get_sendable(),nb),
      MainLoopCommand::ForwardService(ref ovp,ref okad,nb_for,ref c) => MainLoopCommandSend::ForwardService({
          ovp.as_ref().map(|vp|vp.iter().map(|p|p.get_sendable()).collect())
        },okad.clone(),nb_for,c.get_sendable()),
      MainLoopCommand::ForwardApi(ref gc,nb_for,ref ret) => MainLoopCommandSend::ForwardApi(gc.get_sendable(),nb_for,ret.clone()),
      MainLoopCommand::PeerStore(ref cmd) => MainLoopCommandSend::PeerStore(cmd.get_sendable()),
      MainLoopCommand::RejectReadSpawn(s) => MainLoopCommandSend::RejectReadSpawn(s),
      MainLoopCommand::RejectPeer(ref k,ref os,ref os2) => MainLoopCommandSend::RejectPeer(k.clone(),os.clone(),os2.clone()),
      MainLoopCommand::NewPeer(ref rp,ref pp,ref os) => MainLoopCommandSend::NewPeer(rp.get_sendable(),pp.clone(),os.clone()),
      MainLoopCommand::NewPeerChallenge(ref rp,ref pp,rtok,ref chal) => MainLoopCommandSend::NewPeerChallenge(rp.get_sendable(),pp.clone(),rtok,chal.clone()),
      MainLoopCommand::NewPeerUncheckedChallenge(ref rp,ref pp,rtok,ref chal,ref nextchal) => MainLoopCommandSend::NewPeerUncheckedChallenge(rp.get_sendable(),pp.clone(),rtok,chal.clone(),nextchal.clone()),
      MainLoopCommand::ProxyWrite(rt, ref rcs) => MainLoopCommandSend::ProxyWrite(rt, rcs.get_sendable()),
      MainLoopCommand::ProxyGlobal(ref rcs) => MainLoopCommandSend::ProxyGlobal(rcs.get_sendable()),
//      MainLoopCommand::GlobalApi(ref rcs,ref ret) => MainLoopCommandSend::GlobalApi(rcs.get_sendable(),ret.clone()),
      MainLoopCommand::ProxyApiReply(ref rcs) => MainLoopCommandSend::ProxyApiReply(rcs.get_sendable()),
//      MainLoopCommand::ProxyApiLocalReply(ref rcs) => MainLoopCommandSend::ProxyApiLocalReply(rcs.get_sendable()),
    }
  }
}

impl<MC : MyDHTConf> SToRef<MainLoopCommand<MC>> for MainLoopCommandSend<MC>
 where MC::GlobalServiceCommand : SRef,
       MC::LocalServiceCommand : SRef,
       MC::GlobalServiceReply : SRef,
       MC::LocalServiceReply : SRef {
  fn to_ref(self) -> MainLoopCommand<MC> {
    match self {
      MainLoopCommandSend::Start => MainLoopCommand::Start,
      MainLoopCommandSend::TryConnect(a) => MainLoopCommand::TryConnect(a),
//      MainLoopCommandSend::ForwardServiceLocal(a,nb) => MainLoopCommand::ForwardServiceLocal(a.to_ref(),nb),
      MainLoopCommandSend::ForwardService(ovp,okad,nb_for,c) => MainLoopCommand::ForwardService({
          ovp.map(|vp|vp.into_iter().map(|p|p.to_ref()).collect())
        },okad,nb_for,c.to_ref()),
      MainLoopCommandSend::ForwardApi(a,nb_for,r) => MainLoopCommand::ForwardApi(a.to_ref(),nb_for,r),
      MainLoopCommandSend::PeerStore(cmd) => MainLoopCommand::PeerStore(cmd.to_ref()),
      MainLoopCommandSend::RejectReadSpawn(s) => MainLoopCommand::RejectReadSpawn(s),
      MainLoopCommandSend::RejectPeer(k,os,os2) => MainLoopCommand::RejectPeer(k,os,os2),
      MainLoopCommandSend::NewPeer(rp,pp,os) => MainLoopCommand::NewPeer(rp.to_ref(),pp,os),
      MainLoopCommandSend::NewPeerChallenge(rp,pp,rtok,chal) => MainLoopCommand::NewPeerChallenge(rp.to_ref(),pp,rtok,chal),
      MainLoopCommandSend::NewPeerUncheckedChallenge(rp,pp,rtok,chal,nchal) => MainLoopCommand::NewPeerUncheckedChallenge(rp.to_ref(),pp,rtok,chal,nchal),
      MainLoopCommandSend::ProxyWrite(rt,rcs) => MainLoopCommand::ProxyWrite(rt,rcs.to_ref()),
      MainLoopCommandSend::ProxyGlobal(rcs) => MainLoopCommand::ProxyGlobal(rcs.to_ref()),
//      MainLoopCommandSend::GlobalApi(rcs,r) => MainLoopCommand::GlobalApi(rcs.to_ref(),r),
      MainLoopCommandSend::ProxyApiReply(rcs) => MainLoopCommand::ProxyApiReply(rcs.to_ref()),
//      MainLoopCommandSend::ProxyApiLocalReply(rcs) => MainLoopCommand::ProxyApiLocalReply(rcs.to_ref()),
    }
  }
}

pub struct MDHTState<MC : MyDHTConf> {
  me : MC::PeerRef,
  transport : MC::Transport,
  route : MC::Route,
  slab_cache : MC::Slab,
  peer_cache : MC::PeerCache,
  challenge_cache : MC::ChallengeCache,
  events : Option<Events>,
  poll : Poll,
  read_spawn : MC::ReadSpawn,
  read_channel_in : DefaultRecvChannel<ReadCommand, MC::ReadChannelIn>,
  write_spawn : MC::WriteSpawn,
  write_channel_in : MC::WriteChannelIn,
  peermgmt_channel_in : MC::PeerMgmtChannelIn,
  enc_proto : MC::MsgEnc,
  /// currently locally use only for challenge TODO rename (proto not ok) or split trait to have a
  /// challenge only instance
  peermgmt_proto : MC::PeerMgmtMeths,
  dhtrules_proto : MC::DHTRules,
  pub mainloop_send : MioSend<<MC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MC>>>::Send>,

  /// stored internally for global restart on ended (maybe on error)
  global_spawn : MC::GlobalServiceSpawn,
  global_channel_in : MC::GlobalServiceChannelIn,
  /// send to global service
  global_send : <MC::GlobalServiceChannelIn as SpawnChannel<GlobalCommand<MC::PeerRef,MC::GlobalServiceCommand>>>::Send,
  global_handle : GlobalHandle<MC>,
  local_spawn_proto : MC::LocalServiceSpawn,
  local_channel_in_proto : MC::LocalServiceChannelIn,
  api_send : <MC::ApiServiceChannelIn as SpawnChannel<ApiCommand<MC>>>::Send,
  api_handle : ApiHandle<MC>,
  peerstore_send : <MC::PeerStoreServiceChannelIn as SpawnChannel<GlobalCommand<MC::PeerRef,KVStoreCommand<MC::Peer,MC::Peer,MC::PeerRef>>>>::Send,
  peerstore_handle : PeerStoreHandle<MC>,
}

impl<MC : MyDHTConf> MDHTState<MC> {
  pub fn init_state(conf : &mut MC,mlsend : &mut MioSend<<MC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MC>>>::Send>) -> Result<MDHTState<MC>> {
    let poll = Poll::new()?;
    let dhtrules_proto = conf.init_dhtrules_proto()?;
    let transport = conf.init_transport()?;
    let route = conf.init_route()?;
    assert!(true == transport.register(&poll, LISTENER, Ready::readable(),
                    PollOpt::edge())?);
    let me = conf.init_ref_peer()?;
    let read_spawn = conf.init_read_spawner()?;
    let write_spawn = conf.init_write_spawner()?;
    let read_channel_in = DefaultRecvChannel(conf.init_read_channel_in()?,ReadCommand::Run);
    let write_channel_in = conf.init_write_channel_in()?;
    let enc_proto = conf.init_enc_proto()?;
    let mut global_spawn = conf.init_global_spawner()?;
    let api_dest = ApiDest {
      main_loop : mlsend.clone(),
    };
    let mut api_spawn = conf.init_api_spawner()?;
    let api_service = conf.init_api_service()?;
    let mut api_channel_in = conf.init_api_channel_in()?;
    let (api_send, api_recv) = api_channel_in.new()?;
    let api_handle = api_spawn.spawn(api_service, api_dest, None, api_recv, MC::API_NB_ITER)?;

    let global_service = conf.init_global_service()?;
    // TODO use a true dest with mainloop, api weak as dest
    let api_gdest = api_handle.get_weak_handle().map(|wh|HandleSend(api_send.clone(),wh));
    let global_dest = GlobalDest {
      mainloop : mlsend.clone(),
      api : api_gdest,
    };
    let api_gdest_peer = api_handle.get_weak_handle().map(|wh|HandleSend(api_send.clone(),wh));
    let global_dest_peer = GlobalDest {
      mainloop : mlsend.clone(),
      api : api_gdest_peer,
    };

    let mut global_channel_in = conf.init_global_channel_in()?;
    let (global_send, global_recv) = global_channel_in.new()?;
    let global_handle = global_spawn.spawn(global_service, global_dest, None, global_recv, MC::GLOBAL_NB_ITER)?;
    let local_channel_in = conf.init_local_channel_in()?;
    let local_spawn = conf.init_local_spawner()?;
    let mut peerstore_channel_in = conf.init_peerstore_channel_in()?;
    let (peerstore_send, peerstore_recv) = peerstore_channel_in.new()?;
    let mut peerstore_spawn = conf.init_peerstore_spawner()?;
    let peerstore_service = KVStoreService {
      me : me.clone(),
      init_store : conf.init_peer_kvstore()?,
      store : None,
      dht_rules : dhtrules_proto.clone(),
      query_cache : conf.init_peer_kvstore_query_cache()?,
      _ph : PhantomData,
    };
    let peerstore_dest = OptPeerGlobalDest(global_dest_peer);
    let peerstore_handle = peerstore_spawn.spawn(peerstore_service,peerstore_dest,None,peerstore_recv, MC::PEERSTORE_NB_ITER)?;

    let s = MDHTState {
      me : me,
      transport : transport,
      route : route,
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
      dhtrules_proto : dhtrules_proto,
      mainloop_send : mlsend.clone(),
      global_channel_in : global_channel_in,
      global_spawn : global_spawn,
      global_send : global_send,
      global_handle : global_handle,
      local_channel_in_proto : local_channel_in,
      local_spawn_proto : local_spawn,
      api_handle : api_handle,
      api_send : api_send,
      peerstore_send : peerstore_send,
      peerstore_handle : peerstore_handle,
    };
    Ok(s)
  }


  fn connect_with(&mut self, dest_address : &<MC::Peer as Peer>::Address) -> Result<(usize, Option<usize>)> {
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

  fn update_peer(&mut self, pr : MC::PeerRef, pp : PeerPriority, owtok : Option<usize>, owread : Option<usize>) -> Result<()> {
    debug!("Peer added with slab entries : w{:?}, r{:?}",owtok,owread);
    println!("Peer added with slab entries : w{:?}, r{:?}",owtok,owread);
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
      if let Some(t) = p_entry.get_write_token() {
        self.slab_cache.remove(t);
      }
    };
    self.peer_cache.add_val_c(pk, PeerCacheEntry {
      peer : pr.clone(),
      read : owread,
      write : owtok,
      prio : Rc::new(Cell::new(pp)),
    });
    let update_peer = KVStoreCommand::StoreLocally(pr,0,None);
    // peer_service update
    send_with_handle_panic!(&mut self.peerstore_send,&mut self.peerstore_handle,GlobalCommand(None,update_peer),"Panic sending to peerstore TODO consider restart (init of peerstore is a fn)");
//    self.peerstore_send.send()?;
    Ok(())
  }

 
  /// sub call for service (actually mor relevant service implementation : the call will only
  /// return one CommandOut to spawner : finished or fail).
  /// Async yield being into self.
  #[inline]
  fn call_inner_loop<S : SpawnerYield>(&mut self, req: MainLoopCommand<MC>,mlsend : &mut <MC::MainLoopChannelOut as SpawnChannel<MainLoopReply<MC>>>::Send, async_yield : &mut S) -> Result<()> {
    match req {
      MainLoopCommand::Start => {
        // do nothing it has start if the function was called
      },
      MainLoopCommand::PeerStore(kvcmd) => {
        /*
        match kvcmd {
          GlobalCommand(Some(with),KVStoreCommand::Find(ref qm,ref key,None)) => {
            if qm.nb_res == 1 && oaqid.is_some() {
              let op = self.peer_cache.get_val_c(key).map(|v|v.peer.clone());
              if op.is_some() {
              TODO here we can reuse kvstore code to proxy to peer : depending on qm and with...
              }
            }
          },
          _ => (),
        }
        */
        // local query on cache for kvfind before kvstore proxy
        send_with_handle_panic!(&mut self.peerstore_send,&mut self.peerstore_handle,kvcmd,"TODO peerstore service restart?");
      },
      MainLoopCommand::ProxyApiReply(sc) => {
        if MC::USE_API_SERVICE {
          // TODO log try restart ???
          send_with_handle_panic!(&mut self.api_send,&mut self.api_handle,ApiCommand::ServiceReply(sc),"TODO api service restart?");
        } else {
          mlsend.send(MainLoopReply::ServiceReply(sc))?;
        }
      },
/*      MainLoopCommand::ProxyApiGlobalReply(sc) => {
        // TODO log try restart ???
        send_with_handle_panic!(&mut self.api_send,&mut self.api_handle,ApiCommand::GlobalServiceReply(sc),"TODO api service restart?");
      },*/
      MainLoopCommand::ProxyGlobal(sc) => {
        // send in global service TODO when send from api use a composed sender to send directly
        send_with_handle_panic!(&mut self.global_send,&mut self.global_handle,sc,"Global service finished TODO code to make it restartable cf write service and initialising from conf in init_state + on error right error mgmt");
      },
      MainLoopCommand::ForwardService(ovp,okad,nb_for,sg) => {
        let ol = ovp.as_ref().map(|v|v.len()).unwrap_or(0);
        let olka = okad.as_ref().map(|v|v.len()).unwrap_or(0);
        let nb_exp_for = ol + nb_for + olka;
        // take ovp token : if ovp not connected no forward

        let okad : Option<Vec<usize>> = match okad {
          Some(kad) => {
            let mut res = Vec::with_capacity(kad.len());
            for ka in kad.iter() {
              let do_connect = match self.peer_cache.get_val_c(&ka.0) {
                Some(p) if p.write.is_some() => {
                  res.push(p.write.unwrap());
                  false
                },
                _ => true,
              };
              if do_connect {
                let (write_token,ort) = self.connect_with(&ka.1)?;

                if MC::AUTH_MODE == ShadowAuthType::NoAuth {
                  // no auth to do
                  panic!("TODO add a peer build from address to peer cache");
                  res.push(write_token);
                } else {
                  let chal = self.peermgmt_proto.challenge(self.me.borrow());
                  self.challenge_cache.add_val_c(chal.clone(),ChallengeEntry {
                    write_tok : write_token,
                    read_tok : ort,
                    next_msg : Some(WriteCommand::Service(sg.clone())),
                    next_qid : sg.get_api_reply(),
                  });
                  // send a ping
                  self.write_stream_send(write_token,WriteCommand::Ping(chal), <MC>::init_write_spawner_out()?, None)?;
                }
              }
            }
            Some(res)
          },
          None => None,
        };


        let ovp : Option<Vec<usize>> = ovp.map(|vp|vp.iter().map(|p|self.peer_cache.get_val_c(&p.borrow().get_key()).map(|pc|pc.write)).filter_map(|oow|oow.unwrap_or(None)).collect());
        let ovl = ovp.as_ref().map(|v|v.len()).unwrap_or(0);
        if ovl != ol {
          debug!("Forward of service, some peer where unconnected and skip : init {:?}, send {:?}",ol,ovl);
        }
        let (sg,dests) = if nb_for > 0 {
          let (sg, mut dests) = self.route.route(nb_for,sg,&self.slab_cache, &self.peer_cache)?;
          ovp.map(|ref mut vp| dests.append(vp));
          okad.map(|ref mut vp| dests.append(vp));
          (sg,dests)
        } else {
          match ovp {
            Some(mut vp) => {
              okad.map(|ref mut k| vp.append(k));
              (sg,vp)
            },
            None => {
              match okad {
                Some(k) => (sg,k),
                None => 
                  return Ok(()),
              }
            },
          }
        };
        if dests.len() < nb_exp_for {
          if let Some(qid) = sg.get_api_reply() {
            send_with_handle_panic!(&mut self.api_send,&mut self.api_handle,ApiCommand::Adjust(qid,nb_exp_for - dests.len()),"Api service unreachable");
          }
        }
        if dests.len() == 0 {
          // TODO log
          debug!("Global command not forwarded, no dest found by route");
          println!("no dest for command");
          return Ok(());
        }
        let mut ldest = None;
        for dest in dests {
          if let Some(d) = ldest {
            self.write_stream_send(d,WriteCommand::Service(sg.clone()), <MC>::init_write_spawner_out()?, None)?;
          }
          ldest = Some(dest);
        }
        if let Some(d) = ldest {
          self.write_stream_send(d,WriteCommand::Service(sg), <MC>::init_write_spawner_out()?, None)?;
        }
      },
      MainLoopCommand::ForwardApi(sc,nb_for,ret) => {
        if MC::USE_API_SERVICE && sc.is_api_reply() {
          // shortcut peerstore
          match sc {
            MCCommand::PeerStore(KVStoreCommand::Find(ref qm,ref key,Some(ref aqid))) => {
              if qm.nb_res == 1 {
                let op = self.peer_cache.get_val_c(key).map(|v|v.peer.clone());
                if op.is_some() {
                  return self.call_inner_loop(MainLoopCommand::ProxyApiReply(MCReply::PeerStore(KVStoreReply::FoundApi(op,aqid.clone()))),mlsend,async_yield);
                }
              }
            },
            MCCommand::PeerStore(KVStoreCommand::FindLocally(ref key,ref aqid)) => {
              let op = self.peer_cache.get_val_c(key).map(|v|v.peer.clone());
              if op.is_some() {
                return self.call_inner_loop(MainLoopCommand::ProxyApiReply(MCReply::PeerStore(KVStoreReply::FoundApi(op,aqid.clone()))),mlsend,async_yield);
              }
            },
            _ => (),
          }

          // send in global service TODO when send from api use a composed sender to send directly
          // TODO log try restart ???
          send_with_handle_panic!(&mut self.api_send,&mut self.api_handle,ApiCommand::ServiceCommand(sc,nb_for,ret),"Api service finished TODO code to make it restartable cf write service and initialising from conf in init_state + on error right error mgmt");
        } else {
          match sc {
            sc @ MCCommand::Local(..) => {
              self.call_inner_loop(MainLoopCommand::ForwardService(None,None,nb_for,sc),mlsend,async_yield)?;
            },
            MCCommand::Global(sc) => {
              self.call_inner_loop(MainLoopCommand::ProxyGlobal(GlobalCommand(None,sc)),mlsend,async_yield)?;
            },
            MCCommand::PeerStore(sc) => {
              self.call_inner_loop(MainLoopCommand::PeerStore(GlobalCommand(None,sc)),mlsend,async_yield)?;
            },
          }
        }
      },
      MainLoopCommand::TryConnect(dest_address) => {
        let (write_token,ort) = self.connect_with(&dest_address)?;


        if MC::AUTH_MODE == ShadowAuthType::NoAuth {
          // no auth to do
          panic!("TODO add a peer build from address to peer cache");
        } else {
          let chal = self.peermgmt_proto.challenge(self.me.borrow());
          self.challenge_cache.add_val_c(chal.clone(),ChallengeEntry {
            write_tok : write_token,
            read_tok : ort,
            next_msg : None,
            next_qid : None,
          });
          // send a ping
          self.write_stream_send(write_token,WriteCommand::Ping(chal), <MC>::init_write_spawner_out()?, None)?;
        }

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
          next_msg : None,
          next_qid : None,
        });
  
        // do not store new peer as it is not authentified with a fresh challenge (could be replay)
//         self.update_peer(pr.clone(),pp,Some(wtok),Some(rtok))?;
        // send pong with 2nd challeng
        let pongmess = WriteCommand::Pong(pr,chal,rtok,Some(chal2));
        // with is not used because the command will init it
        self.write_stream_send(wtok, pongmess, <MC>::init_write_spawner_out()?, None)?; // TODO remove peer on error
      },
 
      MainLoopCommand::NewPeerUncheckedChallenge(pr,pp,rtok,chal,nextchal) => {
        // check chal
        match self.challenge_cache.remove_val_c(&chal) {
          Some(chal_entry) => {
            self.update_peer(pr.clone(),pp,Some(chal_entry.write_tok),Some(rtok))?;
            if let Some(nchal) = nextchal {
              let pongmess = WriteCommand::Pong(pr,nchal,rtok,None);
              self.write_stream_send(chal_entry.write_tok, pongmess, <MC>::init_write_spawner_out()?, None)?;
            }
            if let Some(next_msg) = chal_entry.next_msg {
              self.write_stream_send(chal_entry.write_tok, next_msg, <MC>::init_write_spawner_out()?, None)?;
            }
          },
          None => {
            // TODO log inconsistence, do not panic as it can be forge or replay
            panic!("TODO same code as RejectReadSpawn plus RejectPeer with pr and rtok : transport must be close at least, peer??");
          },
        };

      },
      MainLoopCommand::RejectPeer(..) => panic!("TODO in this case our accept method fail : for now same as reject ?? TODO rem from kvstore?? or update peer + Api from challenge cache!!!!!"),
      MainLoopCommand::RejectReadSpawn(..) => panic!("TODO in this case challenge check fail + api from challenge cache (if next_qid) !!!"),
      MainLoopCommand::ProxyWrite(..) => panic!("TODO"),
      MainLoopCommand::NewPeer(..) => panic!("TODO"), // TODO send to peermgmt

    }

    Ok(())
  }

  pub fn main_loop<S : SpawnerYield>(&mut self,rec : &mut MioRecv<<MC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MC>>>::Recv>, mlsend : &mut <MC::MainLoopChannelOut as SpawnChannel<MainLoopReply<MC>>>::Send, req: MainLoopCommand<MC>, async_yield : &mut S) -> Result<()> {
   let mut events = if self.events.is_some() {
     let oevents = replace(&mut self.events,None);
     oevents.unwrap_or(Events::with_capacity(MC::events_size))
   } else {
     Events::with_capacity(MC::events_size)
   };
   self.inner_main_loop(rec, mlsend, req, async_yield, &mut events).map_err(|e|{
     self.events = Some(events);
     e
   })
  }

  fn inner_main_loop<S : SpawnerYield>(&mut self, receiver : &mut MioRecv<<MC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MC>>>::Recv>, mlsend : &mut <MC::MainLoopChannelOut as SpawnChannel<MainLoopReply<MC>>>::Send, req: MainLoopCommand<MC>, async_yield : &mut S, events : &mut Events) -> Result<()> {

    //(self.mainloop_send).send(super::server2::ReadReply::MainLoop(MainLoopCommand::Start))?;
    //(self.mainloop_send).send((MainLoopCommand::Start))?;
    // TODO start other service
    let (smgmt,rmgmt) = self.peermgmt_channel_in.new()?; // TODO persistence bad for suspend -> put smgmt into self plus add handle to send 

    assert!(true == receiver.register(&self.poll, LOOP_COMMAND, Ready::readable(),
                      PollOpt::edge())?);

    self.call_inner_loop(req,mlsend,async_yield)?;
    loop {
      self.poll.poll(events, None)?;
      for event in events.iter() {
        match event.token() {
          LOOP_COMMAND => {
            loop {
              let ocin = receiver.recv()?;
              if let Some(cin) = ocin {
                self.call_inner_loop(cin,mlsend,async_yield)?;
              } else {
                break;
              }
            }
            //  continue;
              // Do not yield on empty receive (could be spurrious), yield from loop is only
              // allowed for suspend/stop commands
              // yield_spawn.spawn_yield();
          },
          LISTENER => {
              let read_token = try_breakloop!(self.transport.accept(), "Transport accept failure : {}", 
              |(rs,ows,ad) : (<MC::Transport as Transport>::ReadStream,Option<<MC::Transport as Transport>::WriteStream>,<MC::Transport as Transport>::Address)| -> Result<usize> {
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
              //Self::start_read_stream_listener(self, read_token, <MC>::init_read_spawner_out()?)?;
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
                  // self.write_stream_send(write_token,WriteCommand::Resume , <MC>::init_write_spawner_out()?)?;
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
              self.write_stream_send(tok.0 - START_STREAM_IX, o_sp_write.unwrap(), <MC>::init_write_spawner_out()?, None)?;
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
             let gl = match self.global_handle.get_weak_handle() {
                Some(h) => Some(HandleSend(self.global_send.clone(),h)),
                None => None,
              };

              let mut rd = ReadDest {
                mainloop : self.mainloop_send.clone(),
                peermgmt : smgmt.clone(),
                global : gl,
                write : owrite_send,
                read_token : tok.0 - START_STREAM_IX,
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
  fn get_write_handle_send(&self, wtoken : usize) -> Option<WriteHandleSend<MC>> {
    let ow = self.slab_cache.get(wtoken);
    if let Some(ca) = ow {
      if let SlabEntryState::WriteSpawned((ref write_handle,ref write_s_in)) = ca.state {
        write_handle.get_weak_handle().map(|wh|HandleSend(write_s_in.clone(),wh))
      } else {None}
    } else {None}
  }

  /// The state of the slab entry must be checked before, return error on wrong state
  fn start_read_stream_listener(&mut self, read_token : usize,
    mut read_out : ReadDest<MC>,
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
        let ah = match self.api_handle.get_weak_handle() {
          Some(h) => Some(HandleSend(self.api_send.clone(),h)),
          None => None,
        };

        // spawn reader TODO try with None (default to Run)
        let read_handle = self.read_spawn.spawn(ReadService::new(
            read_token,
            rs,
            self.me.clone(),
            with,
            //with.map(|w|w.get_sendable()),
            self.enc_proto.clone(),
            self.peermgmt_proto.clone(),
            self.local_spawn_proto.clone(),
            self.local_channel_in_proto.clone(),
            read_out.clone(),
            ah,
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
  ///  TODO write_out as param is probably a mistake
  fn write_stream_send(&mut self, write_token : usize, command : WriteCommand<MC>, write_out : MC::WriteDest, with : Option<MC::PeerRef>) -> Result<()> {
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
              let write_service = WriteService::new(write_token,ws,self.me.clone(),with, self.enc_proto.clone(),self.peermgmt_proto.clone());
              let write_handle = self.write_spawn.spawn(write_service, write_out.clone(), ocin, write_r_in, MC::send_nb_iter)?;
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
            //
            send_with_handle(sender,handle,command)?
          },
          _ => return Err(Error("Call of write listener on wrong state".to_string(), ErrorKind::Bug, None)),
        };
        if finished.is_some() {
          let state = replace(&mut entry.state, SlabEntryState::Empty);
          if let SlabEntryState::WriteSpawned((handle,sender)) = state {
            let (serv,mut sen,recv,result) = handle.unwrap_state()?;
            if result.is_ok() {
              // restart
              let write_handle = self.write_spawn.spawn(serv, sen, finished, recv, MC::send_nb_iter)?;
              replace(&mut entry.state, SlabEntryState::WriteSpawned((write_handle,sender)));
              false
            } else {
              // error TODO error management
              // TODO log
              // TODO send directly to api!!!
              if let Some(qid) = finished.unwrap().get_api_reply() {
                send_with_handle_panic!(&mut self.api_send,&mut self.api_handle,ApiCommand::Failure(qid),"Could not reach api");
              }
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
  read : Option<usize>,
  ///  if not initialized CACHE_NO_STREAM, if 
  write : Option<usize>,
  /// peer priority
  prio : Rc<Cell<PeerPriority>>,
}

impl<P> PeerCacheEntry<P> {
  pub fn get_read_token(&self) -> Option<usize> {
    self.read.clone()
/*    let v = self.read.get();
    if v < START_STREAM_IX {
      None
    } else {
      Some(v)
    }*/
  }
  pub fn get_write_token(&self) -> Option<usize> {
    self.write.clone()
/*    let v = self.write.get();
    if v < START_STREAM_IX {
      None
    } else {
      Some(v)
    }*/
  }
  pub fn set_read_token(&mut self, v : usize) {
    self.read = Some(v)
//    self.read.set(v)
  }
  pub fn set_write_token(&mut self, v : usize) {
    self.write = Some(v)
//    self.write.set(v)
  }
}


