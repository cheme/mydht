use std::sync::mpsc::{Sender};
use std::borrow::Borrow;
use peer::{PeerMgmtMeths};
use query::{self,QueryConf,QueryPriority,QueryMode,QueryModeMsg,LastSent,QueryMsg,Query};
use rules::DHTRules;
use kvstore::{StoragePriority, KVStore};
use keyval::{KeyVal};
use query::cache::QueryCache;
use self::mesgs::{PeerMgmtMessage,KVStoreMgmtMessage,QueryMgmtMessage,ClientMessage,ClientMessageIx};
//use self::mesgs::{PeerMgmtMessage,PeerMgmtInitMessage,KVStoreMgmtMessage,QueryMgmtMessage,ClientMessage,ClientMessageIx};
use std::sync::{Arc,Condvar,Mutex};
use std::sync::mpsc::channel;
use std::thread;
use peer::Peer;
use time::Duration;
use utils::{self};
use msgenc::MsgEnc;
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
use self::mainloop::{
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
pub mod mesgs;

mod mainloop;
pub mod api;
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
  ApiSendIn,
};

pub struct MyDHTService<MDC : MyDHTConf>(pub MDC, pub MioRecv<<MDC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MDC>>>::Recv>, pub <MDC::MainLoopChannelOut as SpawnChannel<MainLoopReply>>::Send,pub MioSend<<MDC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MDC>>>::Send>);

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

impl<MDC : MyDHTConf> Service for MyDHTService<MDC> {
  type CommandIn = MainLoopCommand<MDC>;
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


pub type RWSlabEntry<MDC : MyDHTConf> = SlabEntry<
  MDC::Transport,
//  (),
  //SpawnerRefsRead2<ReadService<MDC>, MDC::ReadDest, MDC::ReadChannelIn, MDC::ReadSpawn>,
  SpawnerRefsDefRecv2<ReadService<MDC>,ReadCommand, ReadDest<MDC>, MDC::ReadChannelIn, MDC::ReadSpawn>,
//  SpawnerRefs<ReadService<MDC>,MDC::ReadDest,DefaultRecvChannel<ReadCommand,MDC::ReadChannelIn>,MDC::ReadSpawn>,
  SpawnerRefs2<WriteService<MDC>,WriteCommand<MDC>,MDC::WriteDest,MDC::WriteChannelIn,MDC::WriteSpawn>,
  // bool is has_connect
  (<MDC::WriteChannelIn as SpawnChannel<WriteCommand<MDC>>>::Send,<MDC::WriteChannelIn as SpawnChannel<WriteCommand<MDC>>>::Recv,bool),
  MDC::PeerRef>;

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
type MainLoopRecvIn<MDC : MyDHTConf> = MioRecv<<MDC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MDC>>>::Recv>;
type MainLoopSendIn<MDC : MyDHTConf> = MioSend<<MDC::MainLoopChannelIn as SpawnChannel<MainLoopCommand<MDC>>>::Send>;

pub enum MainLoopReply {
  /// TODO
  Ended,
}

pub type PeerRefSend<MC:MyDHTConf> = <MC::PeerRef as Ref<MC::Peer>>::Send;
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
  type MsgEnc : MsgEnc + Clone;
  /// Peer struct (with key and address)
  type Peer : Peer<Address = <Self::Transport as Transport>::Address>;
  /// most of the time Arc, if not much threading or smal peer description, RcCloneOnSend can be use, or AllwaysCopy
  /// or Copy.
  type PeerRef : Ref<Self::Peer>;
  /// shared info
  type KeyVal : KeyVal;
  /// Peer management methods 
  type PeerMgmtMeths : PeerMgmtMeths<Self::Peer, Self::KeyVal> + Clone;
  /// Dynamic rules for the dht
  type DHTRules : DHTRules + Clone;
  /// loop slab implementation
  type Slab : SlabCache<RWSlabEntry<Self>>;
  /// local cache for peer
  type PeerCache : KVCache<<Self::Peer as KeyVal>::Key,PeerCacheEntry<Self::PeerRef>>;
  /// local cache for auth challenges
  type ChallengeCache : KVCache<Vec<u8>,ChallengeEntry>;
  
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

  // TODO temp for type check , hardcode it after on transport service (not true for kvstore service) the service contains the read stream!!
//  type ReadService : Service;
  // TODO call to read and write in service will use ReadYield and WriteYield wrappers containing
  // the spawn yield (cf sample implementation in transport tests).
  //type WriteService : Service;


  /// Start the main loop
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

