use std::io::Result as IoResult;
use std::sync::mpsc::{Receiver,Sender};
use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};
use peer::{PeerMgmtMeths, PeerPriority, PeerState};
use query::{self,QueryConf,QueryPriority,QueryMode,QueryModeMsg,LastSent,QueryMsg,Query};
use rules::DHTRules;
use kvstore::{StoragePriority, KVStore};
use keyval::{KeyVal};
use query::cache::QueryCache;
use std::str::from_utf8;
use rustc_serialize::json;
use self::mesgs::{PeerMgmtMessage,PeerMgmtInitMessage,KVStoreMgmtMessage,QueryMgmtMessage};
use std::str::FromStr;
use std::sync::{Arc,Semaphore,Mutex,Condvar};
use std::sync::mpsc::channel;
use std::thread::{JoinGuard};
use std::thread;
use route::Route;
use peer::Peer;
use transport::{Transport,ReadTransportStream,WriteTransportStream,Address};
use time::Duration;
use utils::{self,OneResult};
use msgenc::MsgEnc;
use num;
use num::traits::ToPrimitive;
use std::marker::PhantomData;

pub mod mesgs;
mod server;
mod client;
mod peermanager;
mod kvmanager;
mod querymanager;

/// utility trait to avoid lot of parameters in each struct / fn
/// kinda aliasing
pub trait RunningTypes : Send + Sync {
  type A : Address;
  type P : Peer<Address = Self::A>;
  type V : KeyVal;
  type M : PeerMgmtMeths<Self::P, Self::V>;
  type R : DHTRules;
  type E : MsgEnc;
  type T : Transport<Address = Self::A>;
}

// TODO implement : client mode in rules!!
#[derive(Debug,PartialEq,Eq)]
pub enum ClientMode {
  /// client run from PeerManagement and does not loop
  /// - bool say if we spawn a thread or not
  Local(bool),
  /// client stream run in its own stream
  /// one thread by client for sending
  ThreadedOne,
  /// max n clients by threads
  ThreadedMax(usize),
  /// n threads sharing all client
  ThreadPool(usize),
}

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
 
/// Could be use to define the final type of a DHT, most of the time we create a new object (see
/// example/fs.rs).
/// This kind of struct is never use, it is just to a type instead of a
/// trait in generic parameters.
struct RunningTypesImpl<
  A : Address,
  P : Peer<Address = A>,
  V : KeyVal,
  M : PeerMgmtMeths<P, V>, 
  R : DHTRules,
  E : MsgEnc, 
  T : Transport<Address = A>>
  (PhantomData<A>,PhantomData<R>,PhantomData<P>,PhantomData<V>,PhantomData<M>,PhantomData<T>, PhantomData<E>);

impl<
  A : Address,
  P : Peer<Address = A>,
  V : KeyVal,
  M : PeerMgmtMeths<P, V>, 
  R : DHTRules,
  E : MsgEnc, 
  T : Transport<Address = A>>
     RunningTypes for RunningTypesImpl<A, P, V, M, R, E, T> {
  type A = A;
  type P = P;
  type V = V;
  type M = M;
  type R = R;
  type E = E;
  type T = T;
}

pub type ClientChanel<P, V> = Sender<mesgs::ClientMessage<P,V>>;

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
 

// TODO replace f by Arc<Condvar>
/// DHT infos
pub struct DHT<RT : RunningTypes> {
  rp : RunningProcesses<RT>,
  rc : ArcRunningContext<RT>, 
  f : Arc<Semaphore>
}


/// Find a value by key. Specifying our queryconf, and priorities.
pub fn find_local_val<RT : RunningTypes> (rp : &RunningProcesses<RT>, rc : &ArcRunningContext<RT>, nid : <RT::V as KeyVal>::Key ) -> Option<RT::V> {
  debug!("Finding KeyVal locally {:?}", nid);
  let sync = Arc::new((Mutex::new(None),Condvar::new()));
  // local query replyto set to None
  rp.store.send(KVStoreMgmtMessage::KVFindLocally(nid, Some(sync.clone())));
  // block until result
  utils::clone_wait_one_result(&sync, None).unwrap_or(None)
}

/// Store a value. Specifying our queryconf, and priorities. Note that priority rules are very
/// important to know if we do propagate value or store local only or cache local only.
pub fn store_val <RT : RunningTypes> (rp : &RunningProcesses<RT>, rc : &ArcRunningContext<RT>, val : RT::V, qconf : &QueryConf, prio : QueryPriority, sprio : StoragePriority) -> bool {
  let msgqmode = init_qmode(rp, rc, &qconf.mode);
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
  let sync = Arc::new((Mutex::new(false),Condvar::new()));
  // for propagate 
  rp.store.send(KVStoreMgmtMessage::KVAddPropagate(val,Some(sync.clone()),queryconf));
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
  let semsize = match qconf.mode {
    QueryMode::Asynch => num::pow(nbquer.to_usize().unwrap(), maxhop.to_usize().unwrap()),
    // general case we wait reply in each client query
    _ => nbquer.to_usize().unwrap(),
  };
  let msgqmode = init_qmode(rp, rc, &qconf.mode);
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
  
  rp.store.send(KVStoreMgmtMessage::KVFind(nid,Some(query.clone()),queryconf, true));

  // block until result
  query.wait_query_result().right().unwrap()
}

