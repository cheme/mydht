use peer::{
  Peer,
  PeerPriority,
  PeerMgmtMeths,
};

use rules::DHTRules;
use procs::synch_transport::{
  SynchConnListenerCommandDest,
  SynchConnListener,
  SynchConnListenerCommandIn,
  SynchConnectDest,
  SynchConnect,
  SynchConnectCommandIn,

};
use mydht_base::route2::{
  RouteBase,
  RouteBaseMessage,
  RouteMsgType,
};
use kvstore::{
  KVStore,
};
use procs::storeprop::{
  KVStoreCommand,
  KVStoreReply,
  KVStoreService,
  OptPeerGlobalDest,
};
use procs::api::{
  ApiQueriable,
  ApiQueryId,
  ApiRepliable,
  ApiDest,
  ApiReturn,
};
use keyval::{
  KeyVal,
  GettableAttachments,
  SettableAttachments,
};
use query::cache::QueryCache;
//use self::mesgs::{PeerMgmtMessage,PeerMgmtInitMessage,KVStoreMgmtMessage,QueryMgmtMessage,ClientMessage,ClientMessageIx};
use self::deflocal::{
  LocalReply,
  GlobalCommand,
  GlobalReply,
  LocalDest,
  GlobalDest,
};
use utils::{
  SRef,
  SToRef,
};
use msgenc::{
  MsgEnc,
};
use serde::{Serialize};
use serde::de::{DeserializeOwned};
//use num::traits::ToPrimitive;
use transport::{
  Transport,
  SlabEntry,
};
use kvcache::{
  SlabCache,
  Cache,
};
use self::peermgmt::{
  PeerMgmtCommand,
};
use mydhtresult::{
  Result,
};

pub use procs::api::Api;
pub use mydht_base::procs::*;
use service::{
  HandleSend,
  Service,
  Spawner,
  SpawnSend,
  SpawnHandle,
  SpawnChannel,
  MioChannel,
  MioSend,
  MioRecv,
  NoYield,
  YieldReturn,
  SpawnerYield,
  DefaultRecv,
  NoRecv,
  NoSend,
  send_with_handle,
};

pub use self::mainloop::{
  MainLoopCommand,
  //PeerCacheEntry,
  MDHTState,
  MyDHT,
};
use self::server2::{
  ReadService,
  ReadCommand,
  ReadDest,
};
use self::client2::{
  WriteService,
  WriteCommand,
  WriteReply,
};
use utils::{
  Ref,
};


macro_rules! send_with_handle_panic {
  ($s:expr,$h:expr,$c:expr,$($arg:tt)+) => ({
    if send_with_handle($s,$h,$c)?.is_some() {
      panic!($($arg)+);
    }
  })
}

/*
macro_rules! send_with_handle_log {
  ($s:expr,$h:expr,$c:expr,$lvl:expr,$($arg:tt)+) => ({
    if send_with_handle($s,$h,$c)?.is_some() {
      log!($lvl, $($arg)+)
    }
  })
}
*/


mod mainloop;
pub mod api;
pub mod deflocal;
pub mod storeprop;
mod server2;
pub mod noservice;
mod client2;
mod peermgmt;
mod synch_transport;
//mod server;
//mod client;
//mod peermanager;
//mod kvmanager;
//mod querymanager;


// reexport
pub use self::mainloop::{
  PeerCacheEntry,
};
pub use self::api::{
  ApiCommand,
  ApiReply,
  DHTIn,
};

/// Optional into trait, use for conversion between message (if into return None the message is not
/// send)
pub trait OptInto<T>: Sized {
  fn can_into(&self) -> bool;
  fn opt_into(self) -> Option<T>;
}
pub trait OptIntoRef<'a,T>: Sized {
  fn can_into(&self) -> bool;
  fn opt_into_ref(&'a self) -> Option<T>;
}

pub trait OptFrom<T>: Sized {
  fn can_from(&T) -> bool;
  fn opt_from(T) -> Option<Self>;
}
/// OptFrom implies OptInto
impl<T, U> OptInto<U> for T where U: OptFrom<T>
{
  fn can_into(&self) -> bool {
    U::can_from(self)
  }
  fn opt_into(self) -> Option<U> {
    U::opt_from(self)
  }
}

#[derive(Clone,Eq,PartialEq)]
pub enum ShadowAuthType {
  /// skip ping/pong, dest is unknown : reply as a public server
  NoAuth,
  /// shadow used on ping is from ourself (could be NoShadow or group shared secret)
  Public,
  /// shadow used from dest peer, return failure for a subset of api comment (eg connect with
  /// address only)
  Private,
}

pub struct MyDHTService<MC : MyDHTConf>(pub MC, pub MainLoopRecvIn<MC>, pub MainLoopSendOut<MC>,pub MainLoopSendIn<MC>);

impl<MC : MyDHTConf> SRef for MyDHTService<MC> where
  MC : Send,
  MainLoopSendIn<MC> : Send,
  MainLoopRecvIn<MC> : Send,
  MainLoopSendOut<MC> : Send,
  {
  type Send = MyDHTService<MC>;
  fn get_sendable(self) -> Self::Send {
    self
  }
}

impl<MC : MyDHTConf> SToRef<MyDHTService<MC>> for MyDHTService<MC> where
  MC : Send,
  MainLoopSendIn<MC> : Send,
  MainLoopRecvIn<MC> : Send,
  MainLoopSendOut<MC> : Send,
  {
  fn to_ref(self) -> MyDHTService<MC> {
    self
  }
}



