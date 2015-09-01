//! Message used in channel between processes.

use peer::{Peer,PeerPriority,PeerState,PeerStateChange};
use query::{Query,QueryID,QueryMsg,QueryChunk};
use std::sync::{Arc};
//use rustc_serialize::{Encodable,Decodable};
use keyval::KeyVal;
use utils::{OneResult};
use query::cache::CachePolicy;
use transport::{ReadTransportStream,WriteTransportStream};
//use procs::RunningTypes;
use procs::ClientHandle;
//use procs::server::ServerHandle;
use std::sync::mpsc::{SyncSender};
use route::ServerInfo;
use std::marker::PhantomData;

/// message for peermanagement TODO remove TR when stable (currently unused phantomdata in
/// PeerQueryMinus)
pub enum PeerMgmtMessage<P : Peer,V : KeyVal,TR : ReadTransportStream, TW : WriteTransportStream> {
  /// update peer with state (can be used to set a peer offline)
  PeerUpdatePrio(Arc<P>, PeerState),
  /// update peer by changing state
  PeerChangeState(Arc<P>, PeerStateChange),
  /// update peer with priority (can be used to set a peer offline)
  /// with possibly an initial write stream : add to cache or start cli thread (or add to cli pool).
  /// with possibly an initial read stream : start or add to pool of server.
  /// Also Query for a client handle (from procs or other process to skip peermanagement process in
  /// later calls), a synchro is needed eitherway.
  PeerAddFromServer(Arc<P>, PeerPriority, Option<TW>, SyncSender<ClientHandle<P,V,TW>>, ServerInfo),
  /// TODO use key instead of P (peer handle already initialized)
  ServerInfoFromClient(Arc<P>, ServerInfo),
  /// add an offline peer
  /// if you want to add and connect please use PeerPing instead
  PeerAddOffline(Arc<P>),
  /// remove a peer info and terminate possible server thread 
  /// State to change to
  PeerRemFromClient(Arc<P>, PeerStateChange),
  /// remove a peer info and terminate possible client thread
  /// State to change to
  PeerRemFromServer(Arc<P>, PeerStateChange),
  /// Ping a peer, calling process is possibly waiting for a result
  /// Optionaly block and wait for a result.
  /// PeerPing is same as starting an auth, and therefore automatically add the peer (no need to
  /// peer add before)
  PeerPing(Arc<P>, Option<OneResult<bool>>), // optional mutex if we need reply
  /// Pong a peer, this is emit after ping reception on server, client may not be initialized.
  /// String is signed challenge
  /// priority is for initialized of peers if not in peer manager (no update on xisting client)
  /// optional write stream is for non managed server (no client handle get from a first peer add,
  /// and a write stream to send)
  PeerPong(Arc<P>, PeerPriority, String, Option<TW>),
  /// received pong need auth
  /// first peer : TODO switch to key only
  /// second received signed challenge
  PeerAuth(P, String),
  /// Find a peer by its key, (first local lookup, then depending on query conf propagate).
  ///  - first is key of peer to find
  ///  - second is possible query stored in querymanager (asynch is done without it), only used to
  ///  change send and expected result counters (less messages to querymanager but less clean
  ///  TODO change it ??)
  ///  - third is query msg
  PeerFind(P::Key,Option<Query<P,V>>, QueryMsg<P>),
  /// Find a value by its key. Similar to `PeerPing`
  KVFind(V::Key,Option<Query<P,V>>, QueryMsg<P>), // option on query node indicate if it is local or not(must known for asynch) // used for local find or proxied find, , first int is remaining hop, second is nbquery to propagate, query node is used to receive the reply
    // TODO check that option query is needed for kvfind (or just query)
  /// Shutdown peer cache.
  ShutDown,
  /// Ping n peer (usefull to initiate DHT or too many offline nodes)
  Refresh(usize),
  /// Manage counter of running query (currently only for proxy mode related route implementation
  /// where we should avoid peer with existing query (blocking)
  PeerQueryPlus(Arc<P>),
  /// Same as `PeerQueryPlus` but minus
  PeerQueryMinus(Arc<P>, PhantomData<TR>),
  /// Store peer depending on queryconf (locally plus possibly propagate)
  StoreNode(QueryMsg<P>,Option<Arc<P>>), // destination storage node must be in query conf
  /// Store value depending on queryconf (locally plus possibly propagate)
  StoreKV(QueryMsg<P>,Option<V>),


  /// for client send message without client thread : should only be used by clienthandle
  ClientMsg(ClientMessage<P,V,TW>,P::Key),
}

/// message for query management process
pub enum QueryMgmtMessage<P : Peer, V : KeyVal + Send> {
  /// add a new query to query cache
  /// - first is query
  /// - second is optionaly a message to proxied to peermgmt
  NewQuery(Query<P,V>,PeerMgmtInitMessage<P,V>), // store a new query, forward to peermanager
  //NewQuery(QueryID, Query<P,V>, Arc<Semaphore>), // store a new query, mutex to synchro when done
  /// get a reply to a query : either peer or keyval
//  NewPeerReply(QueryID,Option<Arc<P>>), // all reply checking, opening ... must be done before
//  NewKVReply(QueryID,Option<V>), // all reply checking, opening ... must be done before
  NewReply(QueryID,(Option<Arc<P>>,Option<V>)), // TODO remove
  /// run a clean cache
  PerformClean,
}

