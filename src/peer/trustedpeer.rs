//! peers using an asymetric key to check and accept other users
//! it is used as peers, but could also be used as keyvalue. For instance
//! in a wot, findpeers will return online one whereas findkv any peers.
//! In the first case it is for routing (need online user) and in the second to verify contents
//! (no need for online user).

extern crate openssl;
extern crate bincode;
extern crate time;

use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};
use rustc_serialize::hex::{ToHex,FromHex};
use std::io::Result as IoResult;
use self::time::Timespec;
use std::path::{Path,PathBuf};
use self::openssl::crypto::hash::{Hasher,Type};
use self::openssl::crypto::pkey::{PKey};
use std::fmt::{Formatter,Debug};
use std::fmt::Error as FmtError;
static RSA_SIZE : usize = 2048;
static HASH_SIGN : Type = Type::SHA512;
use std::str::FromStr;
use std::cmp::PartialEq;
use std::cmp::Eq;
use std::sync::Arc;
use std::num::Int;
use peer::Peer;
use std::net::{SocketAddr};
use utils::{TimeSpecExt};
use std::collections::VecDeque;
use kvstore::{KeyVal};
use kvstore::Attachment;
use std::io::Write;
use std::ops::Deref;
use utils::ArcKV;
use utils::SocketAddrExt;
// firt is public key second is c openssl key
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
        res.load_pub(puk.as_slice());
        if prk.len() > 0 {
          res.load_priv(prk.as_slice());
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

/// A peer with trust info, if we own the peer, a private key is defined.
/// This is the content stored and exchanged.
pub trait TrustedPeer : Peer<Key = Vec<u8>> + TrustedVal<ArcKV<Self>> + Truster {
  /*fn sign_content (&self, to_sign : &[u8]) -> Vec<u8> {
    self.content_sign(to_sign)
  }
  fn check_content (&self, tocheckenc : &[u8], sign : &[u8]) -> bool {
    self.content_check(tocheckenc, sign)
  }*/
  /*fn check_peer (&self) -> bool {
    self.key_check() && self.check_val(self)
  }*/
}


/// A content which can be signed and checked.
/// TODO parameterized over encoding (use msgenc)
pub trait TrustedVal<T : Truster> : KeyVal {
  type SignedContent : Encodable + 'static ;
  fn get_sign_content<'a> (&'a self) -> Self::SignedContent;
  fn get_sign<'a> (&'a self) -> &'a Vec<u8>;
  fn get_from<'a> (&'a self) -> &'a Vec<u8>;
  fn check_val (&self, me : &T) -> bool {
    if self.get_from() == &me.get_key() {
      let tosign = &self.get_sign_content();
      let tocheckenc = bincode::encode(&(Self::get_about(),tosign), bincode::SizeLimit::Infinite).unwrap();
      me.content_check(tocheckenc.as_slice(), self.get_sign().as_slice())
    } else {
      false
    }
  }

  fn sign_val (me : &T, cont : &Self::SignedContent) -> Vec<u8> {
    me.content_sign(bincode::encode(&(Self::get_about(),cont), bincode::SizeLimit::Infinite).unwrap().as_slice())
  }
  /// same as sign_val but for technical usage when Trustor is not yet totally initiated
  fn init_sign_val (me : &T::Internal, cont : &Self::SignedContent) -> Vec<u8> {
    <T as Truster>::init_content_sign(me, bincode::encode(&(Self::get_about(),cont), bincode::SizeLimit::Infinite).unwrap().as_slice())
  }
 
  /// return identifier of the kind of trust
  fn get_about() -> Vec<u8>;
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

// TODO implement rsa truster with all method plus a get id and getpkey 
pub trait RSATruster : KeyVal<Key = Vec<u8>> {
  fn get_pkey<'a>(&'a self) -> &'a PKeyExt;
}

