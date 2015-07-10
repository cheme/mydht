#![feature(int_uint)]
#![feature(custom_derive)]
#![feature(core)]
#![feature(io)]
#![feature(collections)]
#![feature(std_misc)]
#![feature(file_path)]
#![feature(fs_walk)]
#![feature(path_ext)]
#![feature(net)]
#![feature(os)]
#![feature(tcp)]
#![feature(convert)]
#![feature(alloc)]
#![feature(thread_sleep)]
#![feature(semaphore)]
#![feature(duration)]
#![feature(arc_unique)]
#![feature(deque_extras)]
#![feature(socket_timeout)]
#![feature(vecmap)] // in tcp_loop
#![feature(slice_bytes)] // in tcp_loop
#![feature(split_off)] // in tcp_loop
#[macro_use] extern crate log;
extern crate rustc_serialize;
extern crate time;
extern crate num;
extern crate bincode;
extern crate byteorder;

#[macro_export]
/// Automatic define for KeyVal without attachment
macro_rules! noattachment(() => (
  fn get_attachment(&self) -> Option<&Attachment>{
    None
  }
));

#[macro_export]
/// derive Keyval implementation for simple enum over * KeyVal
/// $kv is enum name
/// $k is enum key name
/// $ix is u8 index use to serialize this variant
/// $st is the enum name to use for this variant
/// $skv is one of the possible subKeyval type name in enum
/// TODO derive to something enpacking Arc (if we keep arc for kvstore) : to avoid clone in derive
/// kvstore!!!! (and in similar impl (see wotstore)).
macro_rules! derive_enum_keyval(($kv:ident {$($para:ident => $tra:ident , )*}, $k:ident, {$($ix:expr , $st:ident => $skv:ty,)*}) => (
// enum for values
  #[derive(RustcDecodable,RustcEncodable,Debug,PartialEq,Eq,Clone)]
  pub enum $kv<$( $para : $tra , )*> {
    $( $st($skv), )* // TODO put Arc to avoid clone?? (or remove arc from interface)
  }

// enum for keys
  #[derive(RustcDecodable,RustcEncodable,Debug,PartialEq,Eq,Hash,Clone,PartialOrd,Ord)]
  pub enum $k {
    $( $st(<$skv as KeyVal>::Key), )*
  }
  
  // TODO split macro then use it splitted in wotstore  (for impl only)
  
  impl KeyVal for $kv {
    type Key = $k;
#[inline]
    fn get_key(&self) -> $k {
      match self {
       $( &$kv::$st(ref f)  => $k::$st(f.get_key()), )*
      }
    }

#[inline]
    fn encode_kv<S:Encoder> (&self, s: &mut S, is_local : bool, with_att : bool) -> Result<(), S::Error>{
      match self {
        $( &$kv::$st(ref f) => {try!(s.emit_u8($ix));f.encode_kv(s, is_local, with_att)}, )*
      }
    }
#[inline]
    fn decode_kv<D:Decoder> (d : &mut D, is_local : bool, with_att : bool) -> Result<$kv, D::Error>{
      let ix = try!(d.read_u8());
      match ix {
        $( $ix => <$skv as KeyVal>::decode_kv(d, is_local, with_att).map(|v|$kv::$st(v)), )*
        _  => panic!("error ix decode"),
      }
    }
#[inline]
    fn get_attachment(&self) -> Option<&Attachment>{
      match self {
        $( &$kv::$st(ref f)  => f.get_attachment(), )*
      }
    }
  }

  impl SettableAttachment for $kv {
#[inline]
    fn set_attachment(& mut self, fi:&Attachment) -> bool{
      match self {
        $( &mut $kv::$st(ref mut f)  => f.set_attachment(fi), )*
      }
    }
  }

));

// dup of derive enum keyval used due to lack of possibility for parametric types in current macro
macro_rules! derive_enum_keyval_inner(($kvt:ty , $kv:ident, $kt:ty, $k:ident, {$($ix:expr , $st:ident => $skv:ty,)*}) => (
#[inline]
    fn get_key(&self) -> $kt {
      match self {
       $( &$kv::$st(ref f)  => $k::$st(f.get_key()), )*
      }
    }
#[inline]
    fn encode_kv<S:Encoder> (&self, s: &mut S, is_local : bool, with_path : bool) -> Result<(), S::Error>{
      match self {
        $( &$kv::$st(ref f) => {try!(s.emit_u8($ix));f.encode_kv(s, is_local, with_path)}, )*
      }
    }
#[inline]
    fn decode_kv<D:Decoder> (d : &mut D, is_local : bool, with_path : bool) -> Result<$kvt, D::Error>{
      let ix = try!(d.read_u8());
      match ix {
        $( $ix => <$skv as KeyVal>::decode_kv(d, is_local, with_path).map(|v|$kv::$st(v)), )*
        _  => panic!("error ix decode"),
      }
    }
#[inline]
    fn get_attachment(&self) -> Option<&Attachment>{
      match self {
        $( &$kv::$st(ref f)  => f.get_attachment(), )*
      }
    }


));

