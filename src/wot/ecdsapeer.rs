//! Web of trust components components implemented with rustcrypto primitives for ecdsa (ed25519) and key
//! representation and content signing hash as RIPEMD 160 of rsa key or original content.


extern crate crypto;
extern crate time;

use bincode::rustc_serialize as bincode;
use bincode::SizeLimit;
use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};
//use rustc_serialize::hex::{ToHex,FromHex};
//use std::io::Result as IoResult;
use self::crypto::digest::Digest;
use self::crypto::ripemd160::Ripemd160;
use self::crypto::ed25519;
//use std::fmt::{Formatter,Debug};
//use std::fmt::Error as FmtError;
//use std::str::FromStr;
//use std::cmp::PartialEq;
use std::cmp::Eq;
//use std::sync::Arc;
//use std::io::Write;
//use std::ops::Deref;
use std::path::{PathBuf};
//use self::time::Timespec;

use keyval::{KeyVal};
use std::net::{SocketAddr};
use utils::SocketAddrExt;
use utils::{TimeSpecExt};
use utils;
use keyval::{Attachment,SettableAttachment};
use super::{TrustedPeer,Truster,TrustRel,TrustedVal,PeerInfoRel};
//use super::trustedpeer::{TrustedPeerToSignDec};
use super::trustedpeer::{TrustedPeerToSignEnc, SendablePeerEnc, SendablePeerDec};
use peer::Peer;

#[derive(Debug, PartialEq, Eq, Clone,RustcEncodable,RustcDecodable)]
pub struct ECDSAPeer {
  /// key to use to identify peer, derived from publickey it is shorter
  key : Vec<u8>,
  /// is used as id/key TODO maybe two publickey use of a master(in case of compromition)
  publickey : Vec<u8>,

  /// an associated complementary info
  name : String,

  /// TODO all other user published info see + factorize with trust peer basis

  /// date (when was those info associated), this is used to manage change of information.
  /// Yet it is obviously not really good, expect some lock information due to using wrongly set
  /// date and so on.
  date : TimeSpecExt,

  /// Signature of this content (must be checked on reception (all transmitted info are signed
  /// (all but private key, trust, trustdate)) + no transient as no need , done by ping proto
  /// (accept...)
  peersign : Vec<u8>,

  ////// transient info
  
  pub addressdate : TimeSpecExt,
  /// last known address.
  /// peers with incorrect address would not be returned in 
  /// findpeerrs but possibly return in findkv (for example in a wot
  /// findkv is to check content, while findpeers to communicate
  pub address : SocketAddrExt,

  ////// local info 
  
  /// warning must only be serialized locally!!!
  /// private stored TODO password protect it?? (for storage)
  privatekey : Option<Vec<u8>>,
}

impl TrustedPeer for ECDSAPeer {}


pub trait ECDSATruster : KeyVal<Key = Vec<u8>> {
  fn get_privatekey<'a>(&'a self) -> &'a Option<Vec<u8>>;
  fn get_publickey<'a>(&'a self) -> &'a Vec<u8>;

  #[inline]
  fn ec_content_sign (&self, to_sign : &[u8]) -> Vec<u8> {
    debug!("sign content {:?}", to_sign);
    debug!("with key {:?}", self.get_publickey());
    let sig = ECDSAPeer::sign_cont(self.get_privatekey().as_ref().unwrap(), to_sign);
    debug!("out sign {:?}", sig);
    sig
  }
  fn ec_init_content_sign (pk : &Vec<u8>, to_sign : &[u8]) -> Vec<u8> {
    ECDSAPeer::sign_cont(pk, to_sign)
  }
  fn ec_content_check (&self, tocheckenc : &[u8], sign : &[u8]) -> bool {
    // some issue when signing big content so sign hash512 instead TODO recheck on later version
    debug!("chec content {:?}", tocheckenc);
    debug!("with sign {:?}", sign);
    debug!("with key {:?}", self.get_publickey());
    let mut digest = Ripemd160::new();
    let chash = utils::hash_buf_crypto(tocheckenc, &mut digest);
 
    ed25519::verify(&chash[..],&self.get_publickey()[..],sign)
  }
  fn ec_key_check (&self) -> bool {
    let mut digest = Ripemd160::new();
    self.get_key() == utils::hash_buf_crypto(self.get_publickey().as_slice(), &mut digest)
  }

}

//impl<T : ECDSATruster> Truster for T {
/// boilerplate implement, to avoid conflict implementation over traits
impl Truster for ECDSAPeer {
  type Internal = Vec<u8>;
  #[inline]
  fn content_sign (&self, to_sign : &[u8]) -> Vec<u8> {
    self.ec_content_sign(to_sign)
  }
  fn init_content_sign (pk : &Vec<u8>, to_sign : &[u8]) -> Vec<u8> {
    <Self as ECDSATruster>::ec_init_content_sign(pk, to_sign)
  }
  fn content_check (&self, tocheckenc : &[u8], sign : &[u8]) -> bool {
    self.ec_content_check(tocheckenc, sign)
  }
  fn key_check (&self) -> bool {
    self.ec_key_check()
  }

}

impl ECDSATruster for ECDSAPeer {
  fn get_publickey<'a>(&'a self) -> &'a Vec<u8> {
    & self.publickey
  }
  fn get_privatekey<'a>(&'a self) -> &'a Option<Vec<u8>> {
    & self.privatekey
  }
}