impl<R : RSATruster> Truster for R { 
  type Internal = PKey;
  #[inline]
  fn content_sign (&self, to_sign : &[u8]) -> Vec<u8> {
    debug!("sign content {:?}", to_sign);
    debug!("with key {:?}", self.get_pkey().0);
    let sig = RSAPeer::sign_cont(&(*self.get_pkey().1), to_sign);
    debug!("out sign {:?}", sig);
    sig
  }
  fn init_content_sign (pk : &PKey, to_sign : &[u8]) -> Vec<u8> {
    RSAPeer::sign_cont(pk, to_sign)
  }
  fn content_check (&self, tocheckenc : &[u8], sign : &[u8]) -> bool {
    // some issue when signing big content so sign hash512 instead TODO recheck on later version
    debug!("chec content {:?}", tocheckenc);
    debug!("with sign {:?}", sign);
    debug!("with key {:?}", self.get_pkey().0);
    let mut digest = Hasher::new(HASH_SIGN);
    digest.write_all(tocheckenc);
    self.get_pkey().1.verify_with_hash(digest.finish().as_slice(), sign, HASH_SIGN)
  }
  fn key_check (&self) -> bool {
    let mut digest = Hasher::new(HASH_SIGN);
    digest.write_all(self.get_pkey().0.as_slice());
    self.get_key() == digest.finish()
  }

}


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
/*  #[inline]
  fn sign_content (&self, to_sign : &[u8]) -> Vec<u8> {
    debug!("sign content {:?}", to_sign);
    debug!("with key {:?}", &self.publickey.0);
    let sig = RSAPeer::sign_cont(&self.publickey.1, to_sign);
    debug!("out sign {:?}", sig);
    sig
  }
  fn check_content (&self, tocheckenc : &[u8], sign : &[u8]) -> bool {
    // some issue when signing big content so sign hash512 instead TODO recheck on later version
    debug!("chec content {:?}", tocheckenc);
    debug!("with sign {:?}", sign);
    debug!("with key {:?}", &self.publickey.0);
    let mut digest = Hasher::new(HASH_SIGN);
    digest.write_all(tocheckenc);
    self.publickey.1.verify_with_hash(digest.finish().as_slice(), sign, HASH_SIGN)
  }

  /// Verify all peer contained information
  #[inline]
  fn check_peer(&self) -> bool {
    self.check_key() && self.check_info()
  }

}*/

impl RSATruster for RSAPeer {
  fn get_pkey<'a>(&'a self) -> &'a PKeyExt {
    & self.publickey
  }
}


/// self signed implementation
impl<'a> TrustedVal<ArcKV<RSAPeer>> for RSAPeer {
  type SignedContent = TrustedPeerToSignEnc<'a>;
  fn get_sign_content<'b> (&'b self) -> TrustedPeerToSignEnc<'b> {
    TrustedPeerToSignEnc {
      version : 0,
      name : &self.name,
      date : &self.date,
    }
  }
  #[inline]
  fn get_sign<'b> (&'b self) -> &'b Vec<u8> {
    &self.peersign
  }
  #[inline]
  fn get_from<'b> (&'b self) -> &'b Vec<u8> {
    &self.key
  }
  // kind of version describing semantic of this trust
  #[inline]
  fn get_about () -> Vec<u8> {
    // TODO replace to a keyval key of peersign concept (need to pass a context in param)
    vec![0]
  }

}



