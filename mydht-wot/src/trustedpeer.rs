//! Trusted peer implementation.


use bincode::rustc_serialize as bincode;
use bincode::SizeLimit;

use rustc_serialize::{Encoder,Encodable,Decoder};
use mydht_base::keyval::{KeyVal};
use mydht_base::utils::ArcKV;
use mydht_base::transport::SerSocketAddr;
use mydht_base::utils::{TimeSpecExt};
use mydht_base::keyval::{Attachment,SettableAttachment};
use super::{TrustedPeer,TrustedVal,PeerTrustRel};


#[derive(RustcEncodable,Debug)]
pub struct TrustedPeerToSignEnc<'a> {
  // to allow multiple encoding in time
  pub version : u8,
  pub name : &'a String,
  pub date : &'a TimeSpecExt,
}

#[derive(RustcDecodable)]
pub struct TrustedPeerToSignDec {
  pub version : u8,
  pub name : String,
  pub date : TimeSpecExt,
}

#[derive(RustcEncodable)]
pub struct SendablePeerEnc<'a> {
  pub key : &'a Vec<u8>,
  pub publickey : &'a Vec<u8>,
  pub name : &'a String,
  pub date : &'a TimeSpecExt,
  pub peersign : &'a Vec<u8>,
  pub addressdate : &'a TimeSpecExt,
  pub address : &'a SerSocketAddr,
}

#[derive(RustcDecodable)]
pub struct SendablePeerDec {
  pub key : Vec<u8>,
  pub publickey : Vec<u8>,
  pub name : String,
  pub date : TimeSpecExt,
  pub peersign : Vec<u8>,
  pub addressdate : TimeSpecExt,
  pub address : SerSocketAddr,
}

/// Trust signing from one node to another. Key is signature (require something to keep relation
/// with its components), it should be some cat of from, about and its tag, but querying for all 
/// possible tag is not very practical
#[derive(Debug, PartialEq, Eq, Clone,RustcEncodable,RustcDecodable)]
pub struct PeerSign<TP : TrustedPeer> {
  /// (peer signing, peer to sign) aka (from, about)
  key : (<TP as KeyVal>::Key, <TP as KeyVal>::Key),
  /// peer trust
  pub trust : u8,
  /// version to update/revoke...
  pub tag : usize,
  /// signature
  pub sign : Vec<u8>,
}

impl<TP : TrustedPeer> PeerSign<TP> {
  #[inline]
  pub fn from<'a>(&'a self) -> &'a <ArcKV<TP> as KeyVal>::Key {
    &self.key.0
  }
  #[inline]
  pub fn about<'a>(&'a self) -> &'a <ArcKV<TP> as KeyVal>::Key {
    &self.key.1
  }


  /// sign an new trust level, from peer must be a peer for whom we got the privatekey (ourselves
  /// for instance). TODO remove those ArcKV
  pub fn new (fromp : &ArcKV<TP>, aboutp : &ArcKV<TP>, trust : u8, tag : usize) -> Option<PeerSign<TP>> {
    // Note that we use bincode but if serializing scheme change, we will be lost, so we add an
    // encoding version in first position (currently serialized as is)
    let from  = fromp.get_key();
    let about = aboutp.get_key();
    let vsign = {
      let tosign = bincode::encode(&
       
      (&from, &about, trust, tag)

, SizeLimit::Infinite).unwrap()
      ;
      <Self as TrustedVal<TP, PeerTrustRel>>::sign_val(fromp, &PeerTrustRel, &tosign)
    };
    debug!("sign : {:?}", vsign);
    // current openssl if is strange
    if vsign.len() == 0 {
      error!("trying to sign with no private key");
      None
    } else {
      Some(
        PeerSign {
          key : (from, about), 
          trust : trust,
          tag   : tag,
          sign  : vsign,
      })
    }
  }
}

impl<TP : TrustedPeer> KeyVal for PeerSign<TP> {
  // a pair of from and about ids
  type Key = (<TP as KeyVal>::Key, <TP as KeyVal>::Key);
  #[inline]
  fn get_key(&self) -> (<TP as KeyVal>::Key, <TP as KeyVal>::Key) {
    self.key.clone()
  }
 /* 
  #[inline]
  fn get_key_ref<'a>(&'a self) -> &'a (<TP as KeyVal>::Key, <TP as KeyVal>::Key) {
    &self.key
  }*/
  noattachment!();
}

impl<TP : TrustedPeer> SettableAttachment for PeerSign<TP> {}

impl<'a, TP : TrustedPeer> TrustedVal<TP, PeerTrustRel> for PeerSign<TP> {
//  type SignedContent = (&'a Vec<u8>, &'a Vec<u8>, u8, usize);
  //fn get_sign_content<'b> (&'b self) -> (&'b Vec<u8>, &'b Vec<u8>, u8, usize) {
  fn get_sign_content (& self) -> Vec<u8> {
   bincode::encode(
     & (self.from(), self.about(), self.trust, self.tag)
, SizeLimit::Infinite).unwrap()
  }
  #[inline]
  fn get_sign<'b> (&'b self) -> &'b Vec<u8> {
    &self.sign
  }

  #[inline]
  fn get_from<'b> (&'b self) -> &'b Vec<u8> {
    self.from()
  }

  #[inline]
  /// static about (PeerTrustRel), calling this will panic
  /// It shall return a key (requires to implement static declaration of semantic)
  fn get_about<'b> (&'b self) -> &'b Vec<u8> {
    panic!("This trusted val use a statically define semantic, get_about is not implemented")
  }

}


