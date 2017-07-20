//! Web of trust components components implemented with Openssl primitive for RSA 2048 and key
//! representation and content signing hash as SHA-512 of rsa key or original content.

// TODO !!!! openssl binding for encryption are bad as it allocate a result each time, and in a
// pipe it is useless TODO add encrypt(&[u8],&mut [u8]) to bindings!!!!
// TODO test like tcp
// TODO make it more generic by using an rsa impl component include into rsapeer (so that new peer
// just include it in struct).

extern crate openssl;
extern crate time;

extern crate mydht_openssl;
#[cfg(test)]
extern crate mydht_basetest;

//use mydhtresult::Result as MDHTResult;
use self::mydht_openssl::rsa_openssl::{
  PKeyExt,
  RSATruster,
  AESShadower,
  ASymSymMode,
};
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::slice::bytes::copy_memory;
use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};
use rustc_serialize::hex::{ToHex};
use std::io::Result as IoResult;
use self::openssl::crypto::hash::{Hasher,Type};
use self::openssl::crypto::pkey::{PKey};
use self::openssl::crypto::symm::{Crypter,Mode};
use self::openssl::crypto::symm::Type as SymmType;
use rand::os::OsRng;
use rand::Rng;
use std::fmt::{Formatter,Debug};
use std::fmt::Error as FmtError;
//use std::str::FromStr;
use std::cmp::PartialEq;
use std::cmp::Eq;
use std::sync::Arc;
use std::io::Write;
use std::io::Read;
use std::ops::Deref;
use std::path::{PathBuf};
//use self::time::Timespec;

use mydht_base::keyval::{KeyVal};
use std::net::{SocketAddr};
use mydht_base::transport::SerSocketAddr;
use mydht_base::utils::{TimeSpecExt};
use mydht_base::keyval::{Attachment,SettableAttachment};
use super::{TrustedPeer,Truster,TrustRel,TrustedVal,PeerInfoRel};
//use super::trustedpeer::{TrustedPeerToSignDec};
use super::trustedpeer::{TrustedPeerToSignEnc, SendablePeerEnc, SendablePeerDec};
use mydht_base::peer::{Peer,Shadow};
use bincode::rustc_serialize as bincode;
use bincode::SizeLimit;
#[cfg(test)]
use std::net::Ipv4Addr;
#[cfg(test)]
use mydht_base::utils;
#[cfg(test)]
use std::io::Cursor;
#[cfg(test)]
use self::mydht_basetest::shadow::shadower_test;


static RSA_SIZE : usize = 2048;
static HASH_SIGN : Type = Type::SHA512;
static SHADOW_TYPE : SymmType = SymmType::AES_256_CBC;

/// a TrustedPeer using RSA 2048 openssl implementation with sha 512 on content and to derive id.
#[derive(Debug, PartialEq, Eq, Clone,RustcEncodable,RustcDecodable)]
pub struct RSAPeer {
  /// key to use to identify peer, derived from publickey it is shorter
  key : Vec<u8>,
  /// is used as id/key TODO maybe two publickey use of a master(in case of compromition)
  publickey : PKeyExt,

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

  ////// transient info
  
  pub addressdate : TimeSpecExt,
  /// last known address.
  /// peers with incorrect address would not be returned in 
  /// findpeerrs but possibly return in findkv (for example in a wot
  /// findkv is to check content, while findpeers to communicate
  pub address : SerSocketAddr,

  ////// local info 
  
  /// warning must only be serialized locally!!!
  /// private stored TODO password protect it?? (for storage)
  /// TODO see usage of protected memory (there is a nacl wrapper) and need of a passphrase to obtain somethin from
  /// last update (change of address most of the time -> use for conflict management)
  /// Not mandatory (network is more likely to converge to last connected).
  privatekey : Option<Vec<u8>>,
}

impl TrustedPeer for RSAPeer {}




//impl<R : RSATruster> Truster for TraitRSATruster<R> {
/// boilerplate implement, to avoid conflict implementation over traits
impl Truster for RSAPeer {
  type Internal = PKey;
  #[inline]
  fn content_sign (&self, to_sign : &[u8]) -> Vec<u8> {
    self.rsa_content_sign(to_sign)
  }
  fn init_content_sign (pk : &PKey, to_sign : &[u8]) -> Vec<u8> {
    <Self as RSATruster>::rsa_init_content_sign(pk, to_sign)
  }
  fn content_check (&self, tocheckenc : &[u8], sign : &[u8]) -> bool {
    self.rsa_content_check(tocheckenc, sign)
  }
  fn key_check (&self) -> bool {
    self.rsa_key_check()
  }

}

impl RSATruster for RSAPeer {
  const HASH_SIGN : Type = Type::SHA512;
  const RSA_SIZE : usize = 2048;
  const SHADOW_TYPE : SymmType = SymmType::AES_256_CBC;
  const CRYPTER_BLOCK_SIZE : usize = 16;
  const CRYPTER_KEY_SIZE : usize = 32;
  const CRYPTER_KEY_ENC_SIZE : usize = 256;
  const CRYPTER_KEY_DEC_SIZE : usize = 214;



