//! Web of trust components components implemented with Openssl primitive for RSA 2048 and key
//! representation and content signing hash as SHA-512 of rsa key or original content.


extern crate openssl;
extern crate time;
extern crate bincode;

use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};
use rustc_serialize::hex::{ToHex,FromHex};
use std::io::Result as IoResult;
use self::openssl::crypto::hash::{Hasher,Type};
use self::openssl::crypto::pkey::{PKey};
use std::fmt::{Formatter,Debug};
use std::fmt::Error as FmtError;
use std::str::FromStr;
use std::cmp::PartialEq;
use std::cmp::Eq;
use std::sync::Arc;
use std::io::Write;
use std::ops::Deref;
use std::path::{Path,PathBuf};
use self::time::Timespec;

use kvstore::{KeyVal};
use std::net::{SocketAddr};
use utils::SocketAddrExt;
use utils::{TimeSpecExt};
use kvstore::Attachment;

use super::{TrustedPeer,Truster,TrustRel,TrustedVal,PeerInfoRel};
use super::trustedpeer::{TrustedPeerToSignEnc, TrustedPeerToSignDec, SendablePeerEnc, SendablePeerDec};
use peer::Peer;

static RSA_SIZE : usize = 2048;
static HASH_SIGN : Type = Type::SHA512;

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
  pub address : SocketAddrExt,

  ////// local info 
  
  /// warning must only be serialized locally!!!
  /// private stored TODO password protect it?? (for storage)
  /// TODO see usage of protected memory (there is a nacl wrapper) and need of a passphrase to obtain somethin from
  /// last update (change of address most of the time -> use for conflict management)
  /// Not mandatory (network is more likely to converge to last connected).
  privatekey : Option<Vec<u8>>,
}

impl TrustedPeer for RSAPeer {}


// firt is public key (to avoid multiple call to ffi just to get it) second is c openssl key
#[derive(Clone)]
/// Additional funtionalites over openssl lib PKey
pub struct PKeyExt(Vec<u8>,Arc<PKey>);
impl Debug for PKeyExt {
  fn fmt (&self, f : &mut Formatter) -> Result<(),FmtError> {
    write!(f, "public : {:?} \n private : {:?}", self.0.to_hex(), self.1.save_priv().to_hex())
  }
}
impl Deref for PKeyExt {
  type Target = PKey;
  #[inline]
  fn deref<'a> (&'a self) -> &'a PKey {
    &self.1
  }
}
/// seems ok (a managed pointer to c struct with drop implemented)
unsafe impl Send for PKeyExt {}
/// since in arc ok
unsafe impl Sync for PKeyExt {}

impl Encodable for PKeyExt {
  fn encode<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
    s.emit_struct("pkey",2, |s| {
      s.emit_struct_field("publickey", 0, |s|{
        self.0.encode(s)
      });
      s.emit_struct_field("privatekey", 1, |s|{
        self.1.save_priv().encode(s)
      })
    })
  }
}

impl Decodable for PKeyExt {
  fn decode<D:Decoder> (d : &mut D) -> Result<PKeyExt, D::Error> {
    d.read_struct("pkey",2, |d| {
      let publickey : Result<Vec<u8>, D::Error> = d.read_struct_field("publickey", 0, |d|{
        Decodable::decode(d)
      });
      let privatekey : Result<Vec<u8>, D::Error>= d.read_struct_field("privatekey", 1, |d|{
        Decodable::decode(d)
      });
      publickey.and_then(move |puk| privatekey.map (move |prk| {
        let mut res = PKey::new();
        res.load_pub(&puk[..]);
        if prk.len() > 0 {
          res.load_priv(&prk[..]);
        }
        PKeyExt(puk, Arc::new(res))
      }))
    })
  }
}

impl PartialEq for PKeyExt {
  fn eq (&self, other : &PKeyExt) -> bool {
    self.0.eq(&other.0)
  }
}

impl Eq for PKeyExt {}




