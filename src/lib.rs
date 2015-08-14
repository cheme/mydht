#![feature(custom_derive)]
#![feature(core)]
#![feature(fs_walk)]
#![feature(path_ext)]
#![feature(tcp)]
#![feature(convert)]
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
extern crate rand;

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
    /*
#[inline]
    fn get_key_ref<'a>(&'a self) -> &'a $k {
      match self {
       $( &$kv::$st(ref f)  => $k::$st(f.get_key_ref()), )*
      }
    }
    */


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
    /*
#[inline]
    fn get_key_ref<'a>(&'a self) -> &'a $kt {
      match self {
       $( &$kv::$st(ref f)  => $k::$st(f.get_key_ref()), )*
      }
    }
*/

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
    fn has_val(& self, k : &$k) -> bool {
      match k {
        $( &$k::$st(ref sk) =>
         self.$ksub.has_val(sk), )*
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
mod kvcache;
mod peer;
mod procs;
mod query;
mod route;
mod transport;
mod kvstore;
mod msgenc;
pub mod utils;
pub mod rules;
pub mod wot;
#[cfg(test)]
mod test;

// reexport
pub use peer::{PeerPriority,PeerState};
pub use procs::{DHT, RunningContext, RunningProcesses, ArcRunningContext, RunningTypes};
pub use procs::{store_val, find_val, find_local_val};
pub use query::{QueryConf,QueryPriority,QueryMode,QueryChunk,LastSentConf};
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
pub use query::{QueryID,Query};

pub mod dhtimpl {
  pub use peer::node::{Node};
  #[cfg(feature="openssl-impl")]
  pub use wot::rsa_openssl::RSAPeer;
  #[cfg(feature="rust-crypto-impl")]
  pub use wot::ecdsapeer::ECDSAPeer;

  pub use wot::trustedpeer::PeerSign;
  pub use kvcache::{NoCache};
  pub use query::simplecache::{SimpleCache,SimpleCacheQuery};
  pub use route::inefficientmap::{Inefficientmap};

  #[cfg(feature="dht-route")]
  pub use route::btkad::{BTKad};
  pub use keyval::{FileKV};
  pub use kvstore::filestore::{FileStore};
  pub use wot::truststore::{WotKV,WotK,WotStore};
  pub use wot::classictrust::{TrustRules,ClassicWotTrust};

  pub use rules::simplerules::{DhtRules,SimpleRules};

}
pub mod queryif{
  pub use query::cache::{QueryCache,CachePolicy};
}
pub mod dhtif{
  pub use mydhtresult::{Result,Error,ErrorKind};
  pub use keyval::{KeyVal,FileKeyVal,Key};
  pub use rules::DHTRules;
  pub use peer::{Peer,PeerMgmtMeths};
  pub use procs::{ClientMode,ServerMode};
}
pub mod kvstoreif{
  pub use kvcache::{KVCache};
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

#[cfg(test)]
pub mod testexp {
  pub mod common {
    pub use test::*;
  }
  pub mod transport {
    pub use transport::test::*;
  }
}


mod mydhtresult {

use std::fmt::Result as FmtResult;
use std::fmt::{Display,Formatter};
use std::error::Error as ErrorTrait;
use std::io::Error as IOError;
use byteorder::Error as BOError;
use std::sync::mpsc::SendError;
//use std::marker::Reflect;
use bincode::EncodingError as BincError;
use bincode::DecodingError as BindError;
use std::result::Result as StdResult;

#[derive(Debug)]
pub struct Error(pub String, pub ErrorKind, pub Option<Box<ErrorTrait>>);

/*pub fn from_io_error<T>(r : StdResult<T, IOError>) -> Result<T> {
  r.map_err(|e| From::from(e))
}*/


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
/* TODO when possible non conflicting imple
impl<T: Send + Reflect> From<SendError<T>> for Error {
  #[inline]
  fn from(e : SendError<T>) -> Error {
    Error(e.description().to_string(), ErrorKind::ChannelSendError, Some(Box::new(e)))
  }
}
*/
impl<T> From<SendError<T>> for Error {
  #[inline]
  fn from(e : SendError<T>) -> Error {
    Error(e.to_string(), ErrorKind::ChannelSendError,None)
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
  ExpectedError,
  ChannelSendError,
  RouteError,
}

/// Result type internal to mydht
pub type Result<R> = StdResult<R,Error>;

}


