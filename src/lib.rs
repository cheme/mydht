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
#[macro_use] extern crate log;
extern crate rustc_serialize;
extern crate time;
extern crate num;
extern crate bincode;

#[macro_export]
/// Automatic define for KeyVal without attachment
macro_rules! noattachment(() => (
  fn get_attachment(&self) -> Option<&Attachment>{
    None
  }
  fn set_attachment(& mut self, _ : &Attachment) -> bool{
    false
  }
));
 
#[macro_export]
/// Automatic define for KeyVal without specific encoding 
macro_rules! nospecificencoding(($t:ty) => (
  #[inline]
  fn encode_dist_with_att<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error>{
    self.encode(s)
  }
  #[inline]
  fn decode_dist_with_att<D:Decoder> (d : &mut D) -> Result<$t, D::Error>{
    Decodable::decode(d)
  }
  #[inline]
  fn encode_dist<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error>{
    self.encode(s)
  }
  #[inline]
  fn decode_dist<D:Decoder> (d : &mut D) -> Result<$t, D::Error>{
    Decodable::decode(d)
  }
  #[inline]
  fn encode_loc_with_att<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error>{
    self.encode(s)
  }
  #[inline]
  fn decode_loc_with_att<D:Decoder> (d : &mut D) -> Result<$t, D::Error>{
    Decodable::decode(d)
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
    fn encode_dist_with_att<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
      match self {
        $( &$kv::$st(ref f)  => {try!(s.emit_u8($ix));f.encode_dist_with_att(s)}, )*
      }
    }
#[inline]
    fn decode_dist_with_att<D:Decoder> (d : &mut D) -> Result<$kv, D::Error> {
      let ix = try!(d.read_u8());
      match ix {
        $( $ix => <$skv as KeyVal>::decode_dist_with_att(d).map(|v|$kv::$st(v)), )*
        _  => panic!("error ix decode"),
      }
    }
#[inline]
    fn encode_dist<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
      match self {
        $( &$kv::$st(ref f)  => {try!(s.emit_u8($ix));f.encode_dist(s)}, )*
      }
    }
#[inline]
    fn decode_dist<D:Decoder> (d : &mut D) -> Result<$kv, D::Error> {
      let ix = try!(d.read_u8());
      match ix {
        $( $ix => <$skv as KeyVal>::decode_dist(d).map(|v|$kv::$st(v)), )*
        _  => panic!("error ix decode"),
      }
    }
#[inline]
    fn encode_loc_with_att<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error>{
      match self {
        $( &$kv::$st(ref f) => {try!(s.emit_u8($ix));f.encode_loc_with_att(s)}, )*
      }
    }
#[inline]
    fn decode_loc_with_att<D:Decoder> (d : &mut D) -> Result<$kv, D::Error>{
      let ix = try!(d.read_u8());
      match ix {
        $( $ix => <$skv as KeyVal>::decode_loc_with_att(d).map(|v|$kv::$st(v)), )*
        _  => panic!("error ix decode"),
      }
    }
#[inline]
    fn get_attachment(&self) -> Option<&Attachment>{
      match self {
        $( &$kv::$st(ref f)  => f.get_attachment(), )*
      }
    }
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
    fn encode_dist_with_att<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
      match self {
        $( &$kv::$st(ref f)  => {try!(s.emit_u8($ix));f.encode_dist_with_att(s)}, )*
      }
    }
#[inline]
    fn decode_dist_with_att<D:Decoder> (d : &mut D) -> Result<$kvt, D::Error> {
      let ix = try!(d.read_u8());
      match ix {
        $( $ix => <$skv as KeyVal>::decode_dist_with_att(d).map(|v|$kv::$st(v)), )*
        _  => panic!("error ix decode"),
      }
    }
#[inline]
    fn encode_dist<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
      match self {
        $( &$kv::$st(ref f)  => {try!(s.emit_u8($ix));f.encode_dist(s)}, )*
      }
    }
#[inline]
    fn decode_dist<D:Decoder> (d : &mut D) -> Result<$kvt, D::Error> {
      let ix = try!(d.read_u8());
      match ix {
        $( $ix => <$skv as KeyVal>::decode_dist(d).map(|v|$kv::$st(v)), )*
        _  => panic!("error ix decode"),
      }
    }
#[inline]
    fn encode_loc_with_att<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error>{
      match self {
        $( &$kv::$st(ref f) => {try!(s.emit_u8($ix));f.encode_loc_with_att(s)}, )*
      }
    }
#[inline]
    fn decode_loc_with_att<D:Decoder> (d : &mut D) -> Result<$kvt, D::Error>{
      let ix = try!(d.read_u8());
      match ix {
        $( $ix => <$skv as KeyVal>::decode_loc_with_att(d).map(|v|$kv::$st(v)), )*
        _  => panic!("error ix decode"),
      }
    }
#[inline]
    fn get_attachment(&self) -> Option<&Attachment>{
      match self {
        $( &$kv::$st(ref f)  => f.get_attachment(), )*
      }
    }
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
pub use peer::{Peer, PeerPriority};
pub use procs::{DHT, RunningContext, RunningProcesses};
pub use procs::{store_val, find_val, find_local_val};
pub use query::{QueryConf,QueryPriority,QueryMode,QueryChunk};
pub use query::cache::{CachePolicy};
pub use kvstore::{StoragePriority, Attachment};
// TODO move msgenc to mod dhtimpl
pub use msgenc::json::{Json};
//pub use msgenc::bencode::{Bencode};
pub use msgenc::bincode::{Bincode};
//pub use msgenc::bencode::{Bencode_bt_dht};
pub use transport::tcp::{Tcp};
pub use transport::udp::{Udp};
pub use wot::{TrustedVal,Truster,TrustedPeer};
//pub use kvstore::nospecificencoding;
pub mod dhtimpl{
  pub use peer::node::{Node};
  pub use wot::rsa_openssl::RSAPeer;
  pub use wot::trustedpeer::PeerSign;
  pub use query::simplecache::{SimpleCache,SimpleCacheQuery};
  pub use route::inefficientmap::{Inefficientmap};
  pub use route::btkad::{BTKad};
  pub use kvstore::{FileKV};
  pub use kvstore::filestore::{FileStore};
  pub use wot::truststore::{WotKV,WotK,WotStore};
  pub use wot::classictrust::{TrustRules,ClassicWotTrust};
}
// TODO rename peerif to peerrulesif
pub mod peerif{
  pub use peer::{PeerMgmtRules};
}
pub mod queryif{
  pub use query::cache::{QueryCache,CachePolicy};
  pub use query::{QueryID,Query,QueryRules};
}
pub mod kvstoreif{
  //pub use kvstore::KVCache;
  pub use kvstore::{KVCache,KVStore2};
  pub use kvstore::{KVStore, KeyVal, FileKeyVal};
  pub use kvstore::{KVStoreRel, Key};
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


