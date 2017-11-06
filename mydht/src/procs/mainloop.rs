//! Main loop for mydht. This is the main dht loop, see default implementation of MyDHT main_loop
//! method.
//! Usage of mydht library requires to create a struct implementing the MyDHTConf trait, by linking with suitable inner trait implementation and their requires component.
use std::clone::Clone;
use std::time::{
  Instant,
  Duration,
};
use std::collections::VecDeque;
use mydht_base::route2::GetPeerRef;
use std::borrow::Borrow;
use procs::synch_transport::{
  SynchConnListener,
  SynchConnListenerCommandDest,
  SynchConnListenerCommandIn,
  SynchConnect,
  SynchConnectCommandIn,
  SynchConnectDest,

};
use procs::storeprop::{
  KVStoreCommand,
  KVStoreCommandSend,
  KVStoreReply,
  KVStoreService,
  OptPeerGlobalDest,
  peer_discover,
  trusted_peer_ping,
  peer_ping,
};
use super::deflocal::{
  GlobalCommand,
  GlobalDest,
};

use super::api::{
  ApiQueryId,
  ApiCommand,
  ApiDest,
  ApiQueriable,
};
use super::{
  FWConf,
  MyDHTConf,
  MCCommand,
  MCReply,
  MainLoopReply,
  MainLoopSendIn,
  ChallengeEntry,
  GlobalHandle,
  ApiHandle,
  PeerStoreHandle,
  SynchConnectHandle,
  Route,
  send_with_handle,
  ShadowAuthType,
  MainLoopRecvIn,
  WriteHandleSend,
  PeerRefSend,
  PeerStatusListener,
  PeerStatusCommand,
  RegReaderBorrow,
};
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
  Spawner,
  SpawnSend,
  SpawnRecv,
  SpawnHandle,
  SpawnUnyield,
  SpawnChannel,
  SpawnerYield,
  DefaultRecv,
  DefaultRecvChannel,
  NoRecv,
};
use std::sync::Arc;

use peer::{
  Peer,
  PeerPriority,
  PeerMgmtMeths,
};
use kvcache::{
  SlabCache,
  Cache,
  KVCache,
  RandCache,
};
use keyval::KeyVal;
use transport::{
  Transport,
  SlabEntry,
  SlabEntryState,
  Registerable,
};
use utils::{
  Proto,
  Ref,
  SRef,
  SToRef,
};
use mio::{
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
    let sync_st = if let $entrystate(ref mut rs,_) = $self.slab_cache.get_mut(token).unwrap().state {
      if rs.register(&$self.poll, Token(token + START_STREAM_IX), Ready::readable(),
        PollOpt::edge())? {
        debug!("Asynch r transport successfully registered");
        false
      } else {
        debug!("Transport not registered, stream is considered as synch stream and connected");
        true
      }
    } else {
      unreachable!();
    };
    if sync_st {
      let (smgmt,_) = $self.peermgmt_channel_in.new()?; // TODO  peer management is unlinked (no service implemented yet)
      $self.start_read_stream_listener(token,$os,smgmt)?;
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
    if let $entrystate(ref mut rs,ref mut infos) = $self.slab_cache.get_mut(token).unwrap().state {
      if rs.register(&$self.poll, Token(token + START_STREAM_IX), Ready::writable(),
        $pollopt )? {
        debug!("Asynch w transport successfully registered");
      } else {
        debug!("WTransport not registered, service configuration for read must run in separate thread and probably without pooling");
        // not asynch transport : it is therefore connected
        infos.2 = true;
      }

    } else {
      unreachable!();
    }
    token
  }
}}


const LISTENER : Token = Token(0);
const LOOP_COMMAND : Token = Token(1);
/// Must be more than registered token (cf other constant) and more than atomic cache status for
/// PeerCache const
const START_STREAM_IX : usize = 2;

/// implementation of api TODO move in its own module
/// Allow call from Rust method or Foreign library
pub struct MyDHT<MC : MyDHTConf>(MainLoopSendIn<MC>);

#[derive(Clone)]
/// subset of mainloop command TODO get api command in here and make MainLoopCommand technical
/// TODO unclear if only for callback
pub enum MainLoopSubCommand<P : Peer> {
//pub enum MainLoopSubCommand<P : Peer,PR,GSC,GSR> {
  TrustedTryConnect(P),
  TryConnect(<P as KeyVal>::Key,<P as Peer>::Address),
  Discover(Vec<(<P as KeyVal>::Key,<P as Peer>::Address)>),
  PoolSize(usize),
}
/// command supported by MyDHT loop
pub enum MainLoopCommand<MC : MyDHTConf> {
  Start,
  SubCommand(MainLoopSubCommand<MC::Peer>),

  TryConnect(<MC::Transport as Transport>::Address,Option<ApiQueryId>),
  TrustedTryConnect(MC::PeerRef,Option<ApiQueryId>),
//  ForwardServiceLocal(MC::LocalServiceCommand,usize),
  ForwardService(Option<Vec<MC::PeerRef>>,Option<Vec<(Option<<MC::Peer as KeyVal>::Key>,Option<<MC::Peer as Peer>::Address>)>>,FWConf,MCCommand<MC>),