impl<MC : MyDHTConf> Service for MyDHTService<MC> {
  type CommandIn = MainLoopCommand<MC>;
  type CommandOut = MainLoopReply<MC>;

  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, _async_yield : &mut S) -> Result<Self::CommandOut> {
    let mut state = MDHTState::init_state(&mut self.0, &mut self.3)?;
    let mut yield_spawn = NoYield(YieldReturn::Loop);
//    (self.3).send((MainLoopCommand::Start))?;
    //(self.mainloop_send).send(super::server2::ReadReply::MainLoop(MainLoopCommand::Start))?;
    //(state.mainloop_send).send((MainLoopCommand::Start))?;

    let r = state.main_loop(&mut self.1,&mut self.2, req, &mut yield_spawn);
    if r.is_err() {
      panic!("mainloop err : {:?}",&r);
    }
    Ok(MainLoopReply::Ended) 
  }
}


pub type RWSlabEntry<MC : MyDHTConf> = SlabEntry<
  MC::Transport,
  SpawnerRefsDefRecv<ReadService<MC>,ReadCommand, ReadDest<MC>, MC::ReadChannelIn, MC::ReadSpawn>,
  SpawnerRefs<WriteService<MC>,WriteCommand<MC>,MC::WriteDest,MC::WriteChannelIn,MC::WriteSpawn>,
  (<MC::WriteChannelIn as SpawnChannel<WriteCommand<MC>>>::Send,<MC::WriteChannelIn as SpawnChannel<WriteCommand<MC>>>::Recv,bool),
  MC::PeerRef>;

type SpawnerRefs<S : Service,COM, D,CI : SpawnChannel<COM>,SP : Spawner<S,D,CI::Recv>> = (SP::Handle,CI::Send); 
type SpawnerRefsDefRecv<S : Service,COM,D, CI : SpawnChannel<COM>, RS : Spawner<S,D,DefaultRecv<COM,CI::Recv>>> = (RS::Handle,CI::Send);
//type SpawnerRefsRead2<S : Service,D, CI : SpawnChannel<ReadCommand>, RS : Spawner<S,D,DefaultRecv<ReadCommand,CI::Recv>>> = (RS::Handle,CI::Send);

/*pub trait Spawner<
  S : Service,
  D : SpawnSend<<S as Service>::CommandOut>,
  R : SpawnRecv<<S as Service>::CommandIn>> {
*/ 
  //CI::Send); 

pub enum MainLoopReply<MC : MyDHTConf> {
  ServiceReply(MCReply<MC>),
  /// TODO
  Ended,
}

impl<MC : MyDHTConf> Clone for MainLoopReply<MC>
  where MC::GlobalServiceCommand : Clone,
        MC::LocalServiceCommand : Clone,
        MC::GlobalServiceReply : Clone,
        MC::LocalServiceReply : Clone {
  fn clone(&self) -> Self {
    match *self {
      MainLoopReply::ServiceReply(ref rep) =>
        MainLoopReply::ServiceReply(rep.clone()),
      MainLoopReply::Ended =>
        MainLoopReply::Ended,
    }
  }
}

pub enum MainLoopReplySend<MC : MyDHTConf>
  where MC::GlobalServiceCommand : SRef,
        MC::LocalServiceCommand : SRef,
        MC::GlobalServiceReply : SRef,
        MC::LocalServiceReply : SRef {
  ServiceReply(MCReplySend<MC>),
  Ended,
}

impl<MC : MyDHTConf> SRef for MainLoopReply<MC> 
  where MC::GlobalServiceCommand : SRef,
        MC::LocalServiceCommand : SRef,
        MC::GlobalServiceReply : SRef,
        MC::LocalServiceReply : SRef {
  type Send = MainLoopReplySend<MC>;
  fn get_sendable(self) -> Self::Send {
    match self {
      MainLoopReply::ServiceReply(rep) =>
        MainLoopReplySend::ServiceReply(rep.get_sendable()),
      MainLoopReply::Ended =>
        MainLoopReplySend::Ended,
    }
  }
}

impl<MC : MyDHTConf> SToRef<MainLoopReply<MC>> for MainLoopReplySend<MC> 
  where MC::GlobalServiceCommand : SRef,
        MC::LocalServiceCommand : SRef,
        MC::GlobalServiceReply : SRef,
        MC::LocalServiceReply : SRef {
  fn to_ref(self) -> MainLoopReply<MC> {
    match self {
      MainLoopReplySend::ServiceReply(rep) =>
        MainLoopReply::ServiceReply(rep.to_ref()),
      MainLoopReplySend::Ended =>
        MainLoopReply::Ended,
    }
  }
}



/// TODO RouteBase trait
pub trait Route<MC : MyDHTConf> {
  /// if set to true, all function return an expected error and result is receive as a mainloop
  /// command containing the message and tokens
  /// TODO seems useless : route service like peermgmt service not implemented yet + should be in
  /// MDHT trait -> remove for now
  const USE_SERVICE : bool = false;
  /// return an array of write token as dest TODO refactor to return iterator?? (avoid vec aloc,
  /// allow nice implementation for route)
  /// second param usize is targetted nb forward
  /// Mut on peer cache is to allow usage of special peer cache implementing routing (see for
  /// instance trait RouteCacheBase), same for slab
  fn route(&mut self, usize, MCCommand<MC>,&mut MC::Slab, &mut MC::PeerCache) -> Result<(MCCommand<MC>,Vec<usize>)>;
}

