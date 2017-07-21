
#![feature(associated_consts)]
#![feature(box_patterns)]

#[macro_use] extern crate log;
extern crate rustc_serialize;
extern crate serde;
extern crate time;
extern crate num;
extern crate rand;
extern crate bit_vec;
extern crate byteorder;
extern crate bincode;
extern crate readwrite_comp;
/*
#[macro_export]
/// static buffer
macro_rules! static_buff {
  ($bname:ident, $bname_size:ident, $bsize:expr, $btype:ty, $bdefval:expr ) => (
    const $bname_size : usize = $bsize;
    static $bname : &'static mut [$btype; $bsize] = &mut [$bdefval; $bsize];
  )
}
*/

#[macro_export]
/// a try which use a wrapping type
macro_rules! tryfor(($ty:ident, $expr:expr) => (

  try!(($expr).map_err(|e|$ty(e)))

));
 
#[macro_export]
/// Automatic define for KeyVal without attachment
macro_rules! noattachment(() => (
  fn get_attachment(&self) -> Option<&Attachment>{
    None
  }
));

#[macro_export]
/// convenience macro for peer implementation without a shadow
macro_rules! noshadow(() => (

  type Shadow = NoShadow;
  #[inline]
  fn get_shadower (&self, _ : bool) -> Self::Shadow {
    NoShadow
  }
  #[inline]
  fn default_auth_mode(&self) -> <Self::Shadow as Shadow>::ShadowMode {()}
  #[inline]
  fn default_message_mode(&self) -> <Self::Shadow as Shadow>::ShadowMode {()}
  #[inline]
  fn default_header_mode(&self) -> <Self::Shadow as Shadow>::ShadowMode {()}


));


#[macro_export]
/// derive Keyval implementation for simple enum over * KeyVal
/// $kv is enum name
/// $k is enum key name
/// $ix is u8 index use to serialize this variant
/// $st is the enum name to use for this variant
/// $skv is one of the possible subKeyval type name in enum
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

#[macro_export]
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

#[macro_export]
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




pub mod utils;
pub mod keyval;
pub mod kvstore;
pub mod simplecache;
pub mod peer;
pub mod kvcache;
pub mod transport;
pub mod mydhtresult;
pub mod msgenc;
pub mod query;
pub mod procs;
pub mod rules;
pub mod route;
pub mod bytes_wr;
#[cfg(feature="tunnel-impl")]
pub mod tunnel;
