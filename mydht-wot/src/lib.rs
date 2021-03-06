
#![feature(convert)]
#![feature(associated_consts)]
#![feature(slice_bytes)]
#![feature(deque_extras)] // TODO could be removed

//! Web of trust base components.
#[macro_use] extern crate log;
#[macro_use] extern crate mydht_base;
extern crate rustc_serialize;
extern crate time;
extern crate num;
extern crate bincode;
extern crate rand;

use bincode::rustc_serialize as bincodeser;
use bincode::SizeLimit;

//use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};
//use std::path::{Path,PathBuf};
use mydht_base::keyval::{KeyVal};
use mydht_base::peer::Peer;
use mydht_base::utils::ArcKV;
use mydht_base::utils::TimeSpecExt;

#[cfg(feature="openssl-impl")]
pub mod rsa_openssl;
#[cfg(feature="rust-crypto-impl")]
pub mod ecdsapeer;
pub mod truststore;
pub mod trustedpeer;
pub mod classictrust;
pub mod exptrust;

pub trait WotTrust<P : KeyVal<Key = Vec<u8>>> : KeyVal<Key = Vec<u8>> {
  type Rule : Send;
  fn new(p : &P, rules : &Self::Rule) -> Self;

  /// first return bool tells if wottrust change (so we update it in kvstore (no inplace kvstore
  /// update for now), second one tell if trust has changed (so we apply cascade chain of
  /// updates).
  fn update (&mut self, from_old_level : u8, from_old_trust : u8, from_new_level : u8, from_new_trust : u8, rules : &Self::Rule) -> (bool,bool);
  fn trust (&self) -> u8;
  fn lastdiscovery (&self) -> TimeSpecExt;
}





/// A peer with trust info, if we own the peer, a private key is defined.
/// This is the content stored and exchanged.
/// This is just a `trait alias`.
pub trait TrustedPeer : Peer<Key = Vec<u8>> + TrustedVal<Self,PeerInfoRel> + Truster {}

pub trait TrustedVal<T : Truster, R : TrustRel> : KeyVal {
//  type SignedContent : Encodable + 'static ;
//  fn get_sign_content<'a> (&'a self) -> Self::SignedContent;
  fn get_sign_content (& self) -> Vec<u8>;
  fn get_sign<'a> (&'a self) -> &'a Vec<u8>;
  fn get_from<'a> (&'a self) -> &'a Vec<u8>;
  /// return identifier of the kind of trust default to empty Vec for 
  fn get_about<'a> (&'a self) -> &'a Vec<u8>;

  fn check_val (&self, from : &T, about : &R) -> bool {
    if self.get_from() == &from.get_key() {
      let tosign = &self.get_sign_content();
      let tocheckenc = bincodeser::encode(&(about.get_rep(),tosign), SizeLimit::Infinite).unwrap();
      from.content_check(tocheckenc.as_slice(), self.get_sign().as_slice())
    } else {
      false
    }
  }

  //fn sign_val<EC : Encodable> (from : &T, about : &R, cont : &EC) -> Vec<u8> 
  fn sign_val (from : &T, about : &R, cont : &Vec<u8>) -> Vec<u8> {
    from.content_sign(bincodeser::encode(&(about.get_rep(),cont), SizeLimit::Infinite).unwrap().as_slice())
  }

  /// same as sign_val but for technical usage when Trustor is not yet totally initiated
  fn init_sign_val (from : &T::Internal, about : &[u8], cont : &Vec<u8>) -> Vec<u8> {
  //fn init_sign_val<EC : Encodable> (from : &T::Internal, about : &[u8], cont : &EC) -> Vec<u8> 
    <T as Truster>::init_content_sign(from, bincodeser::encode(&(about,cont), SizeLimit::Infinite).unwrap().as_slice())
  }

  // TODO evolution to include get_sign value in the key creation -> add a method init key (maybe
  // in another trait).
 
}

// TODO if needed parameterized over a TrustedVal to allow multiple sign or check
/// a content which can sign and check other content, a trust provider
pub trait Truster : KeyVal<Key = Vec<u8>> {
  type Internal : 'static;
  fn content_sign (&self, to_sign : &[u8]) -> Vec<u8>;
  fn init_content_sign (&Self::Internal, to_sign : &[u8]) -> Vec<u8>;
  fn content_check (&self, tocheckenc : &[u8], sign : &[u8]) -> bool;
  /// check some truster content eg if id is a hash of public sign
  fn key_check(&self) -> bool;
}

/// link a truster and a trusted with semantic
/// It could be a KeyVal (dynamic trust) or a phantom type (statically defined).
pub trait TrustRel {
  /// get relation print - an id to include in a signature for relation identity
  /// return identifier of the kind of trust
  fn get_rep(&self) -> Vec<u8>;

}

impl<KV : KeyVal<Key = Vec<u8>>> TrustRel for KV {
  #[inline]
  fn get_rep (&self) -> Vec<u8> {
    self.get_key()
  }
}

// TODO static no content TrustRel
pub struct UnTypeRel;
impl TrustRel for UnTypeRel {
  #[inline]
  fn get_rep(&self) -> Vec<u8> {
    Vec::new()
  }
}

/// Static Peer signing peer (autosign of our own info)
pub struct PeerInfoRel;
impl TrustRel for PeerInfoRel {
  #[inline]
  fn get_rep(&self) -> Vec<u8> {
    vec![0]
  }
}

/// Static Peer signing its trust of another peer
pub struct PeerTrustRel;
impl TrustRel for PeerTrustRel {
  #[inline]
  fn get_rep(&self) -> Vec<u8> {
    vec![1]
  }
}




impl<T : Truster, R : TrustRel, KV : TrustedVal<T,R>> TrustedVal<T,R> for ArcKV<KV> {
  //type SignedContent = <KV as TrustedVal<T,R>>::SignedContent;
  #[inline]
  //fn get_sign_content<'b> (&'b self) -> <KV as TrustedVal<T,R>>::SignedContent {
  fn get_sign_content (& self) -> Vec<u8> {
    self.0.get_sign_content()
  }
  #[inline]
  fn get_sign<'b> (&'b self) -> &'b Vec<u8> {
    self.0.get_sign()
  }
  #[inline]
  fn get_from<'b> (&'b self) -> &'b Vec<u8> {
    self.0.get_from()
  }
  #[inline]
  fn get_about<'b> (&'b self) -> &'b Vec<u8> {
    self.0.get_about()
  }
}

impl<KV : Truster> Truster for ArcKV<KV> {
  type Internal = <KV as Truster>::Internal;
  #[inline]
  fn content_sign (&self, to_sign : &[u8]) -> Vec<u8> {
    self.0.content_sign(to_sign)
  }
  fn init_content_sign (pk : &<KV as Truster>::Internal, to_sign : &[u8]) -> Vec<u8> {
    <KV as Truster>::init_content_sign(pk, to_sign)
  }
  fn content_check (&self, tocheckenc : &[u8], sign : &[u8]) -> bool {
    self.0.content_check(tocheckenc,sign)
  }
  fn key_check (&self) -> bool {
    self.0.key_check()
  }
}


