//! Message used in channel between processes.

use peer::{Peer,PeerPriority};
use query::{Query, QueryID, QueryPriority, QueryConf,QueryConfMsg,LastSent};
use std::sync::{Arc,Mutex,Condvar,Semaphore};
use rustc_serialize::{Encodable,Decodable};
use kvstore::KeyVal;
use utils::{OneResult};
use query::cache::CachePolicy;

/// message for peermanagement
pub enum PeerMgmtMessage<P : Peer,V : KeyVal> {
  /// update peer with priority (can be used to set a peer offline)
  PeerUpdatePrio(Arc<P>, PeerPriority),
  /// update peer with priority (can be used to set a peer offline)
  PeerAdd(Arc<P>, PeerPriority),
  /// remove a channel when a client process broke
  PeerRemChannel(Arc<P>),
  /// Ping a peer, calling process is possibly waiting for a result
  PeerPing(Arc<P>,Option<OneResult<bool>>), // optional mutex if we need reply
  /// Find a peer by its key, (first local lookup, then depending on query conf propagate). Case
  /// with no query is mainly Proxy mode proxy query where we block on next reply and do not need to keep a
  /// query trace.
  PeerFind(P::Key,Option<Query<P,V>>, QueryConfMsg<P>),
  /// Find a value by its key. Similar to `PeerPing`
  KVFind(V::Key,Option<Query<P,V>>, QueryConfMsg<P>), // option on query node indicate if it is local or not(must known for asynch) // used for local find or proxied find, , first int is remaining hop, second is nbquery to propagate, query node is used to receive the reply
    // TODO check that option query is needed for kvfind (or just query)
  /// Shutdown peer cache.
  ShutDown,
  /// Ping n peer (usefull to initiate DHT or too many offline nodes)
  Refresh(usize),
  /// Manage counter of running query (currently only for proxy mode related route implementation
  /// where we should avoid peer with existing query (blocking)
  PeerQueryPlus(Arc<P>),
  /// Same as `PeerQueryPlus` but minus
  PeerQueryMinus(Arc<P>),
  /// Store peer depending on queryconf (locally plus possibly propagate)
  StoreNode(QueryConfMsg<P>,Option<Arc<P>>), // destination storage node must be in query conf
  /// Store value depending on queryconf (locally plus possibly propagate)
  StoreKV(QueryConfMsg<P>,Option<V>),
}

/// message for query management process
pub enum QueryMgmtMessage<P : Peer, V : KeyVal + Send> {
  /// add a new query to query cache
  NewQuery(QueryID, Query<P,V>, Arc<Semaphore>), // store a new query, mutex to synchro when done
  /// get a reply to a query : either peer or keyval
  NewReply(QueryID,(Option<Arc<P>>,Option<V>)), // all reply checking, opening ... must be done before
  /// run a clean cache
  PerformClean,
}

/// message for key value store process
pub enum KVStoreMgmtMessage<P : Peer, V : KeyVal> {
  /// add a keyval, with optional reply and possible propagate based on queryconf
  KVAddPropagate(V,Option<OneResult<bool>>, QueryConfMsg<P>), // Note that the KeyVal rather be of type Arc<something> otherwhise lot of clone involved - query conf is used to propagate the add
  /// add a keyval, with optional reply, and given storing config
  KVAdd(V,Option<OneResult<bool>>, (bool, Option<CachePolicy>)), // Note that the KeyVal rather be of type Arc<something> otherwhise lot of clone involved - query conf is used to propagate the add
  /// look up in stor for a value, reply through optinal query or queryconf reply, propagate like
  /// KVAddPropagate.
  KVFind(V::Key,Option<Query<P,V>>, QueryConfMsg<P>), // option on query node indicate if it is local or not(must known for asynch) // if result and query return through query else send to peermanager for proxy
  /// like kvadd, act locally only
  KVFindLocally(V::Key,Option<OneResult<Option<V>>>),
    // for simple local find only
//    KVLocalFind(V::Key, (Condvar,Mutex<V>)),
  Shutdown,
  Commit,
}

/// message for client process
pub enum ClientMessage<P : Peer, V : KeyVal> {
  /// Ping a peer (establishing connection and storing peer in peer manager
  PeerPing(Arc<P>),
  /// Find a peer, option on query depends on query mode 
  PeerFind(P::Key,Option<Query<P,V>>, QueryConfMsg<P>),
  /// find a `KeyVal`
  KVFind(V::Key,Option<Query<P,V>>, QueryConfMsg<P>),
  /// reply to a find peer or propagate
  StoreNode(QueryConfMsg<P>, Option<Arc<P>>), // proxy a request and do some control if needed
  /// reply to a find keyval or propagate
  StoreKV(QueryConfMsg<P>, Option<V>), // proxy a request and do some control if needed
  /// shutdown client process
  ShutDown, // when issue with receiving on channel or other, use this to drop your client
}