/// Route to use if Peer Cache implements RouteBase trait
pub struct PeerCacheRouteBase;
impl<MC : MyDHTConf> Route<MC> for PeerCacheRouteBase
  where 
    MC::LocalServiceCommand : RouteBaseMessage<MC::Peer>,
    MC::GlobalServiceCommand : RouteBaseMessage<MC::Peer>,
    MC::PeerCache : RouteBase<MC::Peer,MC::PeerRef,PeerCacheEntry<MC::PeerRef>>,
{
  const USE_SERVICE : bool = false;
//pub trait RouteBase<P : Peer,GP : GetPeerRef<P>, C : KVCache<<P as KeyVal>::Key,GP>,LOC : RouteBaseMessage<P>> {
  fn route(&mut self, nb : usize, mcc : MCCommand<MC>, _slab : &mut MC::Slab, cache : &mut MC::PeerCache) -> Result<(MCCommand<MC>,Vec<usize>)> {
    Ok(match mcc {
      MCCommand::Local(c) => {
        let (c,r) = cache.route_base(nb,c,RouteMsgType::Local)?;
        (MCCommand::Local(c),r)
      },
      MCCommand::Global(c) => {
        let (c,r) = cache.route_base(nb,c,RouteMsgType::Global)?;
        (MCCommand::Global(c),r)
      },
      MCCommand::PeerStore(c) => {
        let (c,r) = cache.route_base(nb,c,RouteMsgType::PeerStore)?;
        (MCCommand::PeerStore(c),r)
      },
      c @ MCCommand::TryConnect(..) => (c,Vec::new()),
    })
  }
}

pub enum MCCommand<MC : MyDHTConf> {
  Local(MC::LocalServiceCommand),
  Global(MC::GlobalServiceCommand),
  PeerStore(KVStoreCommand<MC::Peer,MC::PeerRef,MC::Peer,MC::PeerRef>),
  /// TODO must add peer key
  TryConnect(<MC::Peer as Peer>::Address,Option<ApiQueryId>),
}






impl<MC : MyDHTConf> MCCommand<MC> {
  pub fn get_api_reply(&self) -> Option<ApiQueryId> {
    match *self {
      MCCommand::Local(ref a) => a.get_api_reply(),
      MCCommand::Global(ref a) => a.get_api_reply(),
      MCCommand::PeerStore(ref a) => a.get_api_reply(),
      MCCommand::TryConnect(_,ref a) => a.clone(),
    }
  }
}
impl<MC : MyDHTConf> ApiQueriable for MCCommand<MC> {
  fn is_api_reply(&self) -> bool {
    match *self {
      MCCommand::Local(ref c) => c.is_api_reply(),
      MCCommand::Global(ref c) => c.is_api_reply(),
      MCCommand::PeerStore(ref c) => c.is_api_reply(),
      MCCommand::TryConnect(_,_) => true,
    }
  }
  fn set_api_reply(&mut self, i : ApiQueryId) {
    match *self {
      MCCommand::Local(ref mut c) => c.set_api_reply(i),
      MCCommand::Global(ref mut c) => c.set_api_reply(i),
      MCCommand::PeerStore(ref mut c) => c.set_api_reply(i),
      MCCommand::TryConnect(_,ref mut oaid) => {
        *oaid = Some(i)
      },
    }
  }
  fn get_api_reply(&self) -> Option<ApiQueryId> {
    match *self {
      MCCommand::Local(ref c) => c.get_api_reply(),
      MCCommand::Global(ref c) => c.get_api_reply(),
      MCCommand::PeerStore(ref c) => c.get_api_reply(),
      MCCommand::TryConnect(_,ref a) => a.clone(),
    }
  }
}


pub enum MCCommandSend<MC : MyDHTConf> 
  where MC::LocalServiceCommand : SRef,
        MC::GlobalServiceCommand : SRef {
  Local(<MC::LocalServiceCommand as SRef>::Send),
  Global(<MC::GlobalServiceCommand as SRef>::Send),
  PeerStore(<KVStoreCommand<MC::Peer,MC::PeerRef,MC::Peer,MC::PeerRef> as SRef>::Send),
  TryConnect(<MC::Peer as Peer>::Address,Option<ApiQueryId>),
}


impl<MC : MyDHTConf> Clone for MCCommand<MC> {
  fn clone(&self) -> Self {
    match *self {
      MCCommand::Local(ref a) => MCCommand::Local(a.clone()),
      MCCommand::Global(ref a) => MCCommand::Global(a.clone()),
      MCCommand::PeerStore(ref a) => MCCommand::PeerStore(a.clone()),
      MCCommand::TryConnect(ref a,ref i) => MCCommand::TryConnect(a.clone(),i.clone()),
    }
  }
}
impl<MC : MyDHTConf> SRef for MCCommand<MC>
  where MC::LocalServiceCommand : SRef,
        MC::GlobalServiceCommand : SRef {
  type Send = MCCommandSend<MC>;
  fn get_sendable(self) -> Self::Send {
    match self {
      MCCommand::Local(a) => MCCommandSend::Local(a.get_sendable()),
      MCCommand::Global(a) => MCCommandSend::Global(a.get_sendable()),
      MCCommand::PeerStore(a) => MCCommandSend::PeerStore(a.get_sendable()),
      MCCommand::TryConnect(a,i) => MCCommandSend::TryConnect(a,i),
    }
  }
}
impl<MC : MyDHTConf> SToRef<MCCommand<MC>> for MCCommandSend<MC>
  where MC::LocalServiceCommand : SRef,
        MC::GlobalServiceCommand : SRef {
  fn to_ref(self) -> MCCommand<MC> {
    match self {
      MCCommandSend::Local(a) => MCCommand::Local(a.to_ref()),
      MCCommandSend::Global(a) => MCCommand::Global(a.to_ref()),
      MCCommandSend::PeerStore(a) => MCCommand::PeerStore(a.to_ref()),
      MCCommandSend::TryConnect(a,i) => MCCommand::TryConnect(a,i),
    }
  }
}