  //fn init_read_spawner_out(<Self::MainLoopChannelIn as SpawnChannel<MainLoopCommand<Self>>>::Send, <Self::PeerMgmtChannelIn as SpawnChannel<PeerMgmtCommand<Self>>>::Send) -> Result<ReadDest<Self>>;
  fn init_write_spawner_out() -> Result<Self::WriteDest>;
  fn init_read_channel_in(&mut self) -> Result<Self::ReadChannelIn>;
  fn init_write_channel_in(&mut self) -> Result<Self::WriteChannelIn>;
  fn init_enc_proto(&mut self) -> Result<Self::MsgEnc>;
  fn init_peermgmt_proto(&mut self) -> Result<Self::PeerMgmtMeths>;
  fn init_dhtrules_proto(&mut self) -> Result<Self::DHTRules>;
  /// Transport initialization
  fn init_transport(&mut self) -> Result<Self::Transport>;



}


/// entry cache for challenge
pub struct ChallengeEntry {
//  pub challenge : Vec<u8>,
  pub write_tok : usize,
  pub read_tok : Option<usize>,
}

/// utility trait to avoid lot of parameters in each struct / fn
/// kinda aliasing
pub trait RunningTypes : Send + Sync + 'static
{
  type A : Address;
  type P : Peer<Address = Self::A>;
  type V : KeyVal;
  type M : PeerMgmtMeths<Self::P, Self::V>;
  type R : DHTRules;
  type E : MsgEnc;
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
  M : PeerMgmtMeths<P, V>, 
  R : DHTRules,
  E : MsgEnc, 
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
  M : PeerMgmtMeths<P, V>, 
  R : DHTRules,
  E : MsgEnc, 
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
  queries : Sender<QueryMgmtMessage<RT::P,RT::V>>,
  store : Sender<mesgs::KVStoreMgmtMessage<RT::P,RT::V>>,
}