/// message for key value store process
pub enum KVStoreMgmtMessage<P : Peer, V : KeyVal> {
  /// add a keyval, with optional reply and possible propagate based on queryconf
  KVAddPropagate(V,Option<OneResult<bool>>, QueryMsg<P>), // Note that the KeyVal rather be of type Arc<something> otherwhise lot of clone involved - query conf is used to propagate the add
  /// add a keyval, with optional reply, and given storing config
  KVAdd(V,Option<OneResult<bool>>, (bool, Option<CachePolicy>)), // Note that the KeyVal rather be of type Arc<something> otherwhise lot of clone involved - query conf is used to propagate the add
  /// look up in stor for a value, reply through optinal query or queryconf reply, propagate like
  /// KVAddPropagate.
  /// last bool is true if query exist (second param) but is not stored in query manager : if we
  /// need to store query
  KVFind(V::Key,Option<Query<P,V>>, QueryMsg<P>, bool), // option on query node indicate if it is local or not(must known for asynch) // if result and query return through query else send to peermanager for proxy
  /// like kvadd, act locally only
  KVFindLocally(V::Key,Option<OneResult<Option<V>>>),
    // for simple local find only
//    KVLocalFind(V::Key, (Condvar,Mutex<V>)),
  Shutdown,
  Commit,
}

pub enum PeerMgmtInitMessage<P : Peer,V : KeyVal> {
  /// Find a peer by its key, (first local lookup, then depending on query conf propagate).
  PeerFind(P::Key, QueryMsg<P>),
  /// Find a value by its key. Similar to `PeerPing`
  KVFind(V::Key, QueryMsg<P>), // option on query node indicate if it is local or not(must known for asynch) // used for local find or proxied find, , first int is remaining hop, second is nbquery to propagate, query node is used to receive the reply
}
impl <P : Peer, V : KeyVal> PeerMgmtInitMessage<P,V> {
  pub fn change_query_id(&mut self, qid : QueryID) {
    match self {
      &mut PeerMgmtInitMessage::PeerFind(_, ref mut msg) => msg.modeinfo.set_qid(qid),
      &mut PeerMgmtInitMessage::KVFind(_, ref mut msg) => msg.modeinfo.set_qid(qid),
    }
  }
  pub fn to_peermsg<TR : ReadTransportStream, TW : WriteTransportStream> (self, query : Query<P,V>) -> PeerMgmtMessage<P,V,TR,TW> {
    match self {
      PeerMgmtInitMessage::PeerFind(pk, qmsg) => {
        PeerMgmtMessage::PeerFind(pk,Some(query), qmsg)
      },
      PeerMgmtInitMessage::KVFind(vk, qmsg) => {
        PeerMgmtMessage::KVFind(vk,Some(query),qmsg)
      },
    }
 
  }
  // TODO fn to create PeerMgmtMessage from init (consume it)
}


//pub type ClientMessageIx<P : Peer, V : KeyVal> = (ClientMessage<P,V>,usize);
pub type ClientMessageIx<P, V, TW> = (ClientMessage<P,V,TW>,usize);

/// message for client process
pub enum ClientMessage<P : Peer, V : KeyVal, TW : WriteTransportStream> {
  /// Ping a peer (establishing connection and storing peer in peer manager
  /// parameter is challenge
  PeerPing(Arc<P>, String),
  // - first is token for multiplexed threads
  // - second is challenge
  // - third is message signing
  //PeerPing(usize,String,String),
  
  
  // signed challenge
  PeerPong(String),
  /// Find a peer, option on query depends on query mode and is used for error management (lessen
  /// or not the number of send query)
  PeerFind(P::Key, Option<Query<P,V>>, QueryMsg<P>),
  /// find a `KeyVal`
  KVFind(V::Key, Option<Query<P,V>>, QueryMsg<P>),
  /// reply to a find peer or propagate
  StoreNode(Option<QueryID>, Option<Arc<P>>), // proxy a request and do some control if needed
  /// reply to a find keyval or propagate
  StoreKV(Option<QueryID>, QueryChunk, Option<V>), // proxy a request and do some control if needed
  /// end communication with a peer (using usize index)
  // End(usize), // when issue with receiving on channel or other, use this to drop your client
  /// multiplex peer add, token is managed by peermanager and put in param
//  MultPeerAdd(Arc<P>, usize, T::WriteStream), // T is transport
  /// shutdown client process TODO replace by end
  ShutDown, // when issue with receiving on channel or other, use this to drop your client
  /// for multiplex peer, add a peer at given index, index is from clientmassageix
  PeerAdd(Arc<P>, Option<TW>),
  /// for multiplex peer, end the thread. Message send from peer manager to multiplexed thread with
  /// an index out of thread peers scope (otherwhise send to peer).
  EndMult,
}

pub enum ServerPoolMessage<P : Peer, TR : ReadTransportStream> {
  /// multiplex peer add, token is managed by peermanager and put in param
  MultPeerAdd(Arc<P>, usize, TR), // T is transport
  /// end communication with a peer (using usize index)
  End(usize), // when issue with receiving on channel or other, use this to drop your client
 
}