pub enum MCReply<MC : MyDHTConf> {
  Local(MC::LocalServiceReply),
  Global(MC::GlobalServiceReply),
  PeerStore(KVStoreReply<MC::PeerRef>),
  Done(ApiQueryId),
}
impl<MC : MyDHTConf> ApiRepliable for MCReply<MC> {
  fn get_api_reply(&self) -> Option<ApiQueryId> {
    match *self {
      MCReply::Local(ref a) => a.get_api_reply(),
      MCReply::Global(ref a) => a.get_api_reply(),
      MCReply::PeerStore(ref a) => a.get_api_reply(),
      MCReply::Done(ref aid) => Some(aid.clone()),
    }
  }
}


pub enum MCReplySend<MC : MyDHTConf> 
  where MC::LocalServiceReply : SRef,
        MC::GlobalServiceReply : SRef {
  Local(<MC::LocalServiceReply as SRef>::Send),
  Global(<MC::GlobalServiceReply as SRef>::Send),
  PeerStore(<KVStoreReply<MC::PeerRef> as SRef>::Send),
  Done(ApiQueryId),
}

impl<MC : MyDHTConf> Clone for MCReply<MC>
  where MC::LocalServiceReply : Clone,
        MC::GlobalServiceReply : Clone {
  fn clone(&self) -> Self {
    match *self {
      MCReply::Local(ref a) => MCReply::Local(a.clone()),
      MCReply::Global(ref a) => MCReply::Global(a.clone()),
      MCReply::PeerStore(ref a) => MCReply::PeerStore(a.clone()),
      MCReply::Done(ref a) => MCReply::Done(a.clone()),
    }
  }
}

impl<MC : MyDHTConf> SRef for MCReply<MC>
  where MC::LocalServiceReply : SRef,
        MC::GlobalServiceReply : SRef {
  type Send = MCReplySend<MC>;
  fn get_sendable(self) -> Self::Send {
    match self {
      MCReply::Local(a) => MCReplySend::Local(a.get_sendable()),
      MCReply::Global(a) => MCReplySend::Global(a.get_sendable()),
      MCReply::PeerStore(a) => MCReplySend::PeerStore(a.get_sendable()),
      MCReply::Done(a) => MCReplySend::Done(a),
    }
  }
}
impl<MC : MyDHTConf> SToRef<MCReply<MC>> for MCReplySend<MC>
  where MC::LocalServiceReply : SRef,
        MC::GlobalServiceReply : SRef {
  fn to_ref(self) -> MCReply<MC> {
    match self {
      MCReplySend::Local(a) => MCReply::Local(a.to_ref()),
      MCReplySend::Global(a) => MCReply::Global(a.to_ref()),
      MCReplySend::PeerStore(a) => MCReply::PeerStore(a.to_ref()),
      MCReplySend::Done(a) => MCReply::Done(a),
    }
  }
}


pub type PeerRefSend<MC:MyDHTConf> = <MC::PeerRef as SRef>::Send;
//pub type BorRef<
pub trait MyDHTConf : 'static + Send + Sized 
{
//  where <Self::PeerRef as Ref<Self::Peer>>::Send : Borrow<Self::Peer> {

  /// defaults to Public, as the most common use case TODO remove default value??  
  const AUTH_MODE : ShadowAuthType = ShadowAuthType::Public;
  /// Toggle routing of message, if disabled api message are send directly and apireply are send to
  /// mainloop channel out : allowing management of service out of mainloop (listening on receiver
  /// and managing of apiqueryid out of mainloop).
  /// Note that service is started either way so a dummy service, spawner and channels must be use
  /// in mydhtconf even if this const is set to false.
  const USE_API_SERVICE : bool = true;
  /// Name of the main thread
  const LOOP_NAME : &'static str = "MyDHT Main Loop";
  /// number of events to poll (size of mio `Events`)
  const EVENTS_SIZE : usize = 1024;
  /// number of iteration before send loop return, 1 is suggested, but if thread are involve a
  /// little more should be better, in a pool infinite (0) could be fine to.
  const SEND_NB_ITER : usize;
  const GLOBAL_NB_ITER : usize = 0;
  const PEERSTORE_NB_ITER : usize = 0;
  const API_NB_ITER : usize = 0;
  /// number of time we can call for new peer on kvstore, this is not a fine discover mechanism,
  /// some between peer query could be added in the future
  const MAX_NB_DISCOVER_CALL : usize = 2;
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
  type MainLoopChannelOut : SpawnChannel<MainLoopReply<Self>>;
  /// low level transport
  type Transport : Transport;
  /// Message encoding
  type MsgEnc : MsgEnc<Self::Peer, Self::ProtoMsg> + Clone;
  /// Peer struct (with key and address)
  type Peer : Peer<Address = <Self::Transport as Transport>::Address>;
  /// most of the time Arc, if not much threading or smal peer description, RcCloneOnSend can be use, or AllwaysCopy
  /// or Copy.
  type PeerRef : Ref<Self::Peer> + Serialize + DeserializeOwned + Clone;
  /// Peer management methods 
  type PeerMgmtMeths : PeerMgmtMeths<Self::Peer>;
  /// Dynamic rules for the dht
  type DHTRules : DHTRules + Clone;
  /// loop slab implementation
  type Slab : SlabCache<RWSlabEntry<Self>>;
  /// local cache for peer
  type PeerCache : Cache<<Self::Peer as KeyVal>::Key,PeerCacheEntry<Self::PeerRef>>;
  /// local cache for auth challenges
  type ChallengeCache : Cache<Vec<u8>,ChallengeEntry<Self>>;
  