// deriving seems ko for now TODO test with derive again
impl<RT : RunningTypes> Clone for RunningProcesses<RT> {
  fn clone(&self) ->  RunningProcesses<RT> {
    RunningProcesses {
      peers : self.peers.clone(),
      queries : self.queries.clone(),
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
pub fn store_val <RT : RunningTypes> (rp : &RunningProcesses<RT>, rc : &ArcRunningContext<RT>, val : RT::V, qconf : &QueryConf, prio : QueryPriority, sprio : StoragePriority) -> bool {
  let msgqmode = init_qmode(rc, &qconf.mode);
  let lastsent = qconf.hop_hist.map(|(n,ishop)| if ishop 
    {LastSent::LastSentHop(n,vec![rc.me.get_key()].into_iter().collect())}
    else
    {LastSent::LastSentPeer(n,vec![rc.me.get_key()].into_iter().collect())}
  );
  let maxhop = rc.rules.nbhop(prio);
  let nbquer = rc.rules.nbquery(prio);
  let queryconf = QueryMsg {
    modeinfo : msgqmode, 
    chunk : qconf.chunk.clone(), 
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
  }
}


/// Find a value by key. Specifying our queryconf, and priorities.
pub fn find_val<RT : RunningTypes> (rp : &RunningProcesses<RT>, rc : &ArcRunningContext<RT>, nid : <RT::V as KeyVal>::Key, qconf : &QueryConf, prio : QueryPriority, sprio : StoragePriority, nb_res : usize ) -> Vec<Option<RT::V>> {
  debug!("Finding KeyVal {:?}", nid);
  // TODO factorize code with find peer and/or specialize rules( some for peer some for kv) ??
  let maxhop = rc.rules.nbhop(prio);
  let nbquer = rc.rules.nbquery(prio);
  let semsize = rc.rules.notfoundtreshold(nbquer,maxhop,&qconf.mode);
  let msgqmode = init_qmode(rc, &qconf.mode);
  let lifetime = rc.rules.lifetime(prio);
  let lastsent = qconf.hop_hist.map(|(n,ishop)| if ishop 
    {LastSent::LastSentHop(n,vec![rc.me.get_key()].into_iter().collect())}
    else
    {LastSent::LastSentPeer(n,vec![rc.me.get_key()].into_iter().collect())}
  );
  let store = rc.rules.do_store(true, prio, sprio, Some(0)); // first hop
  let queryconf = QueryMsg {
    modeinfo : msgqmode,
    chunk : qconf.chunk.clone(),
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
  qh.wait_query_result().right().unwrap()
}

#[inline]
fn init_qmode<RT : RunningTypes> (rc : &ArcRunningContext<RT>, qm : &QueryMode) -> QueryModeMsg <RT::P> {
  match qm {
    &QueryMode::Asynch => QueryModeMsg::Asynch((rc.me).clone(),NULL_QUERY_ID),
    &QueryMode::AProxy => QueryModeMsg::AProxy((rc.me).clone(),NULL_QUERY_ID),
    &QueryMode::AMix(i) => QueryModeMsg::AMix(i,rc.me.clone(),NULL_QUERY_ID),
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
    init_qmode(&self.rc, qm)
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
    debug!("Finding peer {:?}", nid);
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
      chunk : qconf.chunk.clone(), 
      hop_hist : lastsent,
      storage : StoragePriority::All,
      rem_hop : maxhop,
      nb_forw : nbquer,
      prio : prio,
      nb_res : nb_res}; // querystorage priority is hadcoded but not used to (peer are curently always stored) TODO switch to option??
    // local query replyto set to None
    let query : Query<RT::P,RT::V> = query::init_query(semsize, nb_res, lifetime, None, None); // Dummy store policy
    let qh = query.get_handle();
    // TODO send directly peermgmt
    if self.rp.peers.send(PeerMgmtMessage::PeerFind(nid,Some(query),queryconf,false)).is_err() {
//    if self.rp.queries.send(QueryMgmtMessage::NewQuery(query.clone(), PeerMgmtInitMessage::PeerFind(nid, queryconf))).is_err() {
      error!("find peer, channel error");
      return None
    }; // TODO return result??
    // block until result
    qh.wait_query_result().left().unwrap()

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

/// Main function to start a DHT.
pub fn boot_server
// <T : Route<RT::A,RT::P,RT::V,RT::T>, 
 <T,
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
 -> Result<DHT<RT>> {

  let (tquery,rquery) = channel();
  let (tkvstore,rkvstore) = channel();
  let (tpeer,rpeer) = channel();
  let cleandelay = rc.rules.asynch_clean();
  let cleantquery = tquery.clone();
  let resulttquery = tquery.clone();
  let cleantpeer = tpeer.clone();
  let cleantkstor = tkvstore.clone();
  
  // Query manager is allways start TODO a parameter for not starting it (if running dht in full
  // proxy mode for instance)
  thread::spawn (move ||{
    //sphandler_res(querymanager::start::<RT,_,_>(&rquery, &cleantquery, &cleantpeer, &cleantkstor, querycache, cleandelay));
  });
  let sem = Arc::new((Condvar::new(),Mutex::new(-1))); // wait end of two process from shutdown TODO replace that by joinhandle wait!!!
  
  let rp : RunningProcesses<RT> = RunningProcesses {
    peers : tpeer.clone(), 
    queries : tquery.clone(),
    store : tkvstore.clone()
  };
  let tpeer3 = tpeer.clone();
  // starting peer manager process
  let rcsp = rc.clone();
  let rpsp = rp.clone();
  let semsp = sem.clone();
  thread::spawn (move ||{
  /*  match peermanager::start (rcsp, route, &rpeer,rpsp) {
      Ok(()) => {
        info!("peermanager end ok");
      },
      Err(e) => {
        error!("peermanager failure : {}", e);
        panic!("peermanager failure");
      },
    };
    *semsp.1.lock().unwrap() += 1;
    semsp.0.notify_one(); // TODO replace by join handle*/
  });
  
  // starting kvstore process
  let rcst = rc.clone();
  let rpst = rp.clone();
  let semsp2 = sem.clone();
  thread::spawn (move ||{
    /*match kvmanager::start (rcst, kvst, &rkvstore,rpst) {
      Ok(()) => {
        info!("kvmanager end ok");
      },
      Err(e) => {
        error!("kvmanager failure : {}", e);
        panic!("kvmanager failure");
      },
    };

    *semsp2.1.lock().unwrap() += 1;
    semsp2.0.notify_one(); // TODO replace by join handle*/
  });
  
  // starting socket listener process
  
  let tpeer4 = tpeer3.clone();
  let rcsp2 = rc.clone();
  let rpsp2 = rp.clone();
  thread::spawn (move ||{
//    sphandler_res(server::servloop(rcsp2, rpsp2));
  });
  
  // Typically those cached node are more likely to be initialized with the routing backend (here it
  // is slower as we need to clone nodes)
  info!("loading additional cached node {:?}", cached_nodes);
  for p in cached_nodes.iter() {
    try!(tpeer3.send(PeerMgmtMessage::PeerAddOffline(p.clone())));
  }
  
  info!("bootstrapping with {:?}", boot_nodes);
  for p in boot_nodes.iter() {
    try!(tpeer3.send(PeerMgmtMessage::PeerPing(p.clone(),None))); // TODO avoid cloning node... eg try maping to collection of arc
  }

  Ok(DHT {
    rp : RunningProcesses {
      peers : tpeer4,
      queries : resulttquery,
      store : tkvstore,
    },
    rc : rc,
    f : sem
  })
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