  /// forward global command with temporary service and write stream
  ForwardServiceOnce(Option<<MC::Peer as KeyVal>::Key>,Option<<MC::Peer as Peer>::Address>,FWConf,MC::GlobalServiceCommand),
  /// usize is only for mccommand type local to set the number of local to target
  ForwardApi(MCCommand<MC>,usize,MC::ApiReturn),
  /// received peer store command from peer : almost named ProxyPeerStore but it does a local peer
  /// cache check first so it is not named Proxy
  PeerStore(GlobalCommand<MC::PeerRef,KVStoreCommand<MC::Peer,MC::PeerRef,MC::Peer,MC::PeerRef>>),
  /// reject stream for this token and Address
  RejectReadSpawn(usize),
  /// reject a peer (accept fail), usize are write stream token and read stream token
  RejectPeer(<MC::Peer as KeyVal>::Key,Option<usize>,Option<usize>),
  /// New peer after a ping
  NewPeerChallenge(MC::PeerRef,usize,Vec<u8>),
  /// New peer after first or second pong
  NewPeerUncheckedChallenge(MC::PeerRef,PeerPriority,usize,Vec<u8>,Option<Vec<u8>>),
  /// new peer accepted with optionnal read stream token TODO remove (replace by NewPeerChallenge)
  NewPeer(MC::PeerRef,PeerPriority,Option<usize>),
  /// first field is read token ix, write is obtain from os or a connection
  ProxyWrite(usize,WriteCommand<MC>),
  ProxyGlobal(GlobalCommand<MC::PeerRef,MC::GlobalServiceCommand>),
//  GlobalApi(GlobalCommand<MC::PeerRef,MC::GlobalServiceCommand>,MC::ApiReturn),
  ProxyApiReply(MCReply<MC>),
  /// Synch transport received conn
  ConnectedR(<MC::Transport as Transport>::ReadStream, Option<<MC::Transport as Transport>::WriteStream>),
  /// Synch transport conn result
  ConnectedW(usize,<MC::Transport as Transport>::WriteStream, Option<<MC::Transport as Transport>::ReadStream>),
  FailConnect(usize),

}
/// Send variant (to use when inner ref are not Sendable)
pub enum MainLoopCommandSend<MC : MyDHTConf>
 where MC::GlobalServiceCommand : SRef,
       MC::LocalServiceCommand : SRef,
       MC::GlobalServiceReply : SRef,
       MC::LocalServiceReply : SRef {
  Start,
  SubCommand(MainLoopSubCommand<MC::Peer>),
  TryConnect(<MC::Transport as Transport>::Address,Option<ApiQueryId>),
  TrustedTryConnect(PeerRefSend<MC>,Option<ApiQueryId>),
//  ForwardServiceLocal(<MC::LocalServiceCommand as SRef>::Send,usize),
  ForwardService(Option<Vec<PeerRefSend<MC>>>,Option<Vec<(Option<<MC::Peer as KeyVal>::Key>,Option<<MC::Peer as Peer>::Address>)>>,FWConf,<MCCommand<MC> as SRef>::Send),
  ForwardServiceOnce(Option<<MC::Peer as KeyVal>::Key>,Option<<MC::Peer as Peer>::Address>,FWConf,<MC::GlobalServiceCommand as SRef>::Send),
  ForwardApi(<MCCommand<MC> as SRef>::Send,usize,MC::ApiReturn),
  PeerStore(GlobalCommand<PeerRefSend<MC>,KVStoreCommandSend<MC::Peer,MC::PeerRef,MC::Peer,MC::PeerRef>>),
  /// reject stream for this token and Address
  RejectReadSpawn(usize),
  /// reject a peer (accept fail), usize are write stream token and read stream token
  RejectPeer(<MC::Peer as KeyVal>::Key,Option<usize>,Option<usize>),
  /// new peer accepted with optionnal read stream token
  NewPeer(PeerRefSend<MC>,PeerPriority,Option<usize>),
  NewPeerChallenge(PeerRefSend<MC>,usize,Vec<u8>),
  NewPeerUncheckedChallenge(PeerRefSend<MC>,PeerPriority,usize,Vec<u8>,Option<Vec<u8>>),
  ProxyWrite(usize,WriteCommandSend<MC>),
  ProxyGlobal(GlobalCommand<PeerRefSend<MC>,<MC::GlobalServiceCommand as SRef>::Send>),
//  GlobalApi(GlobalCommand<PeerRefSend<MC>,<MC::GlobalServiceCommand as SRef>::Send>,MC::ApiReturn),
  ProxyApiReply(<MCReply<MC> as SRef>::Send),
//  ProxyApiGlobalReply(<MC::GlobalServiceReply as SRef>::Send),
  ConnectedR(<MC::Transport as Transport>::ReadStream, Option<<MC::Transport as Transport>::WriteStream>),
  ConnectedW(usize,<MC::Transport as Transport>::WriteStream, Option<<MC::Transport as Transport>::ReadStream>),
  FailConnect(usize),
}

impl<MC : MyDHTConf> Clone for MainLoopCommand<MC> 
 where MC::GlobalServiceCommand : Clone,
       MC::LocalServiceCommand : Clone,
       MC::GlobalServiceReply : Clone,
       MC::LocalServiceReply : Clone {
  fn clone(&self) -> Self {
    match *self {
      MainLoopCommand::Start => MainLoopCommand::Start,
      MainLoopCommand::SubCommand(ref sc) => MainLoopCommand::SubCommand(sc.clone()),
      MainLoopCommand::TryConnect(ref a,ref aid) => MainLoopCommand::TryConnect(a.clone(),aid.clone()),
      MainLoopCommand::TrustedTryConnect(ref p,ref aid) => MainLoopCommand::TrustedTryConnect(p.clone(),aid.clone()),
//      MainLoopCommand::ForwardServiceLocal(ref gc,nb) => MainLoopCommand::ForwardServiceLocal(gc.clone(),nb),
      MainLoopCommand::ForwardService(ref ovp,ref okad,ref nb_for,ref c) => MainLoopCommand::ForwardService(ovp.clone(),okad.clone(),nb_for.clone(),c.clone()),
      MainLoopCommand::ForwardServiceOnce(ref ovp,ref okad,ref nb_for,ref c) => MainLoopCommand::ForwardServiceOnce(ovp.clone(),okad.clone(),nb_for.clone(),c.clone()),
      MainLoopCommand::ForwardApi(ref gc,nb_for,ref ret) => MainLoopCommand::ForwardApi(gc.clone(),nb_for,ret.clone()),
      MainLoopCommand::PeerStore(ref cmd) => MainLoopCommand::PeerStore(cmd.clone()),
      MainLoopCommand::RejectReadSpawn(s) => MainLoopCommand::RejectReadSpawn(s),
      MainLoopCommand::RejectPeer(ref k,ref os,ref os2) => MainLoopCommand::RejectPeer(k.clone(),os.clone(),os2.clone()),
      MainLoopCommand::NewPeer(ref rp,ref pp,ref os) => MainLoopCommand::NewPeer(rp.clone(),pp.clone(),os.clone()),
      MainLoopCommand::NewPeerChallenge(ref rp,rtok,ref chal) => MainLoopCommand::NewPeerChallenge(rp.clone(),rtok,chal.clone()),
      MainLoopCommand::NewPeerUncheckedChallenge(ref rp,ref pp,rtok,ref chal,ref nextchal) => MainLoopCommand::NewPeerUncheckedChallenge(rp.clone(),pp.clone(),rtok,chal.clone(),nextchal.clone()),
      MainLoopCommand::ProxyWrite(rt, ref rcs) => MainLoopCommand::ProxyWrite(rt, rcs.clone()),
      MainLoopCommand::ProxyGlobal(ref rcs) => MainLoopCommand::ProxyGlobal(rcs.clone()),
//      MainLoopCommand::GlobalApi(ref rcs,ref ret) => MainLoopCommand::GlobalApi(rcs.clone(),ret.clone()),
      MainLoopCommand::ProxyApiReply(ref rcs) => MainLoopCommand::ProxyApiReply(rcs.clone()),
//      MainLoopCommand::ProxyApiGlobalReply(ref rcs) => MainLoopCommand::ProxyApiGlobalReply(rcs.clone()),
      MainLoopCommand::ConnectedR(..) => unreachable!(),
      MainLoopCommand::ConnectedW(..) => unreachable!(),
      MainLoopCommand::FailConnect(six) => MainLoopCommand::FailConnect(six),
    }
  }
}