impl<'a, T : Truster, KV : TrustedVal<T>> TrustedVal<T> for ArcKV<KV> {
  type SignedContent = <KV as TrustedVal<T>>::SignedContent;
  #[inline]
  fn get_sign_content<'b> (&'b self) -> <KV as TrustedVal<T>>::SignedContent {
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
  fn get_about () -> Vec<u8> {
    <KV as TrustedVal<T>>::get_about()
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



/// Special keyval for queries of signature about a peer.
/// TODO consider including signature in it (not for local storage but as reply)
#[derive(Debug, PartialEq, Eq, Clone,RustcEncodable,RustcDecodable)]
pub struct PromSigns<TP : TrustedPeer> {
  /// peer concerned
  peer : <TP as KeyVal>::Key,
  /// points to key of PeerSign information with firt value being peer id and second peersign id
  pub signby : VecDeque<(<TP as KeyVal>::Key, <TP as KeyVal>::Key)>,
  /// max nb of signs (latest are kept) TODO see if global possible
  pub maxsig : usize,
}

impl<TP : TrustedPeer> PromSigns<TP> {
  pub fn new(s : &PeerSign<TP>, ms : usize) -> PromSigns<TP> {
    PromSigns {
      peer : s.about.clone(),
      signby : { 
        let mut sb = VecDeque::new();
        sb.push_front(s.get_key());
        sb
      },
      maxsig : ms,
    }
  }
  // highly inefficient TODO see redesign keyval with multiple key
  pub fn add_sign(&self, s : &PeerSign<TP>) -> Option<PromSigns<TP>> {
    if self.signby.iter().any(|sb| sb.0 == s.get_key().0 && sb.1 == s.get_key().1) {
      None
    } else {
      let mut newsignby = self.signby.clone();
      newsignby.push_front(s.get_key());
      if newsignby.len() > self.maxsig {
        newsignby.pop_back();
      };
      Some(PromSigns {
        peer   : self.peer.clone(),
        signby : newsignby,
        maxsig : self.maxsig,
      })
    }
  }

  pub fn merge(&self, psigns : &PromSigns<TP>) -> Option<PromSigns<TP>> {
    if self.peer == psigns.peer {
      None
    } else {
      let mut change = false;
      let mut newsignby = self.signby.clone();
      for s in self.signby.iter() {
        if !psigns.signby.iter().any(|v| v == s) { 
          newsignby.push_front(s.clone());
          change = true;
        };
      };
      // same as n time pop back
      newsignby.truncate(self.maxsig);

      if change {
        Some(PromSigns {
          peer : self.peer.clone(),
          signby : newsignby,
          maxsig : self.maxsig,
        })
      } else {
        None
      }
    }
  }

  // highly inefficient
  pub fn remove_sign(&self, s : &PeerSign<TP>) -> Option<PromSigns<TP>> {
    match self.signby.iter().position(|sb| sb.0 == s.get_key().0 && sb.1 == s.get_key().1) {
      Some(ix) => {
        let mut newsignby = self.signby.clone();
        newsignby.remove(ix);
        Some(PromSigns {
          peer : self.peer.clone(),
          signby : newsignby,
          maxsig : self.maxsig,
        })
      },
      None => None,
    }
  }

}

impl<TP : TrustedPeer> KeyVal for PromSigns<TP> {
  // not that good (pkey can contain private and lot a clone... TODO (for now easier this way)
  type Key = <TP as KeyVal>::Key;
  fn get_key(&self) -> <TP as KeyVal>::Key {
    self.peer.clone()
  }
  // TODO may be faster to have distant encoding embedding PeerSign info yet require reference to
  // other store in keyval for encoding.
  nospecificencoding!(PromSigns<TP>);
  noattachment!();
}

/// Trust signing from one node to another. Key is signature (require something to keep relation
/// with its components), it should be some cat of from, about and its tag, but querying for all 
/// possible tag is not very practical
#[derive(Debug, PartialEq, Eq, Clone,RustcEncodable,RustcDecodable)]
pub struct PeerSign<TP : TrustedPeer> {
  /// peer signing
  pub from : <TP as KeyVal>::Key,
  /// peer to sign
  pub about : <TP as KeyVal>::Key,
  /// peer trust
  pub trust : u8,
  /// version to update/revoke...
  pub tag : usize,
  /// signature
  pub sign : Vec<u8>,
}

impl<TP : TrustedPeer> PeerSign<TP> {

  /// sign an new trust level, from peer must be a peer for whom we got the privatekey (ourselves
  /// for instance). TODO remove those ArcKV
  pub fn new (fromP : &ArcKV<TP>, aboutP : &ArcKV<TP>, trust : u8, tag : usize) -> Option<PeerSign<TP>> {
    // Note that we use bincode but if serializing scheme change, we will be lost, so we add an
    // encoding version in first position (currently serialized as is)
    let from  = fromP.get_key();
    let about = aboutP.get_key();
    let vsign = {
      let tosign = (&from, &about, trust, tag);
      <Self as TrustedVal<ArcKV<TP>>>::sign_val(fromP, &tosign)
    };
    debug!("sign : {:?}", vsign);
    // current openssl if is strange
    if vsign.len() == 0 {
      error!("trying to sign with no private key");
      None
    } else {
      Some(
        PeerSign {
          from  : from,
          about : about,
          trust : trust,
          tag   : tag,
          sign  : vsign,
      })
    }
  }
/*
  /// check a signature of a user
  #[inline]
  pub fn check_for (&self, from : &TP) -> bool {
    if from.get_key() == self.from {
      let version : u8 = 0;
      let tocheck = &(version, &self.from, &self.about, self.trust, self.tag);
      let tocheckenc = bincode::encode(tocheck, bincode::SizeLimit::Infinite).unwrap();
      from.content_check(tocheckenc.as_slice(), self.sign.as_slice())
    } else {
      false
    }
  }*/

}

impl<TP : TrustedPeer> KeyVal for PeerSign<TP> {
  // a pair of from and about ids
  type Key = (<TP as KeyVal>::Key, <TP as KeyVal>::Key);
  fn get_key(&self) -> (<TP as KeyVal>::Key, <TP as KeyVal>::Key) {
    (self.from.clone(), self.about.clone())
  }
  nospecificencoding!(PeerSign<TP>);
  noattachment!();
}

impl<'a, TP : TrustedPeer> TrustedVal<ArcKV<TP>> for PeerSign<TP> {
  type SignedContent = (&'a Vec<u8>, &'a Vec<u8>, u8, usize);
  fn get_sign_content<'b> (&'b self) -> (&'b Vec<u8>, &'b Vec<u8>, u8, usize) {
      (&self.from, &self.about, self.trust, self.tag)
  }
  #[inline]
  fn get_sign<'b> (&'b self) -> &'b Vec<u8> {
    &self.sign
  }

  #[inline]
  fn get_from<'b> (&'b self) -> &'b Vec<u8> {
    &self.from
  }

  // kind of version describing semantic of this trust
  #[inline]
  fn get_about () -> Vec<u8> {
    // TODO replace to a keyval key of peersign concept (need to pass a context in param)
    vec![1]
  }

}

