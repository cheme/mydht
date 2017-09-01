//! Main loop for mydht. This is the main dht loop, see default implementation of MyDHT main_loop
//! method.
//! Usage of mydht library requires to create a struct implementing the MyDHTConf trait, by linking with suitable inner trait implementation and their requires component.

use mydhtresult::{
  Result,
};
use service::{
  Service,
  Spawner,
};
use std::rc::Rc;
use std::cell::Cell;
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
  SlabEntry,
  SlabEntryState,
};
use utils::{
  Ref,
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
pub struct MyDHT<M : MyDHTConf>(Sender<MainLoopCommand<M>>);
impl<MC : MyDHTConf> MyDHT<MC> {
  /// main method to start TODO rewrite as service start (multiple spawner) with state -> could
  /// thread it or state it or coroutine it , need rewrite start loop and r... For now : no spawner
  /// , statically spawn a thread
  fn run(conf : MC) -> Result<Self> {
    let sender = conf.start_loop()?;
    Ok(MyDHT(sender))
  }
}

type SpawnerRefs<S : Spawner> = (<S as Spawner>::Handle,<S as Spawner>::Send); 

pub trait MyDHTConf : 'static + Send + Sized {

  /// Name of the main thread
  const loop_name : &'static str = "MyDHT Main Loop";


  /// low level transport
  type Transport : Transport<Address = Self::Address>;
  /// low level transport address
  type Address : Address;
  /// Message encoding
  type MsgEnc : MsgEnc;
  /// Peer struct (with key and address)
  type Peer : Peer<Address = Self::Address>;
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
  type Slab : SlabCache<SlabEntry<Self::Transport,SpawnerRefs<Self::ReadSpawn>, SpawnerRefs<Self::WriteSpawn>,Self::PeerRef>>;
  /// local cache for peer
  type PeerCache : KVCache<<Self::Peer as KeyVal>::Key,PeerCache<Self::PeerRef>>;

  type ReadSpawn : Spawner<Self::ReadService>; 
  type WriteSpawn : Spawner<Self::WriteService>;
  // TODO temp for type check , hardcode it after on transport service (not true for kvstore service) the service contains the read stream!!
  // type ReadService : Service;
  // TODO call to read and write in service will use ReadYield and WriteYield wrappers containing
  // the spawn yield (cf sample implementation in transport tests).
  type WriteService : Service;
  /// Start the main loop
  #[inline]
  fn start_loop(self : Self) -> Result<Sender<MainLoopCommand<Self>>> {
    let (s,r) = channel();
    ThreadBuilder::new().name(Self::loop_name.to_string()).spawn(move || self.main_loop(r))?;
    Ok(s)
  }

  fn main_loop(self : Self, command_rec : Receiver<MainLoopCommand<Self>>) -> Result<()> {
    Ok(())
  }

}

struct PeerCache<P> {
  peer : P,
  ///  if not initialized CACHE_NO_STREAM, if needed in sub process could switch to arc atomicusize
  read : Rc<Cell<usize>>,
  ///  if not initialized CACHE_NO_STREAM, if 
  write : Rc<Cell<usize>>,
}

impl<P> PeerCache<P> {
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


/// command supported by MyDHT loop
enum MainLoopCommand<TT : MyDHTConf> {
  Test(TT::Address)
}