impl<MC : MyDHTConf> SRef for MainLoopCommand<MC>
 where MC::GlobalServiceCommand : SRef,
       MC::LocalServiceCommand : SRef,
       MC::GlobalServiceReply : SRef,
       MC::LocalServiceReply : SRef {
  type Send = MainLoopCommandSend<MC>;
  fn get_sendable(self) -> Self::Send {
    match self {
      MainLoopCommand::Start => MainLoopCommandSend::Start,
      MainLoopCommand::SubCommand(sc) => MainLoopCommandSend::SubCommand(sc),
      MainLoopCommand::TryConnect(a,aid) => MainLoopCommandSend::TryConnect(a,aid),
      MainLoopCommand::TrustedTryConnect(p,aid) => MainLoopCommandSend::TrustedTryConnect(p.get_sendable(),aid),
//      MainLoopCommand::ForwardServiceLocal(ref gc,nb) => MainLoopCommandSend::ForwardServiceLocal(gc.get_sendable(),nb),
      MainLoopCommand::ForwardService(ovp,okad,nb_for,c) => MainLoopCommandSend::ForwardService({
          ovp.map(|vp|vp.into_iter().map(|p|p.get_sendable()).collect())
        },okad,nb_for,c.get_sendable()),
      MainLoopCommand::ForwardServiceOnce(ovp,okad,nb_for,c) => MainLoopCommandSend::ForwardServiceOnce(
        ovp,okad,nb_for,c.get_sendable()),

      MainLoopCommand::ForwardApi(gc,nb_for,ret) => MainLoopCommandSend::ForwardApi(gc.get_sendable(),nb_for,ret),
      MainLoopCommand::PeerStore(cmd) => MainLoopCommandSend::PeerStore(cmd.get_sendable()),
      MainLoopCommand::RejectReadSpawn(s) => MainLoopCommandSend::RejectReadSpawn(s),
      MainLoopCommand::RejectPeer(k,os,os2) => MainLoopCommandSend::RejectPeer(k,os,os2),
      MainLoopCommand::NewPeer(rp,pp,os) => MainLoopCommandSend::NewPeer(rp.get_sendable(),pp,os),
      MainLoopCommand::NewPeerChallenge(rp,rtok,chal) => MainLoopCommandSend::NewPeerChallenge(rp.get_sendable(),rtok,chal),
      MainLoopCommand::NewPeerUncheckedChallenge(rp,pp,rtok,chal,nextchal) => MainLoopCommandSend::NewPeerUncheckedChallenge(rp.get_sendable(),pp,rtok,chal,nextchal),
      MainLoopCommand::ProxyWrite(rt,rcs) => MainLoopCommandSend::ProxyWrite(rt, rcs.get_sendable()),
      MainLoopCommand::ProxyGlobal(rcs) => MainLoopCommandSend::ProxyGlobal(rcs.get_sendable()),
//      MainLoopCommand::GlobalApi(ref rcs,ref ret) => MainLoopCommandSend::GlobalApi(rcs.get_sendable(),ret.clone()),
      MainLoopCommand::ProxyApiReply(rcs) => MainLoopCommandSend::ProxyApiReply(rcs.get_sendable()),
//      MainLoopCommand::ProxyApiLocalReply(ref rcs) => MainLoopCommandSend::ProxyApiLocalReply(rcs.get_sendable()),
      // curently no service usage on synch listener
      MainLoopCommand::ConnectedR(..) => unreachable!(),
      MainLoopCommand::ConnectedW(..) => unreachable!(),
      MainLoopCommand::FailConnect(i) => MainLoopCommandSend::FailConnect(i),
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
      MainLoopCommandSend::SubCommand(sc) => MainLoopCommand::SubCommand(sc),
      MainLoopCommandSend::TryConnect(a,aid) => MainLoopCommand::TryConnect(a,aid),
      MainLoopCommandSend::TrustedTryConnect(p,aid) => MainLoopCommand::TrustedTryConnect(p.to_ref(),aid),
//      MainLoopCommandSend::ForwardServiceLocal(a,nb) => MainLoopCommand::ForwardServiceLocal(a.to_ref(),nb),
      MainLoopCommandSend::ForwardService(ovp,okad,nb_for,c) => MainLoopCommand::ForwardService({
          ovp.map(|vp|vp.into_iter().map(|p|p.to_ref()).collect())
        },okad,nb_for,c.to_ref()),
      MainLoopCommandSend::ForwardServiceOnce(ovp,okad,nb_for,c) => MainLoopCommand::ForwardServiceOnce(
          ovp,okad,nb_for,c.to_ref()),
      MainLoopCommandSend::ForwardApi(a,nb_for,r) => MainLoopCommand::ForwardApi(a.to_ref(),nb_for,r),
      MainLoopCommandSend::PeerStore(cmd) => MainLoopCommand::PeerStore(cmd.to_ref()),
      MainLoopCommandSend::RejectReadSpawn(s) => MainLoopCommand::RejectReadSpawn(s),
      MainLoopCommandSend::RejectPeer(k,os,os2) => MainLoopCommand::RejectPeer(k,os,os2),
      MainLoopCommandSend::NewPeer(rp,pp,os) => MainLoopCommand::NewPeer(rp.to_ref(),pp,os),
      MainLoopCommandSend::NewPeerChallenge(rp,rtok,chal) => MainLoopCommand::NewPeerChallenge(rp.to_ref(),rtok,chal),
      MainLoopCommandSend::NewPeerUncheckedChallenge(rp,pp,rtok,chal,nchal) => MainLoopCommand::NewPeerUncheckedChallenge(rp.to_ref(),pp,rtok,chal,nchal),
      MainLoopCommandSend::ProxyWrite(rt,rcs) => MainLoopCommand::ProxyWrite(rt,rcs.to_ref()),
      MainLoopCommandSend::ProxyGlobal(rcs) => MainLoopCommand::ProxyGlobal(rcs.to_ref()),
//      MainLoopCommandSend::GlobalApi(rcs,r) => MainLoopCommand::GlobalApi(rcs.to_ref(),r),
      MainLoopCommandSend::ProxyApiReply(rcs) => MainLoopCommand::ProxyApiReply(rcs.to_ref()),
//      MainLoopCommandSend::ProxyApiLocalReply(rcs) => MainLoopCommand::ProxyApiLocalReply(rcs.to_ref()),
      MainLoopCommandSend::ConnectedR(..) => unreachable!(),
      MainLoopCommandSend::ConnectedW(..) => unreachable!(),
      MainLoopCommandSend::FailConnect(i) => MainLoopCommand::FailConnect(i),
    }
  }
}

pub struct MDHTState<MC : MyDHTConf> {
  me : MC::PeerRef,
  transport : Option<MC::Transport>,
  transport_synch : Option<Arc<MC::Transport>>,
  route : MC::Route,
  slab_cache : MC::Slab,
  peer_cache : MC::PeerCache,
  address_cache : MC::AddressCache,
  challenge_cache : MC::ChallengeCache,
  events : Option<Events>,
  poll : Poll,
  read_spawn : MC::ReadSpawn,
  read_channel_in : DefaultRecvChannel<ReadCommand<MC>, MC::ReadChannelIn>,
  write_spawn : MC::WriteSpawn,
  write_channel_in : MC::WriteChannelIn,
  peermgmt_channel_in : MC::PeerMgmtChannelIn,
  enc_proto : MC::MsgEnc,
  /// currently locally use only for challenge TODO rename (proto not ok) or split trait to have a
  /// challenge only instance
  peermgmt_proto : MC::PeerMgmtMeths,
  pub mainloop_send : MainLoopSendIn<MC>,

