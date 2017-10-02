use std::sync::mpsc::{Sender};
use std::borrow::Borrow;
use peer::{PeerMgmtMeths};
use query::{self,QueryConf,QueryPriority,QueryMode,QueryModeMsg,LastSent,QueryMsg};
use rules::DHTRules;
use kvstore::{
  StoragePriority, 
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
use self::mesgs::{PeerMgmtMessage,KVStoreMgmtMessage,ClientMessage,ClientMessageIx};
//use self::mesgs::{PeerMgmtMessage,PeerMgmtInitMessage,KVStoreMgmtMessage,QueryMgmtMessage,ClientMessage,ClientMessageIx};
use std::sync::{Arc,Condvar,Mutex};
use std::sync::mpsc::channel;
use std::thread;
use peer::Peer;
use time::Duration;
use self::deflocal::{
  LocalReply,
  DefLocalService,
  GlobalCommand,
  GlobalReply,
  LocalDest,
  GlobalDest,
};
use utils::{
  self,
  SRef,
};
use msgenc::{
  MsgEnc,
};
use serde::{Serializer,Serialize,Deserialize,Deserializer};
use serde::de::{DeserializeOwned};
use std::result::Result as StdResult;
//use num::traits::ToPrimitive;
use std::marker::PhantomData;
use std::fmt::{Debug,Display};
use transport::{
  Transport,
  Address,
  Address as TransportAddress,
  SlabEntry,
  SlabEntryState,
  Registerable,
  WriteTransportStream,
};
use kvcache::{
  SlabCache,
  KVCache,
};
use self::peermgmt::{
  PeerMgmtCommand,
};
use mydhtresult::{
  Result,
  Error,
  ErrorKind,
  ErrorLevel as MdhtErrorLevel,
};

pub use mydht_base::procs::*;
use service::{
  HandleSend,
  Service,
  Spawner,
  Blocker,
  NoChannel,
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
pub use self::mainloop::{
  MainLoopCommand,
  //PeerCacheEntry,
  MDHTState,
};
use self::server2::{
  ReadService,
  ReadCommand,
  ReadReply,
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

/// return the command if handle is finished
#[inline]
pub fn send_with_handle<X,Y,Z, S : SpawnSend<C>, H : SpawnHandle<X,Y,Z>, C> (s : &mut S, h : &mut H, command : C) -> Result<Option<C>> {
//Service,Sen,Recv
  Ok(if h.is_finished() {
    Some(command)
  } else {
    s.send(command)?;
    h.unyield()?;
    None
  })
}


macro_rules! send_with_handle_panic {
  ($s:expr,$h:expr,$c:expr,$($arg:tt)+) => ({
    if send_with_handle($s,$h,$c)?.is_some() {
      panic!($($arg)+);
    }
  })
}

macro_rules! send_with_handle_log {
  ($s:expr,$h:expr,$c:expr,$lvl:expr,$($arg:tt)+) => ({
    if send_with_handle($s,$h,$c)?.is_some() {
      log!($lvl, $($arg)+)
    }
  })
}



pub mod mesgs;

mod mainloop;
pub mod api;
pub mod deflocal;
pub mod storeprop;
mod server2;
mod client2;
mod peermgmt;
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
  ApiSendIn,
};

/// Optional into trait, use for conversion between message (if into return None the message is not
/// send)
pub trait OptInto<T>: Sized {
  fn can_into(&self) -> bool;
  fn opt_into(self) -> Option<T>;
}
pub trait OptFrom<T>: Sized {
  fn can_from(&T) -> bool;
  fn opt_from(T) -> Option<Self>;
}


pub struct MyDHTService<MC : MyDHTConf>(pub MC, pub MioRecv<<MC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MC>>>::Recv>, pub <MC::MainLoopChannelOut as SpawnChannel<MainLoopReply>>::Send,pub MioSend<<MC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MC>>>::Send>);

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

impl<MC : MyDHTConf> Service for MyDHTService<MC> {
  type CommandIn = MainLoopCommand<MC>;
  type CommandOut = MainLoopReply;

  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, async_yield : &mut S) -> Result<Self::CommandOut> {
    let mut state = MDHTState::init_state(&mut self.0, &mut self.3)?;
    let mut yield_spawn = NoYield(YieldReturn::Loop);
//    (self.3).send((MainLoopCommand::Start))?;
    //(self.mainloop_send).send(super::server2::ReadReply::MainLoop(MainLoopCommand::Start))?;
    //(state.mainloop_send).send((MainLoopCommand::Start))?;

    let r = state.main_loop(&mut self.1, req, &mut yield_spawn);
    if r.is_err() {
      panic!("mainloop err : {:?}",&r);
    }
    Ok(MainLoopReply::Ended) 
  }
}


pub type RWSlabEntry<MC : MyDHTConf> = SlabEntry<
  MC::Transport,
//  (),
  //SpawnerRefsRead2<ReadService<MC>, MC::ReadDest, MC::ReadChannelIn, MC::ReadSpawn>,
  SpawnerRefsDefRecv2<ReadService<MC>,ReadCommand, ReadDest<MC>, MC::ReadChannelIn, MC::ReadSpawn>,
//  SpawnerRefs<ReadService<MC>,MC::ReadDest,DefaultRecvChannel<ReadCommand,MC::ReadChannelIn>,MC::ReadSpawn>,
  SpawnerRefs2<WriteService<MC>,WriteCommand<MC>,MC::WriteDest,MC::WriteChannelIn,MC::WriteSpawn>,
  // bool is has_connect
  (<MC::WriteChannelIn as SpawnChannel<WriteCommand<MC>>>::Send,<MC::WriteChannelIn as SpawnChannel<WriteCommand<MC>>>::Recv,bool),
  MC::PeerRef>;

type SpawnerRefs<S : Service,D,CI : SpawnChannel<S::CommandIn>,SP : Spawner<S,D,CI::Recv>> = (SP::Handle,CI::Send); 
type SpawnerRefs2<S : Service,COM_IN, D,CI : SpawnChannel<COM_IN>,SP : Spawner<S,D,CI::Recv>> = (SP::Handle,CI::Send); 
type SpawnerRefsDefRecv2<S : Service,COM_IN,D, CI : SpawnChannel<COM_IN>, RS : Spawner<S,D,DefaultRecv<COM_IN,CI::Recv>>> = (RS::Handle,CI::Send);
type SpawnerRefsRead2<S : Service,D, CI : SpawnChannel<ReadCommand>, RS : Spawner<S,D,DefaultRecv<ReadCommand,CI::Recv>>> = (RS::Handle,CI::Send);

/*pub trait Spawner<
  S : Service,
  D : SpawnSend<<S as Service>::CommandOut>,
  R : SpawnRecv<<S as Service>::CommandIn>> {
*/ 
  //CI::Send); 
type MainLoopRecvIn<MC : MyDHTConf> = MioRecv<<MC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MC>>>::Recv>;
type MainLoopSendIn<MC : MyDHTConf> = MioSend<<MC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MC>>>::Send>;

pub enum MainLoopReply {
  /// TODO
  Ended,
}

pub trait Route<MC : MyDHTConf> {
  /// if set to true, all function return an expected error and result is receive as a mainloop
  /// command containing the message and tokens
  const USE_SERVICE : bool = false;
  /// return an array of write token as dest TODO refactor to return iterator?? (avoid vec aloc,
  /// allow nice implementation for route)
  /// second param usize is targetted nb forward
  fn route(&mut self, usize, MC::LocalServiceCommand,&MC::Slab, &MC::PeerCache) -> Result<(MC::LocalServiceCommand,Vec<usize>)>;
  fn route_global(&mut self, usize, MC::GlobalServiceCommand,&MC::Slab, &MC::PeerCache) -> Result<(MC::GlobalServiceCommand,Vec<usize>)>;
}

pub type PeerRefSend<MC:MyDHTConf> = <MC::PeerRef as SRef>::Send;
//pub type BorRef<
pub trait MyDHTConf : 'static + Send + Sized 
{
//  where <Self::PeerRef as Ref<Self::Peer>>::Send : Borrow<Self::Peer> {

  /// defaults to Public, as the most common use case TODO remove default value??  
  const AUTH_MODE : ShadowAuthType = ShadowAuthType::Public;
  /// Name of the main thread
  const loop_name : &'static str = "MyDHT Main Loop";
  /// number of events to poll (size of mio `Events`)
  const events_size : usize = 1024;
  /// number of iteration before send loop return, 1 is suggested, but if thread are involve a
  /// little more should be better, in a pool infinite (0) could be fine to.
  const send_nb_iter : usize;
  const GLOBAL_NB_ITER : usize = 0;
  const PEERSTORE_NB_ITER : usize = 0;
  const API_NB_ITER : usize = 0;
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
  type MsgEnc : MsgEnc<Self::Peer, Self::ProtoMsg> + Clone;
  /// Peer struct (with key and address)
  type Peer : Peer<Address = <Self::Transport as Transport>::Address>;
  /// most of the time Arc, if not much threading or smal peer description, RcCloneOnSend can be use, or AllwaysCopy
  /// or Copy.
  type PeerRef : Ref<Self::Peer>;
  /// Peer management methods 
  type PeerMgmtMeths : PeerMgmtMeths<Self::Peer> + Clone;
  /// Dynamic rules for the dht
  type DHTRules : DHTRules + Clone;
  /// loop slab implementation
  type Slab : SlabCache<RWSlabEntry<Self>>;
  /// local cache for peer
  type PeerCache : KVCache<<Self::Peer as KeyVal>::Key,PeerCacheEntry<Self::PeerRef>>;
  /// local cache for auth challenges
  type ChallengeCache : KVCache<Vec<u8>,ChallengeEntry<Self>>;
  
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



  /// application protomsg used immediatly by local service
  type ProtoMsg : Into<Self::LocalServiceCommand> + SettableAttachments + GettableAttachments;
  // ProtoMsgSend variant (content not requiring ownership)
//  type ProtoMsgSend<'a> : Into<Self::LocalServiceCommand> + SettableAttachments + GettableAttachments;
  /// global service command : by default it should be protoMsg, depending on spawner use, should
  /// be Send or SRef... Local command require clone (sent to multiple peer)
  type LocalServiceCommand : ApiQueriable + OptInto<Self::ProtoMsg> + Clone;
  /// OptInto proto for forwarding query to other peers TODO looks useless as service command for
  /// it -> TODO consider removal after impl
  type LocalServiceReply : ApiRepliable;
  /// global service command : by default it should be protoMsg see macro `nolocal`.
  /// For default proxy command, this use a globalCommand struct, the only command to define is
  /// LocalServiceCommand
  /// Need clone to be forward to multiple peers
  /// Opt into store of peer command to route those command if global command allows it
  type GlobalServiceCommand : ApiQueriable + OptInto<Self::ProtoMsg>
    + OptFrom<KVStoreCommand<Self::Peer,Self::Peer,Self::PeerRef>>
    + OptInto<KVStoreCommand<Self::Peer,Self::Peer,Self::PeerRef>>
    + Clone;// = GlobalCommand<Self>;
  type GlobalServiceReply : ApiRepliable
    + OptFrom<KVStoreReply<Self::PeerRef>> 
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
    <Self::PeerStoreServiceChannelIn as SpawnChannel<GlobalCommand<Self::PeerRef,KVStoreCommand<Self::Peer,Self::Peer,Self::PeerRef>>>>::Recv
  >;
  type PeerStoreServiceChannelIn : SpawnChannel<GlobalCommand<Self::PeerRef,KVStoreCommand<Self::Peer,Self::Peer,Self::PeerRef>>>;


  /// Start the main loop TODO change sender to avoid mainloop proxies (an API sender like for
  /// others services)
  #[inline]
  fn start_loop(mut self : Self) -> Result<(
    ApiSendIn<Self>,
//    MioSend<<Self::MainLoopChannelIn as SpawnChannel<MainLoopCommand<Self>>>::Send>, 
    <Self::MainLoopChannelOut as SpawnChannel<MainLoopReply>>::Recv
    )> {
    let (s,r) = MioChannel(self.init_main_loop_channel_in()?).new()?;
    let (so,ro) = self.init_main_loop_channel_out()?.new()?;

    let mut sp = self.get_main_spawner()?;
    //let read_handle = self.read_spawn.spawn(ReadService(rs), read_out.clone(), Some(ReadCommand::Run), read_r_in, 0)?;
    let service = MyDHTService(self,r,so,s.clone());
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
    Ok((ApiSendIn{
      main_loop : s,
    },ro))
  }

  fn init_peer_kvstore(&mut self) -> Result<Box<Fn() -> Result<Self::PeerKVStore> + Send>>;
  fn init_peer_kvstore_query_cache(&mut self) -> Result<Self::PeerStoreQueryCache>;
  fn init_peerstore_channel_in(&mut self) -> Result<Self::PeerStoreServiceChannelIn>;
  fn init_peerstore_spawner(&mut self) -> Result<Self::PeerStoreServiceSpawn>;
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
}

/// utility trait to avoid lot of parameters in each struct / fn
/// kinda aliasing
pub trait RunningTypes : Send + Sync + 'static
{
  type A : Address;
  type P : Peer<Address = Self::A>;
  type V : KeyVal;
  type M : PeerMgmtMeths<Self::P>;
  type R : DHTRules;
  type E : MsgEnc<Self::P,Self::V>;
  type T : Transport<Address = Self::A>;
}
/*pub trait RunningTypes : Send + Sync + 'static 
 where 
 <Self::V as KeyVal>::Key : 'static,
 <Self::P as KeyVal>::Key : 'static,
 <Self::T as Transport>::ReadStream : 'static,
 <Self::T as Transport>::WriteStream : 'static,
{
  type A : 'static + Address;
  type P : 'static + Peer<Address = Self::A>;
  type V : 'static + KeyVal;
  type M : 'static + PeerMgmtMeths<Self::P, Self::V>;
  type R : 'static + DHTRules;
  type E : 'static + MsgEnc;
  type T : 'static + Transport<Address = Self::A>;
}*/

/// TODO replace client channel by that
/// Reference used to send Client Message.
/// This could be used by any process to keep trace of a client process : 
/// for instance from server client direct client query (without peermgmt)
pub enum ClientHandle<P : Peer, V : KeyVal, W : WriteTransportStream> {

  /// Uninitialised client or no handle from peermgmt
  /// First message is querying a handle from peermgmt (through OneResult), when in server.
  /// When in query no new handle are needed but next query created from server will certainly use
  /// a faster handle.
  /// - optional WriteStream, this is only in server context when transport has instantiate write
  /// stream on connect
  /// Result in communication with PeerMgmt Process
  NoHandle,

  /// no new thread, the function is called, for DHT where there is not much client treatment
  /// (from peermanager thread). TODO plus all fields
  /// Result in communication with peermgmtprocess
  Local,


  /// thread do multiplexe some client : usize is client ix
  //ThreadedMult(Sender<ClientMessageIx<P,V>>, usize),

  /// WriteTransportStream is runing (loop) in its own thread
  Threaded(Sender<ClientMessageIx<P,V,W>>,usize),
}

impl<P : Peer, V : KeyVal, W : WriteTransportStream> ClientHandle<P,V,W> {
  
  /// send message with the handle if the handle allows it otherwhise return false.
  /// If an error is returned consider either the handle died (ask for new) or something else. 
  pub fn send_msg(&self, mess : ClientMessage<P,V,W>) -> Result<bool> {
    match self {
      &ClientHandle::Threaded(ref clisend, ref ix) => {
        try!(clisend.send((mess,*ix)));
        Ok(true)
      },
      _ => Ok(false),
    }
    
  }


}

#[cfg(not(feature="mio-impl"))]
#[derive(Debug,PartialEq,Eq)]
pub enum ServerMode {
  /// server code is run once on reception, this is for transport that do not need to block on each
  /// message reception : for instance tcp event loop or udp transport.
  /// In this case transport is run for one message only (no loop)
  Local(bool),
  /// Thread recycle local execution (got all stream and receive message to start a read).
  /// Read must be considered Nonblocking.
  /// This is mainly implemented in the handler which will not spawn, otherwhise it is similar to
  /// Local(true)
  /// - max nb of stream to read
  LocalMax(usize),
  /// Same as LocalMax but
  /// - number of thread to share
  LocalPool(usize),
  /// Transport Stream (generally blocking) is read in its own loop in its own Thread.
  /// - optional duration is a timeout to end from peermanager (cleanup), normally transport should
  /// implement a reception timeout for this and this option should be useless in most cases. TODO unimplemented
  ThreadedOne(Option<Duration>),
  /// alternatively receive from blocking thread with a short timeout (timeout will does not mean
  /// thread is off). This is not really good, it will only work if transport read timeout is
  /// short.
  /// - max nb of stream to read
  /// - number of short timeout to consider peer offlint
  ThreadedMax(usize,usize),
  /// same as TreadedMax but in pool mode
  /// - number of thread to share
  /// - number of short timeout to consider peer offlint
  ThreadedPool(usize,usize),
}
 

#[cfg(feature="mio-impl")]
#[derive(Debug,PartialEq,Eq)]
pub enum ServerMode {
  /// start process in a coroutine
  Coroutine,
  /// server code is run once on reception, this is for transport that do not need to block on each
  /// message reception : for instance tcp event loop or udp transport.
  /// In this case transport is run for one message only (no loop)
  Local(bool),
  /// Thread recycle local execution (got all stream and receive message to start a read).
  /// Read must be considered Nonblocking.
  /// This is mainly implemented in the handler which will not spawn, otherwhise it is similar to
  /// Local(true)
  /// - max nb of stream to read
  LocalMax(usize),
  /// Same as LocalMax but
  /// - number of thread to share
  LocalPool(usize),
  /// Transport Stream (generally blocking) is read in its own loop in its own Thread.
  /// - optional duration is a timeout to end from peermanager (cleanup), normally transport should
  /// implement a reception timeout for this and this option should be useless in most cases. TODO unimplemented
  ThreadedOne(Option<Duration>),
  /// alternatively receive from blocking thread with a short timeout (timeout will does not mean
  /// thread is off). This is not really good, it will only work if transport read timeout is
  /// short.
  /// - max nb of stream to read
  /// - number of short timeout to consider peer offlint
  ThreadedMax(usize,usize),
  /// same as TreadedMax but in pool mode
  /// - number of thread to share
  /// - number of short timeout to consider peer offlint
  ThreadedPool(usize,usize),
}
 
/// Could be use to define the final type of a DHT, most of the time we create a new object (see
/// example/fs.rs).
/// This kind of struct is never use, it is just to a type instead of a
/// trait in generic parameters.
pub struct RunningTypesImpl<
  A : Address,
  P : Peer<Address = A>,
  V : KeyVal,
  M : PeerMgmtMeths<P>, 
  R : DHTRules,
  E : MsgEnc<P,V>, 
  T : Transport<Address = A>>
  (PhantomData<(A,R,P,V,M,T,E)>);
/*
impl<
  A : 'static + Address,
  P : 'static + Peer<Address = A>,
  V : 'static + KeyVal,
  M : 'static + PeerMgmtMeths<P, V>, 
  R : 'static + DHTRules,
  E : 'static + MsgEnc, 
  T : 'static + Transport<Address = A>>
     RunningTypes for RunningTypesImpl<A, P, V, M, R, E, T> 
      where 
 <V as KeyVal>::Key : 'static,
 <P as KeyVal>::Key : 'static,
 <T as Transport>::ReadStream : 'static,
 <T as Transport>::WriteStream : 'static,
     {
*/
impl<
  A : Address,
  P : Peer<Address = A>,
  V : KeyVal,
  M : PeerMgmtMeths<P>, 
  R : DHTRules,
  E : MsgEnc<P,V>, 
  T : Transport<Address = A>>
     RunningTypes for RunningTypesImpl<A, P, V, M, R, E, T> 
     {
  type A = A;
  type P = P;
  type V = V;
  type M = M;
  type R = R;
  type E = E;
  type T = T;
}

pub type ClientChanel<P, V, W> = Sender<mesgs::ClientMessageIx<P,V,W>>;

/// Running context contain all information needed, mainly configuration and calculation rules.
pub struct RunningContext<RT : RunningTypes> {
  pub me : Arc<RT::P>,
  pub peerrules : RT::M,
  pub rules : RT::R, // Only one that can switch to trait object : No for homogeneity
  pub msgenc : RT::E,
  pub transport : RT::T, 
  pub keyval : PhantomData<RT::V>,
  pub rtype : PhantomData<RT>,
}

impl<RT : RunningTypes> RunningContext<RT> {
  pub fn new (
  me : Arc<RT::P>,
  peerrules : RT::M,
  rules : RT::R,
  msgenc : RT::E,
  transport : RT::T, 
  ) -> RunningContext<RT> {
    RunningContext {
      me : me,
      peerrules : peerrules,
      rules : rules,
      msgenc : msgenc,
      transport : transport,
      keyval : PhantomData,
      rtype : PhantomData,
    }

  }

}

/// There is a need for RunningContext content to be sync (we share context in an Arc (preventing us from
/// cloning its content and therefore requiring sync to be use in multiple thread).
pub type ArcRunningContext<RT> = Arc<RunningContext<RT>>;
//pub type ArcRunningContext<RT : RunningTypes> = Arc<RunningContext<RT>>;

/// Channel used by several process, they are cloned/moved when send to new thread (sender are not
/// sync)
pub struct RunningProcesses<RT : RunningTypes> {
  peers : Sender<mesgs::PeerMgmtMessage<RT::P,RT::V,<RT::T as Transport>::ReadStream,<RT::T as Transport>::WriteStream>>,
//  queries : Sender<QueryMgmtMessage<RT::P,RT::V>>,
  store : Sender<mesgs::KVStoreMgmtMessage<RT::P,RT::V>>,
}

// deriving seems ko for now TODO test with derive again
impl<RT : RunningTypes> Clone for RunningProcesses<RT> {
  fn clone(&self) ->  RunningProcesses<RT> {
    RunningProcesses {
      peers : self.peers.clone(),
      //queries : self.queries.clone(),
      store : self.store.clone(),
    }
  }
}
 

/// DHT infos
pub struct DHT<RT : RunningTypes> {
  rp : RunningProcesses<RT>,
  rc : ArcRunningContext<RT>, 
  f : Arc<(Condvar,Mutex<isize>)>
}


/// Find a value by key. Specifying our queryconf, and priorities.
pub fn find_local_val<RT : RunningTypes> (rp : &RunningProcesses<RT>, _ : &ArcRunningContext<RT>, nid : <RT::V as KeyVal>::Key ) -> Option<RT::V> {
  debug!("Finding KeyVal locally {:?}", nid);
  let sync = utils::new_oneresult(None);
  // local query replyto set to None
  if rp.store.send(KVStoreMgmtMessage::KVFindLocally(nid, Some(sync.clone()))).is_err() {
    error!("chann error in find local val");
    return None
  };

  // block until result
  utils::clone_wait_one_result(&sync, None).unwrap_or(None)
}

/// Store a value. Specifying our queryconf, and priorities. Note that priority rules are very
/// important to know if we do propagate value or store local only or cache local only.
pub fn store_val<RT : RunningTypes> (rp : &RunningProcesses<RT>, rc : &ArcRunningContext<RT>, val : RT::V, qconf : &QueryConf, prio : QueryPriority, sprio : StoragePriority) -> bool {
  panic!("TODO delete it");
/* 
  let msgqmode;    init_qmode(rc.me, &qconf.mode);
  let lastsent = qconf.hop_hist.map(|(n,ishop)| if ishop 
    {LastSent::LastSentHop(n,vec![rc.me.get_key()].into_iter().collect())}
    else
    {LastSent::LastSentPeer(n,vec![rc.me.get_key()].into_iter().collect())}
  );
  let maxhop = rc.rules.nbhop(prio);
  let nbquer = rc.rules.nbquery(prio);
  let queryconf = QueryMsg {
    modeinfo : msgqmode, 
//    chunk : qconf.chunk.clone(), 
    hop_hist : lastsent, 
    storage : sprio,
    rem_hop : maxhop,
    nb_forw : nbquer,
    prio : prio,
    nb_res : 1};
  let sync = utils::new_oneresult(false);
  // for propagate 
  if rp.store.send(KVStoreMgmtMessage::KVAddPropagate(val,Some(sync.clone()),queryconf)).is_err() {
    error!("chann error in store val");
    return false
  };


  // TODO wait for propagate result...??? plus new message cause storekv is
  // wait for conflict management issue reply TODO instead of simple bool
  // for local
  match utils::clone_wait_one_result(&sync,None) {
    None => {error!("Condvar issue for storing value!!"); false},// not logic 
    Some (r) => r,
  }*/
}


/// Find a value by key. Specifying our queryconf, and priorities.
pub fn find_val<RT : RunningTypes> (rp : &RunningProcesses<RT>, rc : &ArcRunningContext<RT>, nid : <RT::V as KeyVal>::Key, qconf : &QueryConf, prio : QueryPriority, sprio : StoragePriority, nb_res : usize ) -> Vec<Option<RT::V>> {
  panic!("TODO delete it");
/*  debug!("Finding KeyVal {:?}", nid);
  // TODO factorize code with find peer and/or specialize rules( some for peer some for kv) ??
  let maxhop = rc.rules.nbhop(prio);
  let nbquer = rc.rules.nbquery(prio);
  let semsize = rc.rules.notfoundtreshold(nbquer,maxhop,&qconf.mode);
  let msgqmode;
  //= init_qmode(rc, &qconf.mode);
  let lifetime = rc.rules.lifetime(prio);
  let lastsent = qconf.hop_hist.map(|(n,ishop)| if ishop 
    {LastSent::LastSentHop(n,vec![rc.me.get_key()].into_iter().collect())}
    else
    {LastSent::LastSentPeer(n,vec![rc.me.get_key()].into_iter().collect())}
  );
  let store = rc.rules.do_store(true, prio, sprio, Some(0)); // first hop
  let queryconf = QueryMsg {
    modeinfo : msgqmode,
//    chunk : qconf.chunk.clone(),
    hop_hist : lastsent,
    storage : sprio,
    rem_hop : maxhop,
    nb_forw : nbquer,
    prio : prio,
    nb_res : nb_res};
  // local query replyto set to None
  let query = query::init_query(semsize, nb_res, lifetime, None, Some(store));
  let qh = query.get_handle();
  
  if rp.store.send(KVStoreMgmtMessage::KVFind(nid,Some(query),queryconf)).is_err() {
    error!("chann error in find val");
    return Vec::new();
  };

  // block until result
  qh.wait_query_result().right().unwrap()*/
}


#[inline]
fn init_qmode<P : Peer> (me : &P, qm : &QueryMode) -> QueryModeMsg<P> {
  match qm {
    &QueryMode::Asynch => QueryModeMsg::Asynch(me.get_key(),me.get_address().clone(),NULL_QUERY_ID),
    &QueryMode::AProxy => QueryModeMsg::AProxy(NULL_QUERY_ID),
    &QueryMode::AMix(i) => QueryModeMsg::AMix(i,NULL_QUERY_ID),
  }
}

impl<RT : RunningTypes> DHT<RT> {
  pub fn block (&self) {
    debug!("Blocking");
    let mut count = self.f.1.lock().unwrap();
    while *count <= 0 {
      count = self.f.0.wait(count).unwrap();
    }
    *count -= 1;
  }
  pub fn shutdown (&self) {
    debug!("Sending Shutdown");
    let mut err = self.rp.store.send(KVStoreMgmtMessage::Shutdown).is_err();
    err = self.rp.peers.send(PeerMgmtMessage::ShutDown).is_err() || err;
    if err {
      error!("Shutdown is failing due to close channels");
    };
  }
  // reping offline closest peers  TODO refactor so that we refresh until target size not
  // returning nb of connection
  pub fn refresh_closest_peers(&self, targetnb : usize) -> usize {
    if self.rp.peers.send(PeerMgmtMessage::Refresh(targetnb)).is_err() {
      error!("shutdown, channel error");
      return 0
    };
    // TODO get an appropriate response !!!
    0
  }

  #[inline]
  fn init_qmode(&self, qm : &QueryMode) -> QueryModeMsg <RT::P>{
    panic!("TODO del it");
//    init_qmode(&self.rc, qm)
  }

  pub fn ping_peer (&self, peer : Arc<RT::P>) -> bool {
    let res = utils::new_oneresult(false);
    let ores = Some(res.clone());
    if self.rp.peers.send(PeerMgmtMessage::PeerPing(peer,ores)).is_err() {
      error!("ping peer, channel error");
      return false
      };
    // TODO timeout
    utils::clone_wait_one_result(&res,None).unwrap_or(false)
  }

  pub fn find_peer (&self, nid : <RT::P as KeyVal>::Key, qconf : &QueryConf, prio : QueryPriority ) -> Option<Arc<RT::P>> {
/*    debug!("Finding peer {:?}", nid);
    let maxhop = self.rc.rules.nbhop(prio);
    println!("!!!!!!!!!!!!!!!!!!! maxhop : {}, prio : {}", maxhop, prio);
    let nbquer = self.rc.rules.nbquery(prio);
    let semsize = self.rc.rules.notfoundtreshold(nbquer,maxhop,&qconf.mode);
    let msgqmode = self.init_qmode(&qconf.mode);
    let lifetime = self.rc.rules.lifetime(prio);
    let lastsent = qconf.hop_hist.map(|(n,ishop)| if ishop 
      {LastSent::LastSentHop(n,vec![self.rc.me.get_key()].into_iter().collect())}
    else
      {LastSent::LastSentPeer(n,vec![self.rc.me.get_key()].into_iter().collect())}
    );
    let nb_res = 1;
    let queryconf = QueryMsg {
      modeinfo : msgqmode.clone(), 
//      chunk : qconf.chunk.clone(), 
      hop_hist : lastsent,
      storage : StoragePriority::All,
      rem_hop : maxhop,
      nb_forw : nbquer,
      prio : prio,
      nb_res : nb_res}; // querystorage priority is hadcoded but not used to (peer are curently always stored) TODO switch to option??*/
    // local query replyto set to None
/*    let query : Query<RT::P,RT::V> = query::init_query(semsize, nb_res, lifetime, None, None); // Dummy store policy
    let qh = query.get_handle();
    // TODO send directly peermgmt
    if self.rp.peers.send(PeerMgmtMessage::PeerFind(nid,Some(query),queryconf,false)).is_err() {
//    if self.rp.queries.send(QueryMgmtMessage::NewQuery(query.clone(), PeerMgmtInitMessage::PeerFind(nid, queryconf))).is_err() {
      error!("find peer, channel error");
      return None
    }; // TODO return result??
    // block until result
    qh.wait_query_result().left().unwrap()
*/
    panic!("TODOLE");
  }


  // at the time only query without persistence and intermediatory persistence strategy (related
  // to route strategy)
  // Notice that most of the time V must be defined as an Arc of something with serialize
  // implementation on its content (there is quite a lot of clone involved).
  /// Find a value by key. Specifying our queryconf, and priorities.
  #[inline]
  pub fn find_val (&self, nid : <RT::V as KeyVal>::Key, qc : &QueryConf, prio : QueryPriority, sprio : StoragePriority, nb_res : usize ) -> Vec<Option<RT::V>> {
    find_val(&self.rp, &self.rc, nid, qc, prio, sprio, nb_res)
  }

  // at the time only local without propagation strategy return true if ok (todo variant with or
  // without propagation result)
  /// Store a value. Specifying our queryconf, and priorities. Note that priority rules are very
  /// important to know if we do propagate value or store local only or cache local only.
  #[inline]
  pub fn store_val (&self, val : RT::V, qc : &QueryConf, prio : QueryPriority, sprio : StoragePriority) -> bool {
    store_val(&self.rp, &self.rc, val, qc, prio, sprio)
  }

}

/// manage result of a spawned handler
fn sphandler_res<A, E : Debug + Display> (res : StdResult<A, E>) {
  match res {
    Ok(_) => debug!("Spawned result returned gracefully"),
    Err(e) => error!("Thread exit due to error : {}",e),
  }
}

static NULL_QUERY_ID : usize = 0; // TODO replace by optional value to None!!
pub type GlobalHandle<MC : MyDHTConf> = <MC::GlobalServiceSpawn as Spawner<MC::GlobalService,GlobalDest<MC>,<MC::GlobalServiceChannelIn as SpawnChannel<GlobalCommand<MC::PeerRef,MC::GlobalServiceCommand>>>::Recv>>::Handle;

pub type GlobalHandleSend<MC : MyDHTConf> = HandleSend<<MC::GlobalServiceChannelIn as SpawnChannel<GlobalCommand<MC::PeerRef,MC::GlobalServiceCommand>>>::Send,
  <<
    MC::GlobalServiceSpawn as Spawner<MC::GlobalService,GlobalDest<MC>,<MC::GlobalServiceChannelIn as SpawnChannel<GlobalCommand<MC::PeerRef,MC::GlobalServiceCommand>>>::Recv>>::Handle as 
    SpawnHandle<MC::GlobalService,GlobalDest<MC>,<MC::GlobalServiceChannelIn as SpawnChannel<GlobalCommand<MC::PeerRef,MC::GlobalServiceCommand>>>::Recv>
    >::WeakHandle
    >;
pub type ApiHandle<MC : MyDHTConf> = <MC::ApiServiceSpawn as Spawner<MC::ApiService,ApiDest<MC>,<MC::ApiServiceChannelIn as SpawnChannel<ApiCommand<MC>>>::Recv>>::Handle;
pub type ApiHandleSend<MC : MyDHTConf> = HandleSend<<MC::ApiServiceChannelIn as SpawnChannel<ApiCommand<MC>>>::Send,
  <<
    MC::ApiServiceSpawn as Spawner<MC::ApiService,ApiDest<MC>,<MC::ApiServiceChannelIn as SpawnChannel<ApiCommand<MC>>>::Recv>>::Handle as 
    SpawnHandle<MC::ApiService,ApiDest<MC>,<MC::ApiServiceChannelIn as SpawnChannel<ApiCommand<MC>>>::Recv>
    >::WeakHandle
    >;


pub type PeerStoreHandle<MC : MyDHTConf> = <MC::PeerStoreServiceSpawn as 
Spawner<
    KVStoreService<MC::Peer,MC::PeerRef,MC::Peer,MC::PeerRef,MC::PeerKVStore,MC::DHTRules,MC::PeerStoreQueryCache>,
    OptPeerGlobalDest<MC>,
    <MC::PeerStoreServiceChannelIn as SpawnChannel<GlobalCommand<MC::PeerRef,KVStoreCommand<MC::Peer,MC::Peer,MC::PeerRef>>>>::Recv
  >
>::Handle;