  /// Warning peermgmt service is not implemented, this is simply a placeholder
  type PeerMgmtChannelIn : SpawnChannel<PeerMgmtCommand<Self>>;
  type ReadChannelIn : SpawnChannel<ReadCommand>;
  //type ReadChannelIn : SpawnChannel<<Self::ReadService as Service>::CommandIn>;
//  type ReadFrom : SpawnRecv<<Self::ReadService as Service>::CommandIn>;
  //type ReadDest : SpawnSend<<Self::ReadService as Service>::CommandOut>;
//  type ReadDest : SpawnSend<ReadReply<Self>> + Clone;
  type ReadSpawn : Spawner<
    ReadService<Self>,
    ReadDest<Self>,
    DefaultRecv<ReadCommand,
      <Self::ReadChannelIn as SpawnChannel<ReadCommand>>::Recv>>;

  /// TODO This is currently a dummy dest, a static WriteDest similar to ReadDest must be use instead
  /// Currently mainloop seems to be the only dest for technical message (Nothing clear yet)
  /// TODO replace by NoSend instead (looks like the use case)??
  type WriteDest : SpawnSend<WriteReply<Self>> + Clone;
  type WriteChannelIn : SpawnChannel<WriteCommand<Self>>;
//  type WriteFrom : SpawnRecv<<Self::WriteService as Service>::CommandIn>;
  type WriteSpawn : Spawner<WriteService<Self>,Self::WriteDest,<Self::WriteChannelIn as SpawnChannel<WriteCommand<Self>>>::Recv>;

  type Route : Route<Self>;
  // TODO temp for type check , hardcode it after on transport service (not true for kvstore service) the service contains the read stream!!
//  type ReadService : Service;
  // TODO call to read and write in service will use ReadYield and WriteYield wrappers containing
  // the spawn yield (cf sample implementation in transport tests).
  //type WriteService : Service;



  /// application protomsg used immediatly by local service TODO trait alias
  type ProtoMsg : Into<MCCommand<Self>> + SettableAttachments + GettableAttachments + OptFrom<MCCommand<Self>>;
//  type ProtoMsgSend : GettableAttachments + OptFromRef<'a,MCCommand<Self>>;
  // ProtoMsgSend variant (content not requiring ownership)
//  type ProtoMsgSend<'a> : Into<Self::LocalServiceCommand> + SettableAttachments + GettableAttachments;
  /// global service command : by default it should be protoMsg, depending on spawner use, should
  /// be Send or SRef... Local command require clone (sent to multiple peer)
  type LocalServiceCommand : ApiQueriable + Clone;
  /// OptInto proto for forwarding query to other peers TODO looks useless as service command for
  /// it -> TODO consider removal after impl
  type LocalServiceReply : ApiRepliable;
  /// global service command : by default it should be protoMsg see macro `nolocal`.
  /// For default proxy command, this use a globalCommand struct, the only command to define is
  /// LocalServiceCommand
  /// Need clone to be forward to multiple peers
  /// Opt into store of peer command to route those command if global command allows it
  type GlobalServiceCommand : ApiQueriable + PeerStatusListener<Self::PeerRef>
  //  + OptFrom<KVStoreCommand<Self::Peer,Self::Peer,Self::PeerRef>>
    + Clone;// = GlobalCommand<Self>;
  type GlobalServiceReply : ApiRepliable
  //  + OptFrom<KVStoreReply<Self::PeerRef>> 
    ;// = GlobalCommand<Self>;

  // ref for protomsg : need to be compatible with spawners -> this has been disabled, ref will
  // need to be included in service command (which are
  // type LocalServiceCommandRef : Ref<Self::LocalServiceCommand>;
  // type GlobalServiceCommandRef : Ref<Self::GlobalServiceCommand>;
  /// Main service spawned from ReadService.
  /// In most use case it is a global service that is used, therefore a default implementation ( see macro `nolocal`). is
  /// defined here (proxy command to global service and spawn locally with no suspend strategie).
  /// It can be overriden by a service : usefull for instance in streaming use cases (command being
  /// buf and size read obviously), global service may be disabled in this case but may still have
  /// relevant or accessory use.
  /// Local service is initiated with existing peer comm.
  type LocalService : Service<
    CommandIn = Self::LocalServiceCommand,
    CommandOut = LocalReply<Self>
  >;// = DefLocalService<Self>;