#[inline]
fn init_qmode<RT : RunningTypes> (rp : &RunningProcesses<RT>, rc : &ArcRunningContext<RT>, qm : &QueryMode) -> QueryModeMsg <RT::P>{
  match qm {
    &QueryMode::Asynch => QueryModeMsg::Asynch((rc.me).clone(),rc.rules.newid()),
    &QueryMode::AProxy => QueryModeMsg::AProxy((rc.me).clone(),rc.rules.newid()),
    &QueryMode::AMix(i) => QueryModeMsg::AMix(i,rc.me.clone(),rc.rules.newid()),
  }
}

impl<RT : RunningTypes> DHT<RT> {
  pub fn block (&self) {
    debug!("Blocking");
    self.f.acquire();
  }
  pub fn shutdown (&self) {
    debug!("Sending Shutdown");
    self.rp.store.send(KVStoreMgmtMessage::Shutdown);
    self.rp.peers.send(PeerMgmtMessage::ShutDown);
  }
  // reping offline closest peers  TODO refactor so that we refresh until target size not
  // returning nb of connection
  pub fn refresh_closest_peers(&self, targetNb : usize) -> usize {
    self.rp.peers.send(PeerMgmtMessage::Refresh(targetNb));
    // TODO get an appropriate response
    0
  }

  #[inline]
  fn init_qmode(&self, qm : &QueryMode) -> QueryModeMsg <RT::P>{
    init_qmode(&self.rp, &self.rc, qm)
  }

  pub fn find_peer (&self, nid : <RT::P as KeyVal>::Key, qconf : &QueryConf, prio : QueryPriority ) -> Option<Arc<RT::P>>  {
    debug!("Finding peer {:?}", nid);
    let maxhop = self.rc.rules.nbhop(prio);
    println!("!!!!!!!!!!!!!!!!!!! maxhop : {}, prio : {}", maxhop, prio);
    let nbquer = self.rc.rules.nbquery(prio);
    let semsize = match qconf.mode {
      QueryMode::Asynch => num::pow(nbquer.to_usize().unwrap(), maxhop.to_usize().unwrap()),
      // general case we wait reply in each client query
      _ => nbquer.to_usize().unwrap(),
    };
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
    self.rp.queries.send(QueryMgmtMessage::NewQuery(query.clone(), PeerMgmtInitMessage::PeerFind(nid, queryconf)));
    // block until result
    query.wait_query_result().left().unwrap()

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
 <T : Route<RT::P,RT::V,RT::T>, 
  QC : QueryCache<RT::P,RT::V>, 
  S : KVStore<RT::V>,
  F1 : FnOnce() -> Option<S> + Send + 'static,
  F2 : FnOnce() -> Option<T> + Send + 'static,
  F3 : FnOnce() -> Option<QC> + Send + 'static,
 >
 (rc : ArcRunningContext<RT>, 
  mut route : F2, 
  mut querycache : F3, 
  mut kvst : F1,
  cachedNodes : Vec<Arc<RT::P>>, 
  bootNodes : Vec<Arc<RT::P>>,
  ) 
 -> DHT<RT> {

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
thread::scoped (move ||{
  querymanager::start::<RT,_,_>(&rquery, &cleantquery, &cleantpeer, &cleantkstor, querycache, cleandelay);
});
let sem = Arc::new(Semaphore::new(-1)); // wait end of two process from shutdown TODO replace that by joinhandle wait!!!

let rp = RunningProcesses {
  peers : tpeer.clone(), 
  queries : tquery.clone(),
  store : tkvstore.clone()
};
let tpeer3 = tpeer.clone();
// starting peer manager process
let rcsp = rc.clone();
let rpsp = rp.clone();
let semsp = sem.clone();
thread::scoped (move ||{
  peermanager::start (rcsp, route, &rpeer,rpsp, semsp)
});

// starting kvstore process
let rcst = rc.clone();
let rpst = rp.clone();
let semsp2 = sem.clone();
thread::scoped (move ||{
  kvmanager::start (rcst, kvst, &rkvstore,rpst, semsp2);
});

// starting socket listener process
let tpeer2 = tpeer3.clone();
let tpeer4 = tpeer3.clone();
let rcsp2 = rc.clone();
let rpsp2 = rp.clone();
thread::scoped (move ||{
  server::servloop(rcsp2, rpsp2)
});

// Typically those cached node are more likely to be initialized with the routing backend (here it
// is slower as we need to clone nodes)
info!("loading additional cached node {:?}", cachedNodes);
for p in cachedNodes.iter() {
  tpeer3.send(PeerMgmtMessage::PeerAddOffline(p.clone(),false));
}

info!("bootstrapping with {:?}", bootNodes);
for p in bootNodes.iter() {
  tpeer3.send(PeerMgmtMessage::PeerPing(p.clone(),None)); // TODO avoid cloning node... eg try maping to collection of arc
}

DHT{
  rp : RunningProcesses {
    peers : tpeer4,
    queries : resulttquery,
    store : tkvstore,
  },
  rc : rc,
  f : sem
}

}

}