  /// send to global service
  global_send : <MC::GlobalServiceChannelIn as SpawnChannel<GlobalCommand<MC::PeerRef,MC::GlobalServiceCommand>>>::Send,
  global_handle : GlobalHandle<MC>,
  local_spawn_proto : MC::LocalServiceSpawn,
  local_service_proto : MC::LocalServiceProto,
  local_channel_in_proto : MC::LocalServiceChannelIn,
  api_send : <MC::ApiServiceChannelIn as SpawnChannel<ApiCommand<MC>>>::Send,
  api_handle : ApiHandle<MC>,
  peerstore_send : <MC::PeerStoreServiceChannelIn as SpawnChannel<GlobalCommand<MC::PeerRef,KVStoreCommand<MC::Peer,MC::PeerRef,MC::Peer,MC::PeerRef>>>>::Send,
  peerstore_handle : PeerStoreHandle<MC>,

  discover_wait_route : VecDeque<(MCCommand<MC>,usize,usize)>,

  peer_pool_maintain : usize,
  peer_last_query : Instant,
  //peer_pool_wait : Vec<(usize,usize,fn(Vec<MC::PeerRef>) -> MainLoopCommand<MC>)>,


  synch_connect_ix : usize,
  synch_connect_spawn : MC::SynchConnectSpawn,
  synch_connect_channel_in : MC::SynchConnectChannelIn,
  synch_connect_handle_send : Vec<(
    SynchConnectHandle<MC>,
    <MC::SynchConnectChannelIn as SpawnChannel<SynchConnectCommandIn<MC::Transport>>>::Send,
  )>,
}

impl<MC : MyDHTConf> MDHTState<MC> {

  pub fn init_state(conf : &mut MC,mlsend : &mut MainLoopSendIn<MC>) -> Result<MDHTState<MC>> {
  //pub fn init_state(conf : &mut MC,mlsend : &mut MainLoopSendIn<MC>) -> Result<MDHTState<MC>> {
    let poll = Poll::new()?;
    let transport = conf.init_transport()?;
    let route = conf.init_route()?;
    let (transport, transport_synch) = if transport.register(&poll, LISTENER, Ready::readable(),
                    PollOpt::edge())? {
      debug!("Asynch transport successfully registered");
      (Some(transport),None)
    } else {
      debug!("Transport not registered, service configuration for read must run in separate thread and probably without pooling");
      let atransport = Arc::new(transport);
      // start infinite listener loop
      conf.init_synch_listener_spawn()?.spawn(
        SynchConnListener(atransport.clone()),
        SynchConnListenerCommandDest(mlsend.clone()),
        None,
        DefaultRecv(NoRecv,SynchConnListenerCommandIn),
        0)?;
      (None,Some(atransport))
    };
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
    let api_gdest = api_handle.get_weak_handle().map(|wh|{
      MC::ApiServiceChannelIn::get_weak_send(&api_send).map(|aps|
        HandleSend(aps,wh)
      )
    }).unwrap_or(None);
    let global_dest = GlobalDest {
      mainloop : mlsend.clone(),
      api : api_gdest,
    };
    let api_gdest_peer = api_handle.get_weak_handle().map(|wh|{
      MC::ApiServiceChannelIn::get_weak_send(&api_send).map(|aps|
        HandleSend(aps,wh)
      )
    }).unwrap_or(None);
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
      init_cache : conf.init_peer_kvstore_query_cache()?,
      store : None,
      dht_rules : conf.init_dhtrules_proto()?,
      query_cache : None,
      discover : conf.do_peer_query_forward_with_discover(),
      _ph : PhantomData,
    };
    let peerstore_dest = OptPeerGlobalDest(global_dest_peer);
    let peerstore_handle = peerstore_spawn.spawn(peerstore_service,peerstore_dest,None,peerstore_recv, MC::PEERSTORE_NB_ITER)?;