  const LOCAL_SERVICE_NB_ITER : usize;// = 1;
  /// Spawn to local service, run from Read service, it default to local spawn (default local (see
  /// macro `nolocal`))
  /// service proxy command to Global service).
  /// Warn this is clone (spawner will be use for each peer receive and include in many
  /// ReadService)
  type LocalServiceSpawn : Clone + Spawner<
    Self::LocalService,
    LocalDest<Self>,
    <Self::LocalServiceChannelIn as SpawnChannel<Self::LocalServiceCommand>>::Recv
  >;// = Blocker;
  type LocalServiceChannelIn : Clone + SpawnChannel<Self::LocalServiceCommand>;// = NoChannel;
  /* sref of protomessage enough for it
  /// in case of local and global service usage, this type can be used to convert command, it
  /// defaults to an identity converter.
  type LocalGlobalCommandAdapter;*/
  /// Main Service for the application, most of the time it is composed of several service (except
  /// peer management).
  /// Global service is initiated with from peer only, dest peer must be in service command.
  type GlobalService : Service<CommandIn = GlobalCommand<Self::PeerRef,Self::GlobalServiceCommand>, CommandOut = GlobalReply<Self::Peer,Self::PeerRef,Self::GlobalServiceCommand,Self::GlobalServiceReply>>;
  /// GlobalService is spawned from the main loop, and most of the time should use its own thread.
  type GlobalServiceSpawn : Spawner<
    Self::GlobalService,
    GlobalDest<Self>,
    <Self::GlobalServiceChannelIn as SpawnChannel<GlobalCommand<Self::PeerRef,Self::GlobalServiceCommand>>>::Recv
  >;
  type GlobalServiceChannelIn : SpawnChannel<GlobalCommand<Self::PeerRef,Self::GlobalServiceCommand>>;

  type ApiReturn : Clone + Send + ApiReturn<Self>;
  type ApiService : Service<CommandIn = ApiCommand<Self>, CommandOut = ApiReply<Self>>;
  type ApiServiceSpawn : Spawner<
    Self::ApiService,
    ApiDest<Self>,
    <Self::ApiServiceChannelIn as SpawnChannel<ApiCommand<Self>>>::Recv
  >;
  type ApiServiceChannelIn : SpawnChannel<ApiCommand<Self>>;
 
  type PeerStoreQueryCache : QueryCache<Self::Peer,Self::PeerRef,Self::PeerRef>;
  type PeerKVStore : KVStore<Self::Peer>;
  type PeerStoreServiceSpawn : Spawner<
    KVStoreService<Self::Peer,Self::PeerRef,Self::Peer,Self::PeerRef,Self::PeerKVStore,Self::DHTRules,Self::PeerStoreQueryCache>,
    OptPeerGlobalDest<Self>,
    <Self::PeerStoreServiceChannelIn as SpawnChannel<GlobalCommand<Self::PeerRef,KVStoreCommand<Self::Peer,Self::PeerRef,Self::Peer,Self::PeerRef>>>>::Recv
  >;
  type PeerStoreServiceChannelIn : SpawnChannel<GlobalCommand<Self::PeerRef,KVStoreCommand<Self::Peer,Self::PeerRef,Self::Peer,Self::PeerRef>>>;

  type SynchListenerSpawn : Spawner<
    SynchConnListener<Self::Transport>,
    SynchConnListenerCommandDest<Self>,
    DefaultRecv<SynchConnListenerCommandIn, NoRecv>
  >;

  // number of possible simultaneus connections
  const NB_SYNCH_CONNECT : usize;
  // default to infinite as common use case is a pool of parked threads
  const SYNCH_CONNECT_NB_ITER : usize = 0;
  type SynchConnectChannelIn : SpawnChannel<SynchConnectCommandIn<Self::Transport>>;
  type SynchConnectSpawn : Spawner<
    SynchConnect<Self::Transport>,
    SynchConnectDest<Self>,
    <Self::SynchConnectChannelIn as SpawnChannel<SynchConnectCommandIn<Self::Transport>>>::Recv
  >;

  fn init_synch_listener_spawn(&mut self) -> Result<Self::SynchListenerSpawn>;
  fn init_synch_connect_channel_in(&mut self) -> Result<Self::SynchConnectChannelIn>;
  fn init_synch_connect_spawn(&mut self) -> Result<Self::SynchConnectSpawn>;


  /// Start the main loop TODO change sender to avoid mainloop proxies (an API sender like for
  /// others services)
  #[inline]
  fn start_loop(mut self : Self) -> Result<(
    DHTIn<Self>,
    <Self::MainLoopChannelOut as SpawnChannel<MainLoopReply<Self>>>::Recv
    )> {
    println!("Start loop");
    let (s,r) = MioChannel(self.init_main_loop_channel_in()?).new()?;
    let (so,ro) = self.init_main_loop_channel_out()?.new()?;

    let mut sp = self.get_main_spawner()?;
    //let read_handle = self.read_spawn.spawn(ReadService(rs), read_out.clone(), Some(ReadCommand::Run), read_r_in, 0)?;
    let service = MyDHTService(self,r,so,s.clone());
    // the  spawn loop is not use, the poll loop is : here we run a single loop without receive
    sp.spawn(service, NoSend, Some(MainLoopCommand::Start), NoRecv, 1)?; 
    // TODO replace this shit by a spawner then remove constraint on MDht trait where
  /*  ThreadBuilder::new().name(Self::LOOP_NAME.to_string()).spawn(move || {
      let mut state = self.init_state(r)?;
      let mut yield_spawn = NoYield(YieldReturn::Loop);
      let r = state.main_loop(&mut yield_spawn);
      if r.is_err() {
        panic!("mainloop err : {:?}",&r);
      }
      r
    })?;*/
    Ok((DHTIn{
      main_loop : s,
    },ro))
  }