macro_rules! derive_enum_setattach_inner(($kvt:ty , $kv:ident, $kt:ty, $k:ident, {$($ix:expr , $st:ident => $skv:ty,)*}) => (

#[inline]
    fn set_attachment(& mut self, fi:&Attachment) -> bool{
      match self {
        $( &mut $kv::$st(ref mut f)  => f.set_attachment(fi), )*
      }
    }

));


#[macro_export]
/// derive kvstore to multiple independant kvstore implementation
/// $kstore is kvstore name
/// $kv is multiplexed keyvalue name
/// $k is multipexed enum key name
/// $ksubs is substorename for this kind of key : use for struct
/// $sts is the keyval typename to use
/// $ksub is substorename for this kind of key : use for impl (keyval may differ if some in the
/// same storage)
/// $st is the enum name to use for this variant : use for impl
macro_rules! derive_kvstore(($kstore:ident, $kv:ident, $k:ident, 
  {$($ksubs:ident => $sts:ty,)*}, 
  {$($st:ident =>  $ksub:ident ,)*}
  ) => (
  pub struct $kstore {
    $( pub $ksubs : $sts,)*
  }
  impl KVStore<$kv> for $kstore {
    #[inline]
    fn add_val(& mut self, v : $kv, stconf : (bool, Option<CachePolicy>)){
      match v {
        $( $kv::$st(ref st) => self.$ksub.add_val(st.clone(), stconf), )*
      }
    }
    #[inline]
    fn get_val(& self, k : &$k) -> Option<$kv> {
      match k {
        $( &$k::$st(ref sk) =>
      self.$ksub.get_val(sk).map(|ask| $kv::$st((ask).clone())), )*
      }
    }
    #[inline]
    fn remove_val(& mut self, k : &$k) {
      match k {
        $( &$k::$st(ref sk) =>
      self.$ksub.remove_val(sk), )*
      }
    }
    #[inline]
    fn commit_store(& mut self) -> bool{
      let mut r = true;
      $( r = self.$ksub.commit_store() && r; )*
      r
    }
  }
));

mod keyval;
mod peer;
mod procs;
mod query;
mod route;
mod transport;
mod kvstore;
mod msgenc;
pub mod utils;
pub mod wot;

// reexport
pub use peer::{PeerPriority};
pub use procs::{DHT, RunningContext, RunningProcesses, ArcRunningContext, RunningTypes};
pub use procs::{store_val, find_val, find_local_val};
pub use query::{QueryConf,QueryPriority,QueryMode,QueryChunk};
pub use query::cache::{CachePolicy};
pub use kvstore::{StoragePriority};
pub use keyval::{Attachment,SettableAttachment};
// TODO move msgenc to mod dhtimpl
pub use msgenc::json::{Json};
//pub use msgenc::bencode::{Bencode};
pub use msgenc::bincode::{Bincode};
//pub use msgenc::bencode::{Bencode_bt_dht};
pub use transport::tcp::{Tcp};
pub use transport::udp::{Udp};
pub use wot::{TrustedVal,Truster,TrustedPeer};

pub mod dhtimpl {
  pub use peer::node::{Node};
  #[cfg(feature="openssl-impl")]
  pub use wot::rsa_openssl::RSAPeer;
  #[cfg(feature="rust-crypto-impl")]
  pub use wot::ecdsapeer::ECDSAPeer;

  pub use wot::trustedpeer::PeerSign;
  pub use query::simplecache::{SimpleCache,SimpleCacheQuery};
  pub use route::inefficientmap::{Inefficientmap};

