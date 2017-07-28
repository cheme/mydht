//! RSAPeerImpl trait with only access to pkey and primitive
//! Plus Shadower
//! TODO


#[macro_use] extern crate mydht_base;
#[macro_use] extern crate serde_derive;
extern crate openssl;
extern crate time;
extern crate rand;
extern crate bincode;
extern crate mydht_openssl;
extern crate serde;
use serde::{Serializer,Serialize,Deserializer,Deserialize};

//use mydhtresult::Result as MDHTResult;
use std::io::Result as IoResult;
use openssl::pkey::{PKey};
use openssl::rsa::Rsa;
use openssl::sign::Signer;

use std::marker::PhantomData;
//use std::str::FromStr;
use std::sync::Arc;
use std::path::{PathBuf};
//use self::time::Timespec;

use mydht_base::keyval::{KeyVal};
use mydht_base::keyval::KeyVal as KVCont;
use mydht_base::transport::{SerSocketAddr};
use mydht_base::utils::{TimeSpecExt};
use mydht_base::keyval::{Attachment,SettableAttachment};
use mydht_base::peer::{Peer,ShadowBase};
#[cfg(feature="wot")]
use bincode::SizeLimit;
use mydht_openssl::rsa_openssl::PKeyExt;
use mydht_openssl::rsa_openssl::{
  OpenSSLConf,
  OSSLShadowerW,
  OSSLShadowerR,
  ASymSymMode,
};


/// a TrustedPeer using RSA 2048 openssl implementation with sha 512 on content and to derive id.
#[derive(Debug, PartialEq, Eq, Clone,Serialize,Deserialize)]
#[serde(bound(deserialize = ""))]
pub struct RSAPeer<RT : OpenSSLConf, I : KVCont> {
  /// key to use to identify peer, derived from publickey it is shorter
  key : Vec<u8>,
  /// is used as id/key TODO maybe two publickey use of a master(in case of compromition)
  publickey : PKeyExt<RT>,

  /// an associated complementary info
  name : String,

  /// TODO all other user published info see 

  /// date (when was those info associated), this is used to manage change of information.
  /// Yet it is obviously not really good, expect some lock information due to using wrongly set
  /// date and so on.
  /// Plus with current peer locate, it will look for new info only when looking for connected
  /// peer (not that much : on ping we just authenticate -> TODO need publishing info mechanism
  /// to run on connection established (in rules (now that accept has all this info it is easy to
  /// do in accept fn)).
  /// TODO for now this is somewhat fine. Yet it will be complicated to propagate a change.
  /// other mechanism :
  /// - change of key through a special signpeer (aka trust 0) : use a new peer. Pb how to
  /// distrust the old one : changing its trust level will likely repercute on the new one.
  /// We need some semantic to know old 0 is fine until new or is distrust because new 0...
  /// In regard to that 0 is not a good idea, we need a new kind of sign (revoke with new id),
  /// returned when looking for sign (see discovery).
  /// - contain two key, a master and a current use. To revoke a key it is ok, yet need to
  /// repropagate all old sign with new curent each time (or time to simplify trust).
  /// - connected info is allways the good one : on large network difficult to propagate.
  date : TimeSpecExt,

  /// Signature of this content (must be checked on reception (all transmitted info are signed
  /// (all but private key, trust, trustdate)) + no transient as no need , done by ping proto
  /// (accept...)
  peersign : Vec<u8>,

  /// additional signed contet
  signedextend : I,

  ////// transient info
  
  pub addressdate : TimeSpecExt,
  /// last known address.
  /// peers with incorrect address would not be returned in 
  /// findpeerrs but possibly return in findkv (for example in a wot
  /// findkv is to check content, while findpeers to communicate
  pub address : SerSocketAddr,

  pub extend : I,
  ////// local info 
  #[serde(skip)]
  conf : PhantomData<RT>,
}

#[cfg(feature="wot")]
impl<RT : OpenSSLConf, I : KVCont> TrustedPeer for RSAPeer<RT,I> {}

impl<RT : OpenSSLConf, I : KVCont> RSAPeer<RT,I> {

  // shitty method TODO do not use as is (to_sign could be big and no way to bufferize)
  pub fn sign_cont(pkey : &PKey, to_sign : &[u8]) -> IoResult<Vec<u8>> {
    let mut signer = Signer::new(RT::HASH_SIGN(), &pkey)?;
    signer.update(to_sign)?;
    Ok(signer.finish()?)
  }