  fn init_peer_kvstore(&mut self) -> Result<Box<Fn() -> Result<Self::PeerKVStore> + Send>>;
  fn init_peer_kvstore_query_cache(&mut self) -> Result<Box<Fn() -> Result<Self::PeerStoreQueryCache> + Send>>;
  fn init_peerstore_channel_in(&mut self) -> Result<Self::PeerStoreServiceChannelIn>;
  fn init_peerstore_spawner(&mut self) -> Result<Self::PeerStoreServiceSpawn>;
  fn do_peer_query_forward_with_discover(&self) -> bool;
  /// create or load peer for our transport (ourselve)
  fn init_ref_peer(&mut self) -> Result<Self::PeerRef>;
  /// for start_loop usage
  fn get_main_spawner(&mut self) -> Result<Self::MainloopSpawn>;
  /// cache initializing for main loop slab cache
  fn init_main_loop_slab_cache(&mut self) -> Result<Self::Slab>;

  /// Peer cache initialization
  fn init_main_loop_peer_cache(&mut self) -> Result<Self::PeerCache>;
  fn init_main_loop_challenge_cache(&mut self) -> Result<Self::ChallengeCache>;

  /// Main loop channel input builder
  fn init_main_loop_channel_in(&mut self) -> Result<Self::MainLoopChannelIn>;
  fn init_main_loop_channel_out(&mut self) -> Result<Self::MainLoopChannelOut>;

  fn init_peermgmt_channel_in(&mut self) -> Result<Self::PeerMgmtChannelIn>;
  /// instantiate read spawner
  fn init_read_spawner(&mut self) -> Result<Self::ReadSpawn>;
  fn init_write_spawner(&mut self) -> Result<Self::WriteSpawn>;
  fn init_global_channel_in(&mut self) -> Result<Self::GlobalServiceChannelIn>;
  fn init_global_spawner(&mut self) -> Result<Self::GlobalServiceSpawn>;
  fn init_global_service(&mut self) -> Result<Self::GlobalService>;

  fn init_local_service(Self::PeerRef, Option<Self::PeerRef>) -> Result<Self::LocalService>;
  fn init_local_spawner(&mut self) -> Result<Self::LocalServiceSpawn>;
  fn init_local_channel_in(&mut self) -> Result<Self::LocalServiceChannelIn>;
  //fn init_read_spawner_out(<Self::MainLoopChannelIn as SpawnChannel<MainLoopCommand<Self>>>::Send, <Self::PeerMgmtChannelIn as SpawnChannel<PeerMgmtCommand<Self>>>::Send) -> Result<ReadDest<Self>>;
  fn init_write_spawner_out() -> Result<Self::WriteDest>;
  fn init_read_channel_in(&mut self) -> Result<Self::ReadChannelIn>;
  fn init_write_channel_in(&mut self) -> Result<Self::WriteChannelIn>;
  fn init_enc_proto(&mut self) -> Result<Self::MsgEnc>;
  fn init_peermgmt_proto(&mut self) -> Result<Self::PeerMgmtMeths>;
  fn init_dhtrules_proto(&mut self) -> Result<Self::DHTRules>;
  /// Transport initialization
  fn init_transport(&mut self) -> Result<Self::Transport>;
  fn init_route(&mut self) -> Result<Self::Route>;

  fn init_api_channel_in(&mut self) -> Result<Self::ApiServiceChannelIn>;
  fn init_api_spawner(&mut self) -> Result<Self::ApiServiceSpawn>;
  fn init_api_service(&mut self) -> Result<Self::ApiService>;
 

}

/// trait for global message to keep reference to read peer
pub trait GetOrigin<MC : MyDHTConf> {
  fn get_origin(&self) -> Option<&MC::PeerRef>;
}

/// entry cache for challenge
pub struct ChallengeEntry<MC : MyDHTConf> {
//  pub challenge : Vec<u8>,
  pub write_tok : usize,
  /// TODO check if use, might be useless
  pub read_tok : Option<usize>,
  pub next_msg : Option<WriteCommand<MC>>,
  /// use to send an error reply to api on auth fail
  pub next_qid : Option<ApiQueryId>,
  pub api_qid : Option<ApiQueryId>,
}


 
#[derive(Clone,Debug)]
pub struct FWConf {
  /// only use for forward local to get nb of local to try
  pub nb_for : usize,
  /// do we run discovering if not enough peers
  pub discover : bool,
}


/// trait for message allowing synchronisation with online peer
/// Main use case is global service listening to peer updates (peer online and peer offline mainly)
pub trait PeerStatusListener<P> : Sized {
  const DO_LISTEN : bool;
  /// if return false it means that no status update should be send
  /// Should use unreachable if DO_LISTEN is false
  fn build_command(PeerStatusCommand<P>) -> Self;
}

#[derive(Clone)]
pub enum PeerStatusCommand<P> {
  PeerOnline(P,PeerPriority),
  PeerOffline(P,PeerPriority),
}
pub enum PeerStatusCommandSend<P : SRef> {
  PeerOnline(P::Send,PeerPriority),
  PeerOffline(P::Send,PeerPriority),
}


impl<P : SRef> SRef for PeerStatusCommand<P> {
  type Send = PeerStatusCommandSend<P>;
  fn get_sendable(self) -> Self::Send {
    match self {
      PeerStatusCommand::PeerOnline(p,pp) =>
        PeerStatusCommandSend::PeerOnline(p.get_sendable(),pp),
      PeerStatusCommand::PeerOffline(p,pp) =>
        PeerStatusCommandSend::PeerOffline(p.get_sendable(),pp),
    }
  }
}

