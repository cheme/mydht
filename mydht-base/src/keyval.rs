
//! KeyVal common traits
use std::path::{PathBuf};
use serde::{Serializer,Serialize,Deserialize,Deserializer};
use serde::de::{DeserializeOwned};
use std::fmt;
//use num::traits::ToPrimitive;

/// Non serialize binary attached content. For now only a file, as store, since very limited usage.
/// This is usefull depending on msg encoding implementation, obviously when implementation does
/// not use Read/Write interface, message is fully in memory and big content should be serialize as
/// an attachment : a linked file (if multiple content, use tmp file then split).
/// Even with well implemented serializer, it could be usefull to use attachment because it is
/// transmitted afterward and a deserialize error could be catch before transmission of all
/// content.
pub type Attachment = PathBuf; // TODO change to Path !!! to allow copy ....


//pub trait Key : fmt::Debug + Hash + Eq + Clone + Send + Sync + Ord + 'static{}
pub trait Key : Serialize + DeserializeOwned + fmt::Debug + Eq + Clone + 'static + Send + Sync {
//  fn key_encode(&self) -> MDHTResult<Vec<u8>>;
// TODO 
//fn as_ref<KR : KeyRef<Key = Self>>(&'a self) -> KR;
}

// TODO get keyref in keyval as parametric function (keyref could not be associated type as not
// 'static).
// TODO without associated lifetime, passing lifetime to all keyval is to heavy and conversion
// trait is to hacky
pub trait KeyRef : Serialize + fmt::Debug + Eq + Clone {
}

impl<'a> KeyRef for &'a [u8] {
}

impl<'a> KeyRef for &'a str {
}

impl<'a, K : Key> KeyRef for &'a K {
}

impl<'a, K1 : KeyRef, K2 : KeyRef> KeyRef for (K1,K2) {
}

/*
impl<'a, K : Key> AsKeyRef<'a> for &'a K {
  type KeyRef = Self;
  fn as_keyref(&'a self) -> Self::KeyRef {
    self
  }
}
*/

// TODO remove for 'as_key_ref'
impl<K : Serialize + DeserializeOwned + fmt::Debug + Eq + Clone + 'static + Send + Sync> Key for K {
 /* fn key_encode(&self) -> MDHTResult<Vec<u8>> {
    Ok(try!(bincode::encode(self, bincode::SizeLimit::Infinite)))
  }*/
}

pub trait GettableAttachments {
  /// TODO switch to iter
  fn get_attachments(&self) -> Vec<&Attachment>;

}
/// KeyVal is the basis for DHT content, a value with key.
// TODO rem 'static and add it only when needed (Arc) : method as_static??
// TODO rem Serialize from trait, + remove encode_kv and decode_kv : MsgEnc do not use serialize,
// its implementation does!!
//pub trait KeyVal : fmt::Debug + Clone + Send + Sync + Eq + SettableAttachment + 'static {
pub trait KeyVal : Serialize + DeserializeOwned + fmt::Debug + Clone + Send + Sync + Eq + SettableAttachment + 'static {
  /// Key type of KeyVal
  type Key : Key + Send + Sync; //aka key // Ord , Hash ... might be not mandatory but issue currently
  /// return size of attachment to download, default implementation return 0 (safe) as a way to
  /// disable attachment by default
  fn attachment_expected_size(&self) -> usize {
    0
  }
  /// getter for key value
  fn get_key(&self) -> Self::Key;
  /* Get key ref usage is of for type like enum over multiple keyval or other key with calculated
  // super type we need more generic way to derive and then keyref should be doable
  // Other possible refactoring would be to add lifetime to KeyVal (then key should be either &'a
  // actual val or Container<'a>(&'a subkey). TODO rethink it (even post a question for deriving
  // strategie + askvif (limit )...
  #[inline]
  fn get_key(&self) -> Self::Key {
    self.get_key_ref().clone()
  }
  /// getter for key value
  fn get_key_ref<'a>(&'a self) -> &'a Self::Key;
  */
  /// optional attachment
  fn get_attachment(&self) -> Option<&Attachment>;
/*
  /// default serialize encode of keyval is used at least to store localy without attachment,
  /// it could be specialize with the three next variant (or just call from).
  fn encode_dist_with_att<S:Serializer> (&self, s: &mut S) -> Result<(), S::Error>;
  fn decode_dist_with_att<D:Deserializer> (d : &mut D) -> Result<Self, D::Error>;
  fn encode_dist<S:Serializer> (&self, s: &mut S) -> Result<(), S::Error>;
  fn decode_dist<D:Deserializer> (d : &mut D) -> Result<Self, D::Error>;
  fn encode_loc_with_att<S:Serializer> (&self, s: &mut S) -> Result<(), S::Error>;
  fn decode_loc_with_att<D:Deserializer> (d : &mut D) -> Result<Self, D::Error>;
*/
  /// serialize function for
  /// default to nothing specific (reuse of serialize implementation)
  /// When specific treatment, serialize implementation should reuse this with local and no
  /// attachment.
  /// First boolean indicates if the encoding is locally used (not send to other peers).
  /// Second boolean indicates if attachment must be added in the encoding (or just a reference
  /// kept).
  /// Default implementation encode through encode trait.
  fn encode_kv<S:Serializer> (&self, s: S, _ : bool, _ : bool) -> Result<S::Ok, S::Error> {
    self.serialize(s)
  }
  /// First boolean indicates if the encoding is locally used (not send to other peers).
  /// Second boolean indicates if attachment must be added in the encoding (or just a reference
  /// kept).
  /// Default implementation decode through encode trait.
  fn decode_kv<'de,D:Deserializer<'de>> (d : D, _ : bool, _ : bool) -> Result<Self, D::Error> {
    Self::deserialize(d)
  }
}