#[derive(RustcEncodable,Debug)]
pub struct TrustedPeerToSignEnc<'a> {
  // to allow multiple encoding in time
  version : u8,
  name : &'a String,
  date : &'a TimeSpecExt,
}

#[derive(RustcDecodable)]
struct TrustedPeerToSignDec {
  version : u8,
  name : String,
  date : TimeSpecExt,
}

#[derive(RustcEncodable)]
struct SendablePeerEnc<'a> {
  key : &'a Vec<u8>,
  publickey : &'a Vec<u8>,
  name : &'a String,
  date : &'a TimeSpecExt,
  peersign : &'a Vec<u8>,
  addressdate : &'a TimeSpecExt,
  address : &'a SocketAddrExt,
}

#[derive(RustcDecodable)]
struct SendablePeerDec {
  key : Vec<u8>,
  publickey : Vec<u8>,
  name : String,
  date : TimeSpecExt,
  peersign : Vec<u8>,
  addressdate : TimeSpecExt,
  address : SocketAddrExt,
}

impl RSAPeer {

  fn sign_cont(pkey : &PKey, to_sign : &[u8]) -> Vec<u8> {
    let mut digest = Hasher::new(HASH_SIGN);
    digest.write_all(to_sign);
    pkey.sign_with_hash(digest.finish().as_slice(), HASH_SIGN)
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
    pkey.load_pub(data.publickey.as_slice());
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
    digest.write_all(public.as_slice());
    let key = digest.finish();

    let now = TimeSpecExt(time::get_time());
    let sign = {
      let tosign = &TrustedPeerToSignEnc {
        version : 0,
        name : &name,
        date : &now,
      };
      debug!("in create info : {:?}", tosign);
      <RSAPeer as TrustedVal<ArcKV<RSAPeer>>>::init_sign_val(&pkey, tosign)
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
      <RSAPeer as TrustedVal<ArcKV<RSAPeer>>>::init_sign_val(&pkey, tosign)
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