/// This trait allows any keyval having a rsa pkey to implement RSA trust 
pub trait RSATruster : KeyVal<Key=Vec<u8>> {
  fn get_pkey<'a>(&'a self) -> &'a PKeyExt;
  #[inline]
  fn rsa_content_sign (&self, to_sign : &[u8]) -> Vec<u8> {
    debug!("sign content {:?}", to_sign);
    debug!("with key {:?}", self.get_pkey().0);
    let sig = RSAPeer::sign_cont(&(*self.get_pkey().1), to_sign);
    debug!("out sign {:?}", sig);
    sig
  }
  fn rsa_init_content_sign (pk : &PKey, to_sign : &[u8]) -> Vec<u8> {
    RSAPeer::sign_cont(pk, to_sign)
  }
  fn rsa_content_check (&self, tocheckenc : &[u8], sign : &[u8]) -> bool {
    // some issue when signing big content so sign hash512 instead TODO recheck on later version
    debug!("chec content {:?}", tocheckenc);
    debug!("with sign {:?}", sign);
    debug!("with key {:?}", self.get_pkey().0);
    let mut digest = Hasher::new(HASH_SIGN);
    digest.write_all(tocheckenc);
    self.get_pkey().1.verify_with_hash(&digest.finish()[..], sign, HASH_SIGN)
  }
  fn rsa_key_check (&self) -> bool {
    let mut digest = Hasher::new(HASH_SIGN);
    digest.write_all(&self.get_pkey().0[..]);
    self.get_key() == digest.finish()
  }


}


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
  fn get_pkey<'a>(&'a self) -> &'a PKeyExt {
    & self.publickey
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
, bincode::SizeLimit::Infinite).unwrap()
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

  fn sign_cont(pkey : &PKey, to_sign : &[u8]) -> Vec<u8> {
    let mut digest = Hasher::new(HASH_SIGN);
    digest.write_all(to_sign);
    pkey.sign_with_hash(&digest.finish()[..], HASH_SIGN)
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
      addressdate : data.addressdate,
      address : data.address,
      privatekey : None,
    }
  }

  pub fn new (name : String, pem : Option<PathBuf>, address : SocketAddr) -> RSAPeer {
    // pkey gen
    let mut pkey = PKey::new();
    pkey.gen(RSA_SIZE);

    let private = pkey.save_priv();
    let public  = pkey.save_pub();

    let mut digest = Hasher::new(HASH_SIGN);
    digest.write_all(&public[..]);
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
, bincode::SizeLimit::Infinite).unwrap()
      )
    };
 
    RSAPeer {
      key : key,
      publickey : PKeyExt(public, Arc::new(pkey)),
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
, bincode::SizeLimit::Infinite).unwrap()
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
  fn get_key(&self) -> Vec<u8> {
    self.key.clone()
  }

  #[inline]
  fn encode_dist_with_att<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
    self.to_sendablepeer().encode(s)
  }

  #[inline]
  fn decode_dist_with_att<D:Decoder> (d : &mut D) -> Result<RSAPeer, D::Error> {
    let tmp : Result<SendablePeerDec, D::Error> = Decodable::decode(d);
    tmp.map(|t| RSAPeer::from_sendablepeer(t))
  }

  #[inline]
  fn encode_dist<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
    self.to_sendablepeer().encode(s)
  }

  #[inline]
  fn decode_dist<D:Decoder> (d : &mut D) -> Result<RSAPeer, D::Error> {
    let tmp : Result<SendablePeerDec, D::Error> = Decodable::decode(d);
    tmp.map(|t| RSAPeer::from_sendablepeer(t))
  }

  #[inline]
  fn encode_loc_with_att<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
    self.encode(s)
  }

  #[inline]
  fn decode_loc_with_att<D:Decoder> (d : &mut D) -> Result<RSAPeer, D::Error> {
    Decodable::decode(d)
  }

  noattachment!();
}

impl Peer for RSAPeer {
  //type Address = SocketAddr;
  #[inline]
  fn to_address(&self) -> SocketAddr {
    self.address.0
  }
}