  #[cfg(feature="dht-route")]
  pub use route::btkad::{BTKad};
  pub use keyval::{FileKV};
  pub use kvstore::filestore::{FileStore};
  pub use wot::truststore::{WotKV,WotK,WotStore};
  pub use wot::classictrust::{TrustRules,ClassicWotTrust};

}
pub mod queryif{
  pub use query::cache::{QueryCache,CachePolicy};
}
pub mod dhtif{
  pub use mydhtresult::{Result,Error,ErrorKind};
  pub use keyval::{KeyVal,FileKeyVal,Key};
  pub use rules::DHTRules;
  pub use peer::{Peer,PeerMgmtMeths};
  pub use query::{QueryID,Query};
  pub use procs::{ClientMode,ServerMode};
}
pub mod kvstoreif{
  //pub use kvstore::KVCache;
  pub use kvstore::{KVCache,KVStore2};
  pub use kvstore::{KVStore, KVStoreRel};
  pub use procs::mesgs::{KVStoreMgmtMessage};
}
pub mod routeif{
  pub use route::{Route};
}
pub mod transportif{
  pub use transport::{Transport,TransportStream};
}
pub mod msgencif{
  pub use msgenc::{MsgEnc};
}

mod mydhtresult {

use std::fmt::Result as FmtResult;
use std::fmt::{Display,Formatter};
use std::error::Error as ErrorTrait;
use std::io::Error as IOError;
use byteorder::Error as BOError;
use bincode::EncodingError as BincError;
use bincode::DecodingError as BindError;
use std::result::Result as StdResult;

#[derive(Debug)]
pub struct Error(pub String, pub ErrorKind, pub Option<Box<ErrorTrait>>);

#[inline]
pub fn from_io_error<T>(r : StdResult<T, IOError>) -> Result<T> {
  r.map_err(|e| From::from(e))
}


impl ErrorTrait for Error {
  
  fn description(&self) -> &str {
    &self.0
  }
  fn cause(&self) -> Option<&ErrorTrait> {
    match self.2 {
      Some(ref berr) => Some (&(**berr)),
      None => None,
    }
  }
}

impl From<IOError> for Error {
  #[inline]
  fn from(e : IOError) -> Error {
    Error(e.description().to_string(), ErrorKind::IOError, Some(Box::new(e)))
  }
}

impl From<BincError> for Error {
  #[inline]
  fn from(e : BincError) -> Error {
    Error(e.description().to_string(), ErrorKind::EncodingError, Some(Box::new(e)))
  }
}
impl From<BindError> for Error {
  #[inline]
  fn from(e : BindError) -> Error {
    Error(e.description().to_string(), ErrorKind::DecodingError, Some(Box::new(e)))
  }
}

impl From<BOError> for Error {
  #[inline]
  fn from(e : BOError) -> Error {
    Error(e.description().to_string(), ErrorKind::ByteOrderError, Some(Box::new(e)))
  }
}

impl Display for Error {

  fn fmt(&self, ftr : &mut Formatter) -> FmtResult {
    let kind = format!("{:?} : ",self.1);
    try!(ftr.write_str(&kind));
    try!(ftr.write_str(&self.0));
    match self.2 {
      Some(ref tr) => {
        let trace = format!(" - trace : {}", tr);
        try!(ftr.write_str(&trace[..]));
      },
      None => (),
    };
    Ok(())
  }
}

#[derive(Debug)]
pub enum ErrorKind {
  DecodingError,
  EncodingError,
  MissingFile,
  IOError,
  ByteOrderError,
}

/// Result type internal to mydht
pub type Result<R> = StdResult<R,Error>;

}

mod rules {
  use time::Duration;
  use query::QueryID;
  use query::QueryPriority;
  use kvstore::StoragePriority;
  use query::cache::CachePolicy;
  use transport::Transport;
  use procs::ClientMode;
  use procs::ServerMode;

/// Rules for DHT.
/// This is used to map priorities with actual query strategies.
/// Rules are designed as trait to allow more flexibility than conf, yet here it might be good to
/// have fast implementations.
/// In fact some info are related to DHT, this is more DHTRules than QueryRules (could be split in
/// two).
pub trait DHTRules : Sync + Send + 'static {
  /// create a query new id : use for asynch query (most of the time will simply be the key of the resource
  fn newid (&self) -> QueryID;
  /// Max number of hop for the query, the method is currently called in main peermgmt process, therefore it must be fast (a mapping, not a db access).
  fn nbhop (&self, QueryPriority) -> u8;
  /// Number of peers to transmit to at each hop, the method is currently called in main peermgmt process, therefore it must be fast (a mapping not a db access).
  fn nbquery (&self, QueryPriority) -> u8;
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



  /// Define if we require authentication, this way Ping/Pong challenge exchange could be skip and peers is
  /// immediatly stored.
  /// So if this function reply no, implementation of challenge, signmsg and checkmsg for
  /// peermgmtrules is useless
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


}