  fn get_pkey<'a>(&'a self) -> &'a PKeyExt {
    & self.publickey
  }
  fn get_pkey_mut<'a>(&'a mut self) -> &'a mut PKeyExt {
    &mut self.publickey
  }

}

/// self signed implementation
impl<'a> TrustedVal<RSAPeer, PeerInfoRel> for RSAPeer
{
//  type SignedContent = TrustedPeerToSignEnc<'a>;
  //fn get_sign_content<'b> (&'b self) -> TrustedPeerToSignEnc<'b> {
  fn get_sign_content (& self) -> Vec<u8> {
    bincode::encode (
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

impl RSAPeer {

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
      publickey : &self.publickey.0,
      name : &self.name,
      date : &self.date,
      peersign : &self.peersign,
      addressdate : &self.addressdate,
      address : &self.address,
    }
  }

  /// initialize to min trust value TODO redesign to include trust calculation???
  fn from_sendablepeer(data : SendablePeerDec) -> RSAPeer {
    let now = TimeSpecExt(time::get_time());
    let mut pkey = PKey::new();
    pkey.load_pub(&data.publickey[..]);
    RSAPeer {
      key : data.key,
      publickey : PKeyExt(data.publickey, Arc::new(pkey)),
      name : data.name,
      date : data.date,
      peersign : data.peersign,
      addressdate : now,
      address : data.address,
      privatekey : None,
    }
  }
  
  pub fn new (name : String, pem : Option<PathBuf>, address : SocketAddr) -> RSAPeer {
    if pem.is_some() {
      panic!("TODO loading from pem"); // TODO
    };
    // pkey gen
    let mut pkey = PKey::new();
    pkey.gen(RSA_SIZE);

    let private = pkey.save_priv();
    let public  = pkey.save_pub();

    let mut digest = Hasher::new(HASH_SIGN);
    digest.write_all(&public[..]).unwrap(); // digest should not panic
    let key = digest.finish();

    let now = TimeSpecExt(time::get_time());
    let sign = {
      let tosign = &TrustedPeerToSignEnc {
        version : 0,
        name : &name,
        date : &now,
      };
      debug!("in create info : {:?}", tosign);
      <RSAPeer as TrustedVal<RSAPeer,PeerInfoRel>>::init_sign_val(&pkey, &PeerInfoRel.get_rep()[..], 
      & bincode::encode(
      & tosign
, SizeLimit::Infinite).unwrap()
      )
    };
 
    RSAPeer {
      key : key,
      publickey : PKeyExt(public, Arc::new(pkey)),
      name : name,
      date : now.clone(),
      peersign : sign,
      addressdate : now.clone(),
      address : SerSocketAddr(address),
      privatekey : Some(private),
    }
  }

  /// update signable info
  pub fn update_info (&mut self, name : String) -> bool {
    // pkey gen
    let pkey = &self.publickey.1;

    let now = TimeSpecExt(time::get_time());
    let sign = {
      //let tosign = &self.to_trustedpeer_tosign();
      let tosign = &TrustedPeerToSignEnc {
        version : 0,
        name : &name,
        date : &now,
      };
      debug!("in update info : {:?}", tosign);
      <RSAPeer as TrustedVal<RSAPeer,PeerInfoRel>>::init_sign_val(&pkey, &PeerInfoRel.get_rep()[..], 
      &bincode::encode(&
      tosign
, SizeLimit::Infinite).unwrap()
      )
    };

    debug!("with key {:?}", &self.publickey.0);
    debug!("with sign {:?}", sign);
    // when we do not have private key a zero length signature is created
    if sign.len() > 0 {
      self.name = name;
      self.date = now;
      self.peersign = sign;
      true
    } else {
      false
    }
  }

}

impl KeyVal for RSAPeer {
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
  fn encode_kv<S:Encoder> (&self, s: &mut S, is_local : bool, _ : bool) -> Result<(), S::Error> {
    if is_local {
      self.encode(s)
    } else {
      self.to_sendablepeer().encode(s)
    }
  }

  #[inline]
  fn decode_kv<D:Decoder> (d : &mut D, is_local : bool, _ : bool) -> Result<RSAPeer, D::Error> {
    if is_local {
      Decodable::decode(d)
    } else {
      let tmp : Result<SendablePeerDec, D::Error> = Decodable::decode(d);
      tmp.map(|t| RSAPeer::from_sendablepeer(t))
    }
  }
  
  noattachment!();
}

impl SettableAttachment for RSAPeer {}

impl Peer for RSAPeer {
  type Address = SerSocketAddr;
  type Shadow = AESShadower<RSAPeer>;
  #[inline]
  fn to_address(&self) -> SerSocketAddr {
    self.address.clone()
  }
  #[inline]
  fn get_shadower (&self, write : bool) -> Self::Shadow {
    AESShadower::new(self.get_pkey(), write)
  }
  #[inline]
  fn default_message_mode (&self) -> <Self::Shadow as Shadow>::ShadowMode {
    ASymSymMode::ASymSym
  }
  #[inline]
  fn default_header_mode (&self) -> <Self::Shadow as Shadow>::ShadowMode {
    ASymSymMode::ASymOnly
  }
  #[inline]
  fn default_auth_mode (&self) ->  <Self::Shadow as Shadow>::ShadowMode {
    ASymSymMode::None
  }

}