/// self signed implementation
/// TODO factorize with RSAPeer(a BaseTrustedPeer generic over trusted impl and key type)
impl<'a> TrustedVal<ECDSAPeer, PeerInfoRel> for ECDSAPeer {
  // type SignedContent = TrustedPeerToSignEnc<'a>;
  fn get_sign_content (& self) -> Vec<u8> {
    bincode::encode(
    & TrustedPeerToSignEnc {
      version : 0,
      name : &self.name,
      date : &self.date,
    }
, SizeLimit::Infinite).unwrap()
  }
  #[inline]
  fn get_sign<'b> (&'b self) -> &'b Vec<u8> {
    &self.peersign
  }
  #[inline]
  fn get_from<'b> (&'b self) -> &'b Vec<u8> {
    &self.key
  }
  #[inline]
  /// static about (PeerInfoRel), calling this will panic
  /// It shall return a key (requires to implement static declaration of semantic)
  fn get_about<'b> (&'b self) -> &'b Vec<u8> {
    panic!("This trusted val use a statically define semantic, get_about is not implemented")
  }

}

impl ECDSAPeer {
  // TODO move to ECDSA trait
  fn sign_cont(pkey : &Vec<u8>, to_sign : &[u8]) -> Vec<u8> {
    let mut digest = Ripemd160::new();
    let chash = utils::hash_buf_crypto(to_sign, &mut digest);
    ed25519::signature(&chash[..], &pkey[..]).to_vec()
  }

  fn to_trustedpeer_tosign<'a>(&'a self) -> TrustedPeerToSignEnc<'a> {
    TrustedPeerToSignEnc {
      version : 0,
      name : &self.name,
      date : &self.date,
    }
  }

  fn to_sendablepeer<'a>(&'a self) -> SendablePeerEnc<'a> {
    SendablePeerEnc {
      key : &self.key,
      publickey : &self.publickey,
      name : &self.name,
      date : &self.date,
      peersign : &self.peersign,
      addressdate : &self.addressdate,
      address : &self.address,
    }
  }

  /// initialize to min trust value TODO redesign to include trust calculation???
  fn from_sendablepeer(data : SendablePeerDec) -> ECDSAPeer {
    let now = TimeSpecExt(time::get_time());
    let pkey = data.publickey;
    ECDSAPeer {
      key : data.key,
      publickey : pkey,
      name : data.name,
      date : data.date,
      peersign : data.peersign,
      addressdate : now,
      address : data.address,
      privatekey : None,
    }
  }

  pub fn new (name : String, pem : Option<PathBuf>, address : SocketAddr) -> ECDSAPeer {
    if pem.is_some() {
      panic!("TODO loading from pem or rem param"); // TODO
    };
    let seed = utils::random_bytes(32);
    // pkey gen
    let (pr, pu) = ed25519::keypair(&seed[..]);
    let public = pu.to_vec();
    let private = pr.to_vec();
    let mut digest = Ripemd160::new();
    let key = utils::hash_buf_crypto(&public[..], &mut digest);

    let now = TimeSpecExt(time::get_time());
    let sign = {
      let tosign = &TrustedPeerToSignEnc {
        version : 0,
        name : &name,
        date : &now,
      };
      debug!("in create info : {:?}", tosign);
      <ECDSAPeer as TrustedVal<ECDSAPeer,PeerInfoRel>>::init_sign_val(&private, PeerInfoRel.get_rep().as_slice(), 
      & bincode::encode(
      &tosign
, SizeLimit::Infinite).unwrap()
      )
    };
 
    ECDSAPeer {
      key : key,
      publickey : public,
      name : name,
      date : now.clone(),
      peersign : sign,
      addressdate : now.clone(),
      address : SocketAddrExt(address),
      privatekey : Some(private),
    }
  }

  /// update signable info
  pub fn update_info (&mut self, name : String) -> bool {

    let now = TimeSpecExt(time::get_time());
    let sign = self.privatekey.as_ref().map (|pk|{
      //let tosign = &self.to_trustedpeer_tosign();
      let tosign = &TrustedPeerToSignEnc {
        version : 0,
        name : &name,
        date : &now,
      };
      debug!("in update info : {:?}", tosign);
      <ECDSAPeer as TrustedVal<ECDSAPeer,PeerInfoRel>>::init_sign_val(pk, PeerInfoRel.get_rep().as_slice(), 
      & bincode::encode(
      tosign
, SizeLimit::Infinite).unwrap()
      )
    });

    debug!("with key {:?}", &self.publickey);
    debug!("with sign {:?}", sign);
    // when we do not have private key a zero length signature is created
    match sign {
      Some(s) => {
        self.name = name;
        self.date = now;
        self.peersign = s;
        true
      },
      None => false,
    }
  }

}

// TODO factorize with RSAPeer (using a trustable peer or something).
impl KeyVal for ECDSAPeer {
  type Key = Vec<u8>; 
  #[inline]
  fn get_key(&self) -> Vec<u8> {
    self.key.clone()
  }
/*
  #[inline]
  fn get_key_ref<'a>(&'a self) -> &'a Vec<u8> {
    &self.key
  }
*/
  #[inline]
  fn encode_kv<S:Encoder> (&self, s: &mut S, is_local : bool, _ : bool) -> Result<(), S::Error> {
    if is_local {
      self.encode(s)
    } else {
      self.to_sendablepeer().encode(s)
    }
  }

  #[inline]
  fn decode_kv<D:Decoder> (d : &mut D, is_local : bool, _ : bool) -> Result<ECDSAPeer, D::Error> {
    if is_local {
      Decodable::decode(d)
    } else {
      let tmp : Result<SendablePeerDec, D::Error> = Decodable::decode(d);
      tmp.map(|t| ECDSAPeer::from_sendablepeer(t))
    }
  }


  noattachment!();
}

impl SettableAttachment for ECDSAPeer { }

impl Peer for ECDSAPeer {
  type Address = SocketAddr;
  #[inline]
  fn to_address(&self) -> SocketAddr {
    self.address.0
  }
}