    let s = MDHTState {
      me : me,
      transport : transport,
      transport_synch : transport_synch,
      route : route,
      slab_cache : conf.init_main_loop_slab_cache()?,
      peer_cache : conf.init_main_loop_peer_cache()?,
      address_cache : conf.init_main_loop_address_cache()?,
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
      mainloop_send : mlsend.clone(),
      global_send : global_send,
      global_handle : global_handle,
      local_channel_in_proto : local_channel_in,
      local_spawn_proto : local_spawn,
      local_service_proto : conf.init_local_service_proto()?,
      api_handle : api_handle,
      api_send : api_send,
      peerstore_send : peerstore_send,
      peerstore_handle : peerstore_handle,
      discover_wait_route : VecDeque::new(),
      peer_pool_maintain : MC::PEER_POOL_INIT_SIZE,
      peer_last_query : Instant::now() - Duration::from_millis(MC::PEER_POOL_DELAY_MS),
      //peer_pool_wait : Vec::new(),
      synch_connect_ix : 0,
      synch_connect_handle_send : Vec::with_capacity(MC::NB_SYNCH_CONNECT),
      synch_connect_channel_in : conf.init_synch_connect_channel_in()?,
      synch_connect_spawn : conf.init_synch_connect_spawn()?,
    };
    Ok(s)
  }


  #[inline]
  fn connect_with(&mut self, dest_address : &<MC::Peer as Peer>::Address) -> Result<(usize, Option<usize>)> {
    self.connect_with2(dest_address,true,None)
  }
  /// trusted peer is for noauth sending to global on connect
  fn connect_with2(&mut self, dest_address : &<MC::Peer as Peer>::Address, allow_mult : bool, trusted_peer : Option<MC::PeerRef>) -> Result<(usize, Option<usize>)> {
    if self.transport.is_some() {
      // first check cache if there is a connect already : no (if peer yes) 
      let (ws,mut ors) = self.transport.as_ref().unwrap().connectwith(&dest_address)?;

      if !allow_mult {
        ors = None;
      }

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

      self.address_cache.add_val_c(dest_address.clone(),AddressCacheEntry {
        read : ort,
        write : write_token,
      });
      if let Some(pr) = trusted_peer {
        if <MC::GlobalServiceCommand as PeerStatusListener<MC::PeerRef>>::DO_LISTEN {
          let listen_global = <MC::GlobalServiceCommand as PeerStatusListener<MC::PeerRef>>::build_command(PeerStatusCommand::PeerOnline(pr.clone(),PeerPriority::Unchecked));
          send_with_handle_panic!(&mut self.global_send,&mut self.global_handle,GlobalCommand::Local(listen_global),"Panic sending to peer online to global service");
        }
      }

      Ok((write_token,ort))
    } else {
      let (s,r) = self.write_channel_in.new()?;
      let connect_token = self.slab_cache.insert (
        SlabEntry {
          state : SlabEntryState::WriteConnectSynch((s,r,false),trusted_peer),
//           state : SlabEntryState::WriteConnectSynch((ref write_s_in ,ref write_r_in,ref has_connect)),
          os : None,
          peer : None,
        }
      );
      self.address_cache.add_val_c(dest_address.clone(),AddressCacheEntry {
        read : None,
        write : connect_token,
      });
      let do_spawn = if self.synch_connect_ix == self.synch_connect_handle_send.len() {
        if self.synch_connect_handle_send.len() < MC::NB_SYNCH_CONNECT {
          true
        } else {
          // restart pool
          self.synch_connect_ix = 0;
          self.synch_connect_handle_send[self.synch_connect_ix].0.is_finished()
        }
      } else {
        // spawn a new on finish (the service is stateless and we do not restart from its state :
        // WARN this could become an issue if it evolves
        self.synch_connect_handle_send[self.synch_connect_ix].0.is_finished()
      };
      if do_spawn {
        let atr = self.transport_synch.as_ref().unwrap().clone();
        // spawn new
        let (s,r) = self.synch_connect_channel_in.new()?;
        let h = self.synch_connect_spawn.spawn(
          SynchConnect(atr),
          SynchConnectDest(self.mainloop_send.clone()),
          Some(SynchConnectCommandIn(connect_token,dest_address.clone())),
          r,
          MC::SYNCH_CONNECT_NB_ITER)?;
        if self.synch_connect_ix < self.synch_connect_handle_send.len() {
          self.synch_connect_handle_send[self.synch_connect_ix] = (h,s);
        } else {
          self.synch_connect_handle_send.push((h,s));
        }
      } else {
        self.synch_connect_handle_send[self.synch_connect_ix].0.unyield()?;
        self.synch_connect_handle_send[self.synch_connect_ix].1.send(SynchConnectCommandIn(connect_token,dest_address.clone()))?;
      }
      // move ix
      self.synch_connect_ix += 1;

      Ok((connect_token,None))
    }

  }

  fn update_peer(&mut self, pr : MC::PeerRef, pp : PeerPriority, owtok : Option<usize>, owread : Option<usize>) -> Result<()> {
    debug!("Peer added with slab entries : w{:?}, r{:?}",owtok,owread);
    let pk = pr.borrow().get_key();
    // TODO conflict for existing peer (close )
    // TODO useless has_val_c : u
    let o_slab_remove = if let Some(p_entry) = self.peer_cache.get_val_c(&pk) {
      // TODO update peer, if xisting write should be drop (new one should be use) and read could
      // receive a message for close on timeout (read should stay open as at the other side on
      // double connect the peer can keep sending on this read : the new read could timeout in
      // fact
      // TODO add a logg for this removeal and one for double read_token
      // TODO clean close??
      // TODO check if read is finished and remove if finished
      p_entry.get_write_token()
    } else { None };
    if <MC::GlobalServiceCommand as PeerStatusListener<MC::PeerRef>>::DO_LISTEN {
      let listen_global = <MC::GlobalServiceCommand as PeerStatusListener<MC::PeerRef>>::build_command(PeerStatusCommand::PeerOnline(pr.clone(),pp.clone()));
      send_with_handle_panic!(&mut self.global_send,&mut self.global_handle,GlobalCommand::Local(listen_global),"Panic sending to peer online to global service");
    }
    self.peer_cache.add_val_c(pk, PeerCacheEntry {
      peer : pr.clone(),
      read : owread,
      write : owtok,
      prio : pp,
    });

    let update_peer = KVStoreCommand::StoreLocally(pr,0,None);
    // peer_service update
    send_with_handle_panic!(&mut self.peerstore_send,&mut self.peerstore_handle,GlobalCommand::Local(update_peer),"Panic sending to peerstore TODO consider restart (init of peerstore is a fn)");
    // now that cache update remove can be done without race (considering send is ordered)
    if let Some(t) = o_slab_remove {
      debug!("slab cache remove due to double connect : {:?}",t);
      self.slab_cache.remove(t);
    }


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
      MainLoopCommand::SubCommand(sc) => {
        match sc {
          MainLoopSubCommand::TrustedTryConnect(peer) => {
            if !self.peer_cache.has_val_c(peer.get_key_ref()) && !self.address_cache.has_val_c(peer.get_address()) {
              self.call_inner_loop(MainLoopCommand::TrustedTryConnect(<MC::PeerRef as Ref<_>>::new(peer),None),mlsend,async_yield)?;
            }
          },
          MainLoopSubCommand::TryConnect(key,add) => {
            if !self.peer_cache.has_val_c(&key) && !self.address_cache.has_val_c(&add) {
              self.call_inner_loop(MainLoopCommand::TryConnect(add,None),mlsend,async_yield)?;
            }
          },
          MainLoopSubCommand::PoolSize(min_no_repeat) => {
            if self.peer_pool_maintain < min_no_repeat {
              debug!("change pool maintain from {:?} to {:?}",self.peer_pool_maintain,min_no_repeat); 
              self.peer_pool_maintain = min_no_repeat;
              // do not wait next peer pool check
              let cache_l = if MC::AUTH_MODE == ShadowAuthType::NoAuth {
                self.address_cache.len_c()
              } else {
                self.peer_cache.len_c()
              };
              if cache_l < self.peer_pool_maintain {
                // do update immediatly
                self.peer_last_query = Instant::now();
              }

            }
    /*        if self.peer_cache.len_c() < min_no_repeat {
              self.peer_pool_wait.push((min_no_repeat,nb_tot,on_res));
            } else {
              let peers = self.peer_cache.exact_rand(min_no_repeat,nb_tot)?.into_iter().map(|pc|pc.peer).collect();
              self.call_inner_loop(on_res(peers),mlsend,async_yield)?;
            }*/
          },

          MainLoopSubCommand::Discover(lkad) => {
            debug!("Discover fwd to : {:?}", lkad.len());
            let mut nb_send = 0;
            if let Some((command, nb_for, rem_call)) = self.discover_wait_route.pop_back() {

              assert!(nb_for >= lkad.len());
              for ka in lkad {
                // do not use connected peer (some may be accepted in between but it is view as
                // harmless)
                if !self.peer_cache.has_val_c(&ka.0) {
                  let ocache = self.address_cache.get_val_c(&ka.1).map(|acache|(acache.write,acache.read));
                  let (need_ping,(write_token,ort)) = if let Some(c) = ocache {
                    (false,c)
                  } else {
                    (true,self.connect_with(&ka.1)?)
                  };

                  if MC::AUTH_MODE == ShadowAuthType::NoAuth || need_ping == false {

                    // no auth to do
                    self.write_stream_send(write_token,WriteCommand::Service(command.clone()), <MC>::init_write_spawner_out()?, None)?;
                    nb_send += 1;
                  } else {
                    let chal = self.peermgmt_proto.challenge(self.me.borrow());
                    self.challenge_cache.add_val_c(chal.clone(),ChallengeEntry {
                      write_tok : write_token,
                      read_tok : ort,
                      next_msg : Some(WriteCommand::Service(command.clone())),
                      next_qid : command.get_api_reply(),
                      api_qid : None,
                    });
                    self.write_stream_send(write_token,WriteCommand::Ping(chal), <MC>::init_write_spawner_out()?, None)?;
                    nb_send += 1;
                  }
                }
                if nb_send == nb_for {
                  break;
                }
              }
              if nb_send < nb_for && rem_call > 0 {

                let nb_disco = nb_for - nb_send;
                self.discover_wait_route.push_front((command.clone(),nb_disco,rem_call - 1));
                let kvscom = GlobalCommand::Local(KVStoreCommand::Subset(nb_disco, peer_discover));
                send_with_handle_panic!(&mut self.peerstore_send,&mut self.peerstore_handle,kvscom,"Panic sending to peerstore");
              }
            }
          },
        }
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
      MainLoopCommand::ForwardServiceOnce(ok,oad,fwconf,sg) => {
        let address = match oad {
          Some(ad) => ad,
          None => {
            if let Some(Some(a)) = ok.map(|k|self.peer_cache.get_val_c(&k).map(|pe|pe.peer.borrow().get_address().clone())) {
              a
            } else {
              warn!("Could not forward service in forward service once");
              return Ok(());
            }
          },
        };
        // connect without mult
        let (write_token,_ort) = self.connect_with2(&address,false,None)?;
        sg.get_read().map(|rreg|
          rreg.reregister(&self.poll, Token(write_token + START_STREAM_IX), Ready::readable(),
          PollOpt::edge()).unwrap());
        println!("read reregister : {:?}",write_token);
        if MC::AUTH_MODE != ShadowAuthType::NoAuth {
          let chal = self.peermgmt_proto.challenge(self.me.borrow());
          let apr = sg.get_api_reply();
          self.challenge_cache.add_val_c(chal.clone(),ChallengeEntry {
            write_tok : write_token,
            read_tok : None,
            next_msg : Some(WriteCommand::Service(MCCommand::Global(sg))),
            next_qid : apr,
            api_qid : None,
          });
          // send a ping
          self.write_stream_send2(write_token,WriteCommand::Ping(chal), <MC>::init_write_spawner_out()?, None,false)?;
        } else {
          self.write_stream_send2(write_token,WriteCommand::Service(MCCommand::Global(sg)), <MC>::init_write_spawner_out()?, None,false)?;
        }
      },
      MainLoopCommand::ForwardService(ovp,okad,fwconf,sg) => {
        
        let ol = ovp.as_ref().map(|v|v.len()).unwrap_or(0);
        let olka = okad.as_ref().map(|v|v.len()).unwrap_or(0);
        let nb_exp_for = ol + fwconf.nb_for + olka;
        let mut nb_disco = 0;
        let mut ws_res : Vec<usize> = Vec::with_capacity(nb_exp_for);
        // take ovp token : if ovp not connected no forward
        if let Some(vp) = ovp {
          for p in vp.iter() {
            let k = p.borrow().get_key_ref();
            self.peer_cache.get_val_c(k).map(|pc|pc.write.map(|ws|ws_res.push(ws)));
          }
        }
        if ws_res.len() != ol {
          debug!("Forward of service, some peer where unconnected and skip : init {:?}, send {:?}",ol,ws_res.len());
        }

        match okad {
          Some(kad) => {
            for ka in kad.iter() {
              let do_connect = ka.0.as_ref().map(|key|match self.peer_cache.get_val_c(key) {
                Some(p) if p.write.is_some() => {
                  ws_res.push(p.write.unwrap());
                  false
                },
                _ => fwconf.discover,
              }).unwrap_or(fwconf.discover);

              if do_connect && ka.1.is_some() {
                let ocache = self.address_cache.get_val_c(ka.1.as_ref().unwrap()).map(|acache|(acache.write,acache.read));
                let (need_ping,(write_token,ort)) = if let Some(c) = ocache {
                  (false,c)
                } else {
                  (true,self.connect_with(ka.1.as_ref().unwrap())?)
                };


                if MC::AUTH_MODE == ShadowAuthType::NoAuth || !need_ping {
                  // no auth to do
                  ws_res.push(write_token);
                } else {
                  let chal = self.peermgmt_proto.challenge(self.me.borrow());
                  self.challenge_cache.add_val_c(chal.clone(),ChallengeEntry {
                    write_tok : write_token,
                    read_tok : ort,
                    next_msg : Some(WriteCommand::Service(sg.clone())),
                    next_qid : sg.get_api_reply(),
                    api_qid : None,
                  });
                  // send a ping
                  self.write_stream_send(write_token,WriteCommand::Ping(chal), <MC>::init_write_spawner_out()?, None)?;
                  nb_disco += 1;
                }
              }
            }
          },
          None => (),
        };


        let sg = if fwconf.nb_for > 0 {
          let (sg, mut dests) = self.route.route(fwconf.nb_for,sg,&mut self.slab_cache, &mut self.peer_cache)?;
          debug!("dests replied from route call of {} :{:?}",fwconf.nb_for,&dests[..]);
          ws_res.append(&mut dests);
          sg
        } else {
          sg
        };
        let nb_ok = ws_res.len() + nb_disco;
        if nb_ok < nb_exp_for {
          if fwconf.discover {
            let nb_disco = nb_exp_for - nb_ok;
            // TODO a max size for the buffer and adjust if max
            self.discover_wait_route.push_front((sg.clone(),nb_disco,MC::MAX_NB_DISCOVER_CALL));
            let kvscom = GlobalCommand::Local(KVStoreCommand::Subset(nb_disco, peer_discover));
            send_with_handle_panic!(&mut self.peerstore_send,&mut self.peerstore_handle,kvscom,"Panic sending to peerstore");
          } else {
            if let Some(qid) = sg.get_api_reply() {
              send_with_handle_panic!(&mut self.api_send,&mut self.api_handle,ApiCommand::Adjust(qid,nb_exp_for - nb_ok),"Api service unreachable");
            }
            // TODO if forward from global service use fw service callback (cf comment at start of
            // api)
            if nb_ok == 0 {
              debug!("Global command not forwarded, no dest found by route");
            }

          }
        }
        let mut ldest = None;
        for dest in ws_res {
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
              self.call_inner_loop(MainLoopCommand::ForwardService(None,None,FWConf{ nb_for : nb_for, discover : false },sc),mlsend,async_yield)?;
            },
            MCCommand::Global(sc) => {
              self.call_inner_loop(MainLoopCommand::ProxyGlobal(GlobalCommand::Local(sc)),mlsend,async_yield)?;
            },
            MCCommand::PeerStore(sc) => {
              self.call_inner_loop(MainLoopCommand::PeerStore(GlobalCommand::Local(sc)),mlsend,async_yield)?;
            },
            MCCommand::TryConnect(ad,aid) => {
              self.call_inner_loop(MainLoopCommand::TryConnect(ad,aid),mlsend,async_yield)?;
            }
          }
        }
      },
      MainLoopCommand::TryConnect(dest_address,oapi) => {
        let add_incache = self.address_cache.has_val_c(&dest_address);
        if !add_incache {
          let (write_token,ort) = self.connect_with(&dest_address)?;
          if MC::AUTH_MODE == ShadowAuthType::NoAuth  {
            // no auth to do cache addresse done in connect_with
          } else {
            let chal = self.peermgmt_proto.challenge(self.me.borrow());
            self.challenge_cache.add_val_c(chal.clone(),ChallengeEntry {
              write_tok : write_token,
              read_tok : ort,
              next_msg : None,
              next_qid : None,
              api_qid : oapi,
            });
            // send a ping
            self.write_stream_send(write_token,WriteCommand::Ping(chal), <MC>::init_write_spawner_out()?, None)?;
          }
        }

      },
      MainLoopCommand::TrustedTryConnect(peer,oapi) => {
        let dest_address = peer.borrow().get_address().clone();
        let add_incache = self.address_cache.has_val_c(&dest_address);
        if MC::AUTH_MODE == ShadowAuthType::NoAuth  {
          // we call even if in cache : may not have trusted peer info
 
          // no auth to do cache addresse done in connect_with
          self.connect_with2(&dest_address,true,Some(peer))?;
        } else {
          if !add_incache {
            let (write_token,ort) = self.connect_with2(&dest_address,true,None)?;
            let chal = self.peermgmt_proto.challenge(self.me.borrow());
            self.challenge_cache.add_val_c(chal.clone(),ChallengeEntry {
              write_tok : write_token,
              read_tok : ort,
              next_msg : None,
              next_qid : None,
              api_qid : oapi,
            });
            // send a ping
            self.write_stream_send(write_token,WriteCommand::Ping(chal), <MC>::init_write_spawner_out()?, None)?;
          }
        }
      },
      MainLoopCommand::NewPeerChallenge(pr,rtok,chal) => {
        let owtok = self.slab_cache.get(rtok).map(|r|r.os);
        let wtok = match owtok {
          Some(Some(tok)) => tok,
          Some(None) => {
            // do not use address cache : if already connecting we cannot use it as in different
            // auth state (first ping and here we received ping to send first pong : dest expect a
            // second pong).
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
          api_qid : None,
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
            if let Some(qid) = chal_entry.api_qid {
              send_with_handle_panic!(&mut self.api_send,&mut self.api_handle,ApiCommand::ServiceReply(MCReply::Done(qid)),"Could not reach api");
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
      MainLoopCommand::ConnectedR(rs,ows) => {
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


      },
      MainLoopCommand::ConnectedW(write_token,ws,ors) => {
        // reg rt
        ors.map(|rs| -> Result<()> {
          let read_token = register_state_r!(self,rs,Some(write_token),SlabEntryState::ReadStream,None);
          // update read reference
          self.slab_cache.get_mut(write_token).map(|r|r.os = Some(read_token));
          Ok(())
        }).unwrap_or(Ok(()))?;
   
        let to_wr = if let Some(&SlabEntry {
           state : SlabEntryState::WriteConnectSynch(..),
           os : _,
           peer : _,
        }) = self.slab_cache.get(write_token) {
           true
        } else {
          warn!("transport connected synchronously with an unknown connection query");
          false
        };
        let oc = if to_wr {
          let ent_p = &mut self.slab_cache.get_mut(write_token).unwrap().state;
          let entry = replace(ent_p,SlabEntryState::Empty);
          if let SlabEntryState::WriteConnectSynch(mut st,trusted_peer) = entry {
            // is connected
            st.2 = true;
            let oc = st.1.recv()?;
            replace(ent_p,SlabEntryState::WriteStream(ws, st));
            if let Some(pr) = trusted_peer {
              if <MC::GlobalServiceCommand as PeerStatusListener<MC::PeerRef>>::DO_LISTEN {
                let listen_global = <MC::GlobalServiceCommand as PeerStatusListener<MC::PeerRef>>::build_command(PeerStatusCommand::PeerOnline(pr.clone(),PeerPriority::Unchecked));
                send_with_handle_panic!(&mut self.global_send,&mut self.global_handle,GlobalCommand::Local(listen_global),"Panic sending to peer online to global service");
              }
            }
            oc
          } else { unreachable!() }
        } else { None };
        if let Some(command) =  oc {
          self.write_stream_send(write_token, command, <MC>::init_write_spawner_out()?, None)?;
        }

      },
      MainLoopCommand::FailConnect(write_token) => {
        debug!("slab cache remove on connect failure ws : {:?}",write_token);
        if let Some(SlabEntry {
          state : _,
          os,
          peer,
        }) = self.slab_cache.remove(write_token) {
          os.map(|s|{
            debug!("slab cache remove fc ors : {:?}",s);
            self.slab_cache.remove(s)
          });
          peer.map(|p|self.peer_cache.remove_val_c(p.borrow().get_key_ref()));
        }
      },
    }

    Ok(())
  }

  pub fn main_loop<S : SpawnerYield>(&mut self,rec : &mut MainLoopRecvIn<MC>, mlsend : &mut <MC::MainLoopChannelOut as SpawnChannel<MainLoopReply<MC>>>::Send, req: MainLoopCommand<MC>, async_yield : &mut S) -> Result<()> {
   let mut events = if self.events.is_some() {
     let oevents = replace(&mut self.events,None);
     oevents.unwrap_or(Events::with_capacity(MC::EVENTS_SIZE))
   } else {
     Events::with_capacity(MC::EVENTS_SIZE)
   };
   self.inner_main_loop(rec, mlsend, req, async_yield, &mut events).map_err(|e|{
     self.events = Some(events);
     e
   })
  }

  fn inner_main_loop<S : SpawnerYield>(&mut self, receiver : &mut MainLoopRecvIn<MC>, mlsend : &mut <MC::MainLoopChannelOut as SpawnChannel<MainLoopReply<MC>>>::Send, req: MainLoopCommand<MC>, async_yield : &mut S, events : &mut Events) -> Result<()> {

    //(self.mainloop_send).send(super::server2::ReadReply::MainLoop(MainLoopCommand::Start))?;
    //(self.mainloop_send).send((MainLoopCommand::Start))?;
    // TODO start other service
    let (smgmt,_rmgmt) = self.peermgmt_channel_in.new()?; // TODO persistence bad for suspend -> put smgmt into self plus add handle to send 

    assert!(true == receiver.register(&self.poll, LOOP_COMMAND, Ready::readable(),
                      PollOpt::edge())?);

    self.call_inner_loop(req,mlsend,async_yield)?;
    loop {
      let cache_l = if MC::AUTH_MODE == ShadowAuthType::NoAuth {
        self.address_cache.len_c()
      } else {
        self.peer_cache.len_c()
      };

      // some bad connection pool management (should use a registered timer for next try)
      if cache_l < self.peer_pool_maintain {
        let now = Instant::now();
        if self.peer_last_query < now {
          let nb_disco = (self.peer_pool_maintain - cache_l) as f64 * (1.0 + MC::PEER_EXTRA_POOL_RATIO);
//            println!("subset from pool timed");
          let kvscom = if MC::AUTH_MODE == ShadowAuthType::NoAuth {
            // peer info is use from kvstore to feed global store if needed
            MainLoopCommand::PeerStore(GlobalCommand::Local(KVStoreCommand::Subset(nb_disco as usize, trusted_peer_ping)))
          } else {
            // send peer to global only after auth and update of peer info
            MainLoopCommand::PeerStore(GlobalCommand::Local(KVStoreCommand::Subset(nb_disco as usize, peer_ping)))
          };
          self.call_inner_loop(kvscom,mlsend,async_yield)?;
        }
        self.peer_last_query = Instant::now() + Duration::from_millis(MC::PEER_POOL_DELAY_MS);
      }
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
              try_breakloop!(self.transport.as_ref().unwrap().accept(), "Transport accept failure : {}", 
              |(rs,ows) : (<MC::Transport as Transport>::ReadStream,Option<<MC::Transport as Transport>::WriteStream>)| -> Result<()> {
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
              Ok(())
            });
          },
          tok => {
            let (sp_read, o_sp_write,os) =  if let Some(ca) = self.slab_cache.get_mut(tok.0 - START_STREAM_IX) {
              let os = ca.os;
              match ca.state {
                SlabEntryState::ReadStream(_,_) => {
                  (true,None,os)
                },
                SlabEntryState::WriteConnectSynch(..) => unreachable!(),
                SlabEntryState::WriteStream(ref mut _ws,(_,ref mut write_r_in,ref mut has_connect)) => {
                  println!("awrite unyield during connect {:?}", tok.0 - START_STREAM_IX);
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
                  println!("aread unyield {:?}", tok.0 - START_STREAM_IX);
                  handle.unyield()?;
                  (false,None,os)
                },
                SlabEntryState::WriteSpawned((ref mut handle,_)) => {
                  println!("awrite unyield {:?}", tok.0 - START_STREAM_IX);
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
    //(self.mainloop_send).send((MainLoopCommand::Start))?;
    //(self.mainloop_send).send(super::server2::ReadReply::MainLoop(MainLoopCommand::Start))?;
              self.start_read_stream_listener(tok.0 - START_STREAM_IX,os,smgmt.clone())?;
            }


          },
        }
      }
    }
 
  }
  fn get_write_handle_send(&self, wtoken : usize) -> Option<WriteHandleSend<MC>> {
    let ow = self.slab_cache.get(wtoken);
    if let Some(ca) = ow {
      if let SlabEntryState::WriteSpawned((ref write_handle,ref write_s_in)) = ca.state {
        write_handle.get_weak_handle().map(|wh|{
          MC::WriteChannelIn::get_weak_send(&write_s_in).map(|aps|
            HandleSend(aps,wh)
          )
        }).unwrap_or(None)
      } else {None}
    } else {None}
  }

  /// The state of the slab entry must be checked before, return error on wrong state
  fn start_read_stream_listener(&mut self, read_token : usize,
    os : Option<usize>,
    smgmt : <MC::PeerMgmtChannelIn as SpawnChannel<PeerMgmtCommand<MC>>>::Send,
                                ) -> Result<()> {
    let owrite_send = match os {
      Some(s) => {
        self.get_write_handle_send(s)
      },
      None => None,
    };

//(self.mainloop_send).send(super::server2::ReadReply::MainLoop(MainLoopCommand::Start))?;
//(self.mainloop_send).send((MainLoopCommand::Start))?;
    let gl = match self.global_handle.get_weak_handle() {
      Some(wh) => 
        MC::GlobalServiceChannelIn::get_weak_send(&self.global_send).map(|aps| HandleSend(aps,wh)),
      None => None,
    };

    let read_out = ReadDest {
      mainloop : self.mainloop_send.clone(),
      peermgmt : smgmt.clone(),
      global : gl,
      write : owrite_send,
      read_token : read_token,
    };

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
          Some(wh) => 
            MC::ApiServiceChannelIn::get_weak_send(&self.api_send).map(|aps|HandleSend(aps,wh)),
          None => None,
        };

        // spawn reader TODO try with None (default to Run)
        let read_handle = self.read_spawn.spawn(ReadService::new(
            read_token,
            rs,
            self.me.clone(),
            with,
            //with.map(|w|w.get_sendable()),
            self.enc_proto.get_new(),
            self.peermgmt_proto.clone(),
            self.local_spawn_proto.clone(),
            self.local_channel_in_proto.clone(),
            read_out.clone(),
            ah,
            self.local_service_proto.clone(),
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


  fn remove_writestream(&mut self, _write_token : usize) -> Result<()> {
    // TODO remove ws and rs (test if rs spawn is finished before : could still be running) if in ws plus update peer cache to remove peer
    panic!("TODO");
  }

  #[inline]
  fn write_stream_send(&mut self, write_token : usize, command : WriteCommand<MC>, write_out : MC::WriteDest, with : Option<MC::PeerRef>) -> Result<()> {
    self.write_stream_send2(write_token, command, write_out, with, true)
  }
  /// The state of the slab entry must be checked before, return error on wrong state
  ///  TODO replace dest with channel spawner mut ref
  ///  TODO write_out as param is probably a mistake
  fn write_stream_send2(&mut self, write_token : usize, command : WriteCommand<MC>, write_out : MC::WriteDest, with : Option<MC::PeerRef>, not_write_once : bool) -> Result<()> {
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
              let write_service = WriteService::new(write_token,ws,self.me.clone(),with, self.enc_proto.get_new(),self.peermgmt_proto.clone(),!not_write_once);
              let write_handle = self.write_spawn.spawn(write_service, write_out.clone(), ocin, write_r_in, MC::SEND_NB_ITER)?;
              let state = SlabEntryState::WriteSpawned((write_handle,write_s_in));
              replace(e,state);
              if MC::AUTH_MODE == ShadowAuthType::NoAuth {
                debug!("cache non auth connected");
              }
              None
            } else {unreachable!()}
          },
          SlabEntryState::WriteConnectSynch((ref mut send,_,_),_) |
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
            let (serv,sen,recv,result) = handle.unwrap_state()?;
            if result.is_ok() {
              // restart
              let write_handle = self.write_spawn.spawn(serv, sen, finished, recv, MC::SEND_NB_ITER)?;
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


#[derive(Clone)]
pub struct AddressCacheEntry {
  read : Option<usize>,
  write : usize,
}
#[derive(Clone)]
pub struct PeerCacheEntry<RP : Clone> {
  /// ref peer
  peer : RP,
  ///  if not initialized CACHE_NO_STREAM, if needed in sub process could switch to arc atomicusize
  read : Option<usize>,
  ///  if not initialized CACHE_NO_STREAM, if 
  write : Option<usize>,
  /// peer priority
  prio : PeerPriority,
}
impl<P,RP : Ref<P> + Clone> GetPeerRef<P,RP> for PeerCacheEntry<RP> {
  fn get_peer_ref(&self) -> (&P,&PeerPriority,Option<usize>) {
    (self.peer.borrow(),&self.prio,self.write.clone())
  }
}
impl<P : Clone> PeerCacheEntry<P> {
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