/*
/// default serialize encode of keyval is used at least to store localy without attachment,
impl<KV : KeyVal> Serialize for KV {
#[inline]
  fn encode<S:Serializer> (&self, s: &mut S) -> Result<(), S::Error> {
    self.encode_kv(s, true, false)
  }
}

/// default serialize encode of keyval is used at least to store localy without attachment,
impl<KV : KeyVal> Deserialize for KV {
#[inline]
  fn decode<D:Deserializer> (d : &mut D) -> Result<KV, D::Error> {
    KV::decode_kv(d, true, false);
  }
}
*/

/// This trait has been extracted from keyval, because it is essentially use for KeyVal init,
/// but afterwards KeyVal should be used as trait object. Furthermore in some case it simplify
/// KeyVal derivation of structure by allowing use of `AsKeyValIf` :
/// ``` impl<KV : Keyval> KeyVal for AsKeyValIf<KV> ```
/// It also make trivial in most case (no attachment support) the implementation.
pub trait SettableAttachment {

  /// optional attachment
  fn set_attachment(& mut self, &Attachment) -> bool {
    // default to no attachment support
    false
  }
}
pub trait SettableAttachments {
  // TODO replace by iter over getable attachments
  fn attachment_expected_sizes(&self) -> Vec<usize>;
  /// optional attachment
  fn set_attachments(& mut self, &[Attachment]) -> bool;
}

/*
/// currently this could only be used for type ,
/// If/when trait object could include associated type def, as_key_val_if should return &KeyVal,
/// and this could be extend to derivation of enum (replacement for derive_keyval macro).
pub trait AsKeyValIf : fmt::Debug + Clone + Send + Sync + Eq + SettableAttachment {
  type KV : KeyVal;
  type BP;
  fn as_keyval_if(& self) -> & Self::KV;
  fn build_from_keyval(Self::BP, Self::KV) -> Self;
  fn encode_bef<S:Serializer> (&self, s: &mut S, is_local : bool, with_att : bool) -> Result<(), S::Error> { Ok(()) }
  fn decode_bef<D:Deserializer> (d : &mut D, is_local : bool, with_att : bool) -> Result<Self::BP, D::Error>;
}
*/
// Library adapter to convert AsRef
//pub struct AsRefSt<'a, KV : 'a> (&'a KV);
/*
impl<'a, KV : KeyVal, BP, AKV : AsKeyValIf< KV = KV, BP = BP>> AsRef<KV> for AsRefSt<'a, AKV> {
  fn as_ref (&self) -> &KV {
    self.0.as_keyval_if()
  }
}
*/
/*
impl<AKV : AsKeyValIf> Serialize for AKV {
  fn encode<S:Serializer> (&self, s: &mut S) -> Result<(), S::Error> {
    try!(self.encode_bef(true, false));
    self.as_keyval_if().encode(s);
  }
}
impl<AKV : AsKeyValIf> Deserialize for AKV {
  fn decode<D:Deserializer> (d : &mut D) -> Result<AKV, D::Error> {
    let bp = try!(<AKV as AsKeyValIf>::decode_bef(d, true, false));
    <<AKV as AsKeyValIf>::KV as Deserialize>::decode(d).map(|r|Self::build_from_keyval(r, bp))
  }
}
*/
/*
impl<AKV : AsKeyValIf + Serialize + Deserialize> KeyVal for AKV 
{
  type Key = <<AKV as AsKeyValIf>::KV as KeyVal>::Key;

  #[inline]
  fn get_key(&self) -> Self::Key {
    self.as_keyval_if().get_key()
  }
  #[inline]
  fn get_attachment(&self) -> Option<&Attachment> {
    self.as_keyval_if().get_attachment() 
  }
  #[inline]
  fn encode_kv<S:Serializer> (&self, s: &mut S, is_local : bool, with_att : bool) -> Result<(), S::Error> {
    try!(self.encode_bef(s, true, false));
    self.as_keyval_if().encode_kv(s, is_local, with_att)
  }
  #[inline]
  fn decode_kv<D:Deserializer> (d : &mut D, is_local : bool, with_att : bool) -> Result<Self, D::Error> {
    let bp = try!(<AKV as AsKeyValIf>::decode_bef(d, true, false));
    <<AKV as AsKeyValIf>::KV as KeyVal>::decode_kv(d, is_local, with_att).map(|r|Self::build_from_keyval(bp,r))
  }

}

*/
/// Specialization of Keyval for FileStore
pub trait FileKeyVal : KeyVal {
  /// initiate from a file (usefull for loading)
  fn from_path(PathBuf) -> Option<Self>;
  /// name of the file
  fn name(&self) -> String;
  /// get attachment
  fn get_file_ref(&self) -> &Attachment {
    self.get_attachment().unwrap()
  }
}


