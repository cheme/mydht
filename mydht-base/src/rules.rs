use time::Duration;
use query::QueryPriority;
use query::QueryMode;
use kvstore::StoragePriority;
use kvstore::CachePolicy;
//use transport::Transport;
use procs::ClientMode;
//use procs::ServerMode;
use num;
use num::traits::ToPrimitive;


/// Rules for DHT.
/// This is used to map priorities with actual query strategies.
/// Rules are designed as trait to allow more flexibility than conf, yet here it might be good to
/// have fast implementations.
/// In fact some info are related to DHT, this is more DHTRules than QueryRules (could be split in
/// two).
pub trait DHTRules : Sync + Send + 'static {
  /// Max number of hop for the query, the method is currently called in main peermgmt process, therefore it must be fast (a mapping, not a db access).
  fn nbhop (&self, QueryPriority) -> u8;
  /// Number of peers to transmit to at each hop, the method is currently called in main peermgmt process, therefore it must be fast (a mapping not a db access).
  fn nbquery (&self, QueryPriority) -> u8;
  /// Number of no reply requires to consider no result, default implementation is suitable for
  /// many transports, but some may require some tunning depending on network to avoid timeout (for
  /// exemple asynch that is here by default (depth ^ nb_proxy) * 2 / 3 (2 / 3 is a lot it should
  /// be log of depth).
  /// both u8 are result of nbhop and nbquery from rules.
  fn notfoundtreshold (&self, nbquer : u8, maxhop : u8, mode : &QueryMode) -> usize {
    match mode {
      &QueryMode::Asynch => {
        let max = num::pow(nbquer.to_usize().unwrap(), maxhop.to_usize().unwrap());
        if max > 2 {
          (max / 3) * 2
        } else {
          max
        }
      },
      _ => nbquer.to_usize().unwrap(),
    }
  }
  /// delay between to cleaning of cache query
  fn asynch_clean(&self) -> Option<Duration>; 
  /// get the lifetime of a query (before clean and possibly no results).
  fn lifetime (&self, prio : QueryPriority) -> Duration;
  /// get the storage rules (a pair with persistent storage as bool plus cache storage as possible
  /// duration), depending on we beeing query originator, query priority, query storage priority
  /// and possible estimation of the number of hop at this point, very important
  fn do_store (&self, islocal : bool, qprio : QueryPriority, sprio : StoragePriority, hopnb : Option <usize>) -> (bool,Option<CachePolicy>); // wether you need to store the keyval or not
  /// Most of the time return one, as when proxying we want to decrease by one hop the number of
  /// hop, but sometimes we may by random  this nbhop to be 0 (this way a received query with
  /// seemingly remaining number of hop equal to max number of hop mode may not be send by the
  /// query originator
  fn nbhop_dec (&self) -> u8;
  /// function defining tunnel length of tunnel querymode
  fn tunnel_length(&self, QueryPriority) -> u8;




  /// Define if we require authentication, this way Ping/Pong challenge exchange could be skip and peers is
  /// immediatly stored.
  /// So if this function reply no, implementation of challenge, signmsg and checkmsg for
  /// peermgmtrules is useless.
  /// Plus their will not be ping pong exchange in both way (no need to challenge twice).
  /// TODO not implemented (need to pass Peer info in each query)
  fn is_authenticated(&self) -> bool;
  // TODO option to do authentication on every message (with is_authenticated only adress is
  // subsequantly trusted).
  /// client mode to use for client process
  fn client_mode(&self) -> &ClientMode;

  /// server mode conf, used to define server mode in conjonction with transport definition
  /// see procs::server::resolve_server_mode and specific transport transport::Transport::do_spawn_rec
  /// implementations, and of course ServerMode definition.
  /// Return :
  /// - first usize is number of stream per threads or 0 if not multiplexed or
  /// - second usize is number of threads pool or 0 if not multiplexed or
  /// - third usize is possible timeout multiplicator (thread looping over blocking transport needs
  /// small timeout in transport and only after n timeout are seen as offline) or 0
  /// - fourth is optional duration for non multiplexed
  fn server_mode_conf(&self) -> (usize, usize, usize, Option<Duration>);
  
  /// Define how to handle accept for incomming peers.
  /// - return false if light : in this case accept is run from reception thread directly in
  /// reception processing and authentication if needed is done afterward (no useless auth).
  /// An example of light accept would be an open access or a simple map filtering forbidden nodes.
  /// - return true if heavy : in this case accept is run after authentication (on pong reception)
  /// and in a new thread (result passed to peermgmt through continuation).
  /// An example of heavy accept would be if we need to query an external entity to have our reply
  /// (or do some discovery on a web of trust).
  fn is_accept_heavy(&self) -> bool;

  /// tells if our routing functions (deciding which peers to use for a keyval query or peer
  /// lookup) are heavy.
  /// Routing is done from peermanager process which is pretty central. A route implementation is
  /// generally composed by a cache over PeerInfo (to get peers fastly when we know who to address)
  /// and a routing strategy which could be fast (for example a routing based on Hashing like for
  /// btkad) or slow (for example a web of trust with possibles lookup (route implementation is
  /// responsible to run it asynchronously in another thread through the heavy route designed interface)).
  /// - return false if light : peermanager queries route using direct interface (avoiding a
  /// continuation).
  /// - return true if heavy : route is queried through its heavy interface running result in a
  /// continuation and probably spawning a thread (it is the route implementation that should run
  /// thread (route is not thread safe so it must be in its inner implementation that we can do
  /// something)).
  ///
  /// first bool is for peer (node),
  /// second one is for keyval (query),
  /// third one is for pool (currently used to refresh n nodes) : most of the time light but could
  /// be more smarter than random
  fn is_routing_heavy(&self) -> (bool,bool,bool);
}