  #[cfg(feature="wot")]
  fn to_trustedpeer_tosign<'a>(&'a self) -> TrustedPeerToSignEnc<'a> {
    TrustedPeerToSignEnc {
      version : 0,
      name : &self.name,
      date : &self.date,
    }
  }

 
  #[cfg(feature="wot")]
  pub fn inner_sign_peer (name : &String, date : &Date, extendsign : &I) -> IoResult<Vec<u8>> {
      let tosign = &TrustedPeerToSignEnc {
        version : 0,
        name : &name,
        date : &date,
        extend : &extendsign,
      };
      debug!("in create info : {:?}", tosign);
      <RSAPeer as TrustedVal<RSAPeer,PeerInfoRel>>::init_sign_val(&pkey, &PeerInfoRel.get_rep()[..], 
      & bincodeser::encode(
      & tosign
, SizeLimit::Infinite).unwrap()
      )
  }
  #[cfg(not(feature="wot"))]
  pub fn inner_sign_peer (_ : &String, _ : &TimeSpecExt, _ : &I) -> IoResult<Vec<u8>> {
    // TODO ?
    Ok(Vec::new())
  }

  pub fn new (name : String, pem : Option<PathBuf>, address : SerSocketAddr, extend : I, extendsign : I) -> IoResult<Self> {
    if pem.is_some() {
      panic!("TODO loading from pem"); // TODO
    };
    // pkey gen

    let pkeyrsa = Rsa::generate(RT::RSA_SIZE)?;

    let pkeyext = PKeyExt::new(Arc::new(pkeyrsa));
    let key = pkeyext.derive_key()?;

    let now = TimeSpecExt(time::get_time());
 
    let sign = Self::inner_sign_peer(&name, &now, &extendsign)?;
    Ok(RSAPeer {
      key : key,
      publickey : pkeyext,
      name : name,
      date : now.clone(),
      peersign : sign,
      addressdate : now.clone(),
      address : address,
      extend : extend,
      signedextend : extendsign,
      conf : PhantomData,
    })
  }

  /// update signable info
  pub fn update_info (&mut self, name : String, extendsign : I) -> IoResult<bool> {

    let now = TimeSpecExt(time::get_time());
    let sign = Self::inner_sign_peer(&name, &now, &extendsign)?;

    // when we do not have private key a zero length signature is created
    Ok(if sign.len() > 0 {
      self.name = name;
      self.date = now;
      self.peersign = sign;
      true
    } else {
      false
    })
  }

}

impl<RT : OpenSSLConf, I : KVCont> KeyVal for RSAPeer<RT,I> {
  // not that good (pkey can contain private and lot a clone... TODO (for now easier this way)
  type Key = Vec<u8>;

  #[inline]
  fn get_key(&self) -> Vec<u8> {
    self.key.clone()
  }
/*
  #[inline]
  fn get_key_ref<'a>(&'a self) -> &'a Vec<u8> {
    &self.key
  }*/

  #[inline]
  fn encode_kv<S:Serializer> (&self, s: S, is_local : bool, _ : bool) -> Result<S::Ok, S::Error> {
    if is_local {
      RSAPeerSerPri::from_rsa_peer(self).serialize(s) 
    } else {
      self.serialize(s)
    }
  }

  #[inline]
  fn decode_kv<'de,D:Deserializer<'de>> (d : D, _ : bool, _ : bool) -> Result<Self, D::Error> {
      <RSAPeer<RT,I>>::deserialize(d)
  }
  
  noattachment!();
}

impl<RT : OpenSSLConf, I : KVCont> SettableAttachment for RSAPeer<RT,I> {}

impl<RT : OpenSSLConf, I : KVCont> Peer for RSAPeer<RT,I> {
  type Address = SerSocketAddr;

  type ShadowW = OSSLShadowerW<RT>;
  type ShadowR = OSSLShadowerR<RT>;
 
  #[inline]
  fn get_shadower_r (&self) -> Self::ShadowR {
    OSSLShadowerR::new(self.publickey.clone()).unwrap() // TODO change peer trait
  }
  #[inline]
  fn get_shadower_w (&self) -> Self::ShadowW {
    OSSLShadowerW::new(self.publickey.clone()).unwrap() // TODO change peer trait
  }

  #[inline]
  fn default_message_mode (&self) -> <Self::ShadowW as ShadowBase>::ShadowMode {
    ASymSymMode::ASymSym
  }
/*  #[inline]
  fn default_header_mode (&self) -> <Self::ShadowW as ShadowBase>::ShadowMode {
    ASymSymMode::ASymOnly
  }*/
  #[inline]
  fn default_auth_mode (&self) ->  <Self::ShadowW as ShadowBase>::ShadowMode {
    ASymSymMode::None
  }
  #[inline]
  fn get_address(&self) -> &SerSocketAddr {
    &self.address
  }
}

/// temp struct for serializing with private key (we could use a wrapper but using this struct
/// allows using serde_derive
#[derive(Serialize)]
#[serde(rename = "RSAPeer")]
pub struct RSAPeerSerPri<'a, RT : OpenSSLConf + 'a, I : KVCont + 'a> {
  key : &'a Vec<u8>,
  publickey : &'a PKeyExt<RT>,
  name : &'a String,
  date : &'a TimeSpecExt,
  peersign : &'a Vec<u8>,
  signedextend : &'a I,
  addressdate : &'a TimeSpecExt,
  address : &'a SerSocketAddr,
  extend : &'a I,
  #[serde(skip)]
  conf : PhantomData<RT>,
}

impl<'a, RT : OpenSSLConf + 'a, I : KVCont + 'a> RSAPeerSerPri<'a, RT, I> {
  fn from_rsa_peer(or : &'a RSAPeer<RT,I>) -> Self {
    RSAPeerSerPri {
      key : &or.key,
      publickey : &or.publickey,
      name : &or.name,
      date : &or.date,
      peersign : &or.peersign,
      signedextend : &or.signedextend,
      addressdate : &or.addressdate,
      address : &or.address,
      extend : &or.extend,
      conf : PhantomData,
    }
  }
}