impl<P : SRef> SToRef<PeerStatusCommand<P>> for PeerStatusCommandSend<P> {
  fn to_ref(self) -> PeerStatusCommand<P> {
    match self {
      PeerStatusCommandSend::PeerOnline(p,pp) =>
        PeerStatusCommand::PeerOnline(p.to_ref(),pp),
      PeerStatusCommandSend::PeerOffline(p,pp) =>
        PeerStatusCommand::PeerOffline(p.to_ref(),pp),
    }
  }
}


pub type LocalRecvIn<MC : MyDHTConf> = <MC::LocalServiceChannelIn as SpawnChannel<MC::LocalServiceCommand>>::Recv;
pub type LocalSendIn<MC : MyDHTConf> = <MC::LocalServiceChannelIn as SpawnChannel<MC::LocalServiceCommand>>::Send;
pub type LocalHandle<MC : MyDHTConf> = <MC::LocalServiceSpawn as Spawner<MC::LocalService,LocalDest<MC>,LocalRecvIn<MC>>>::Handle;


pub type GlobalSendIn<MC : MyDHTConf> = <MC::GlobalServiceChannelIn as SpawnChannel<GlobalCommand<MC::PeerRef,MC::GlobalServiceCommand>>>::Send;
pub type GlobalRecvIn<MC : MyDHTConf> = <MC::GlobalServiceChannelIn as SpawnChannel<GlobalCommand<MC::PeerRef,MC::GlobalServiceCommand>>>::Recv;
pub type GlobalHandle<MC : MyDHTConf> = <MC::GlobalServiceSpawn as Spawner<MC::GlobalService,GlobalDest<MC>,GlobalRecvIn<MC>>>::Handle;
pub type GlobalWeakSend<MC : MyDHTConf> = <MC::GlobalServiceChannelIn as SpawnChannel<GlobalCommand<MC::PeerRef,MC::GlobalServiceCommand>>>::WeakSend;
pub type GlobalWeakHandle<MC : MyDHTConf> = <GlobalHandle<MC> as SpawnHandle<MC::GlobalService,GlobalDest<MC>, GlobalRecvIn<MC>>>::WeakHandle;
pub type GlobalHandleSend<MC : MyDHTConf> = HandleSend<GlobalWeakSend<MC>,GlobalWeakHandle<MC>>;

pub type ApiSendIn<MC : MyDHTConf> = <MC::ApiServiceChannelIn as SpawnChannel<ApiCommand<MC>>>::Send;
pub type ApiRecvIn<MC : MyDHTConf> = <MC::ApiServiceChannelIn as SpawnChannel<ApiCommand<MC>>>::Recv;
pub type ApiHandle<MC : MyDHTConf> = <MC::ApiServiceSpawn as Spawner<MC::ApiService,ApiDest<MC>,ApiRecvIn<MC>>>::Handle;
pub type ApiWeakHandle<MC : MyDHTConf> = <ApiHandle<MC> as SpawnHandle<MC::ApiService,ApiDest<MC>,ApiRecvIn<MC>>>::WeakHandle;
pub type ApiWeakSend<MC: MyDHTConf> = <MC::ApiServiceChannelIn as SpawnChannel<ApiCommand<MC>>>::WeakSend;
pub type ApiHandleSend<MC : MyDHTConf> = HandleSend<ApiWeakSend<MC>,ApiWeakHandle<MC>>;



pub type WriteSendIn<MC : MyDHTConf> = <MC::WriteChannelIn as SpawnChannel<WriteCommand<MC>>>::Send;
pub type WriteRecvIn<MC : MyDHTConf> = <MC::WriteChannelIn as SpawnChannel<WriteCommand<MC>>>::Recv;
pub type WriteHandle<MC : MyDHTConf> = <MC::WriteSpawn as Spawner<WriteService<MC>,MC::WriteDest,WriteRecvIn<MC>>>::Handle;
pub type WriteWeakHandle<MC : MyDHTConf> = <WriteHandle<MC> as SpawnHandle<WriteService<MC>,MC::WriteDest,WriteRecvIn<MC>>>::WeakHandle;
pub type WriteWeakSend<MC : MyDHTConf> = <MC::WriteChannelIn as SpawnChannel<WriteCommand<MC>>>::WeakSend;
pub type WriteHandleSend<MC : MyDHTConf> = HandleSend<WriteWeakSend<MC>,WriteWeakHandle<MC>>;


pub type PeerStoreHandle<MC : MyDHTConf> = <MC::PeerStoreServiceSpawn as 
Spawner<
    KVStoreService<MC::Peer,MC::PeerRef,MC::Peer,MC::PeerRef,MC::PeerKVStore,MC::DHTRules,MC::PeerStoreQueryCache>,
    OptPeerGlobalDest<MC>,
    <MC::PeerStoreServiceChannelIn as SpawnChannel<GlobalCommand<MC::PeerRef,KVStoreCommand<MC::Peer,MC::PeerRef,MC::Peer,MC::PeerRef>>>>::Recv
  >
>::Handle;

pub type SynchConnectHandle<MC : MyDHTConf> = <MC::SynchConnectSpawn as 
 Spawner<
    SynchConnect<MC::Transport>,
    SynchConnectDest<MC>,
    <MC::SynchConnectChannelIn as SpawnChannel<SynchConnectCommandIn<MC::Transport>>>::Recv
  >>::Handle;



type MainLoopRecvIn<MC : MyDHTConf> = MioRecv<<MC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MC>>>::Recv>;
type MainLoopSendIn<MC : MyDHTConf> = MioSend<<MC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MC>>>::Send>;
type MainLoopSendOut<MC : MyDHTConf> = <MC::MainLoopChannelOut as SpawnChannel<MainLoopReply<MC>>>::Send;
