//! Web of trust components components implemented with Openssl primitive for RSA 2048 and key
//! representation and content signing hash as SHA-512 of rsa key or original content.

// TODO !!!! openssl binding for encryption are bad as it allocate a result each time, and in a
// pipe it is useless TODO add encrypt(&[u8],&mut [u8]) to bindings!!!!
// TODO test like tcp
// TODO make it more generic by using an rsa impl component include into rsapeer (so that new peer
// just include it in struct).

extern crate openssl;
extern crate time;

//use mydhtresult::Result as MDHTResult;
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

use keyval::{KeyVal};
use std::net::{SocketAddr};
use utils::SocketAddrExt;
use utils::{TimeSpecExt};
use keyval::{Attachment,SettableAttachment};
use super::{TrustedPeer,Truster,TrustRel,TrustedVal,PeerInfoRel};
//use super::trustedpeer::{TrustedPeerToSignDec};
use super::trustedpeer::{TrustedPeerToSignEnc, SendablePeerEnc, SendablePeerDec};
use peer::{Peer,Shadow};
use bincode::rustc_serialize as bincode;
use bincode::SizeLimit;
#[cfg(test)]
use std::net::Ipv4Addr;
#[cfg(test)]
use utils;
#[cfg(test)]
use std::io::Cursor;


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
      try!(s.emit_struct_field("publickey", 0, |s|{
        self.0.encode(s)
      }));
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
    digest.write_all(tocheckenc).is_ok() // TODO proper errror??
    && self.get_pkey().1.verify_with_hash(&digest.finish()[..], sign, HASH_SIGN)
  }
  fn rsa_key_check (&self) -> bool {
    let mut digest = Hasher::new(HASH_SIGN);
    digest.write_all(&self.get_pkey().0[..]).is_ok() // TODO return proper error??
    && self.get_key() == digest.finish()
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

  fn ciph_cont(pkey : &PKey, to_ciph : &[u8]) -> Vec<u8> {
    pkey.encrypt(to_ciph) // TODO check padding options and use encrypt_with_padding
  }
  fn unciph_cont(pkey : &PKey, to_ciph : &[u8]) -> Option<Vec<u8>> {
    Some(pkey.decrypt(to_ciph)) // TODO what happen if no private key in pkey???
  }



  fn sign_cont(pkey : &PKey, to_sign : &[u8]) -> Vec<u8> {
    let mut digest = Hasher::new(HASH_SIGN);
    match digest.write_all(to_sign) { // TODO return result<vec<u8>>
      Ok(_) => (),
      Err(e) => {
        error!("Rsa peer digest failure : {:?}",e);
        return Vec::new();
      },
    }
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
  type Address = SocketAddr;
  type Shadow = AESShadower;
  #[inline]
  fn to_address(&self) -> SocketAddr {
    self.address.0
  }
  #[inline]
  fn get_shadower (&self, write : bool) -> Self::Shadow {
    AESShadower::new(&self, write)
  }
}


pub struct AESShadower {
  /// buffer for cyper uncypher
  buf : Vec<u8>,
  /// index of content writen in buffer but not yet enciphered
  bufix : usize,
  crypter : Crypter,
  /// Symetric key, renew on connect (aka new object)
  key : Vec<u8>,
  /// key to send
  keyexch : Option<Arc<PKey>>,

}

// Crypter is not send but lets try
unsafe impl Send for AESShadower {}

const CIPHER_TAG_AES_256_CBC : u8 = 1;
const AES_256_BLOCK_SIZE : usize = 16;
const AES_256_KEY_SIZE : usize = 32;
const CIPHER_TAG_NOPE : u8 = 0;


/// currently only one scheme (still u8 at 1 in header to support more later).
/// It is AES-256-CBC.
impl AESShadower {

  // TODO make it generic over peer (new fn needed)
  pub fn new(dest : &RSAPeer, write : bool) -> Self {
    let mut rng = OsRng::new().unwrap();

  
    let key = if write {
      let mut s = vec![0; AES_256_KEY_SIZE];
      rng.fill_bytes(&mut s);
      s
    } else {
      Vec::new()
    };


    let buf =  vec![0; AES_256_BLOCK_SIZE];
 

    let crypter = Crypter::new(SHADOW_TYPE);
    crypter.pad(true);

    let enckey = dest.get_pkey().1.clone();
   

    AESShadower {
      buf : buf,
      bufix : 0,
      key : key,
      crypter : crypter,
      keyexch : Some(enckey),
    }
  }
}


impl Shadow for AESShadower {
  type ShadowMode = bool;
  #[inline]
  fn shadow_header<W : Write> (&mut self, w : &mut W, m : &Self::ShadowMode) -> IoResult<()> {

    if *m {
      // change salt each time (if to heavy a counter to change each n time
      try!(w.write(&[CIPHER_TAG_AES_256_CBC]));
      OsRng::new().unwrap().fill_bytes(&mut self.buf);
      self.crypter.init(Mode::Encrypt,&self.key[..],&self.buf[..]);
      try!(w.write(&self.buf[..]));
      match self.keyexch {
        Some(ref apk) => {
          let enckey = RSAPeer::ciph_cont(&(*apk), &self.key[..]);
        println!("{}",self.key.len()); // TODO check size of key
        println!("{}",enckey.len()); // TODO check size of key
      assert!(enckey.len() == 256);
          try!(w.write(&enckey));
        },
        None => (),
      }
      self.keyexch = None;
    } else {
      try!(w.write(&[CIPHER_TAG_NOPE]));
    }
    Ok(())
  }

  #[inline]
  fn read_shadow_header<R : Read> (&mut self, r : &mut R) -> IoResult<Self::ShadowMode> {
    let mut tag = [0];
    try!(r.read(&mut tag));
    if tag[0] == CIPHER_TAG_AES_256_CBC {
      try!(r.read(&mut self.buf[..]));
      match self.keyexch {
        Some(ref apk) => {
          let mut enckey = vec![0;256]; // enc from 32 to 256
          try!(r.read(&mut enckey[..]));
          // init key
          let okey = RSAPeer::unciph_cont(&(*apk), &enckey[..]);
          match okey {
            Some(k) => self.key = k,
            None => return Err(IoError::new (
              IoErrorKind::Other,
              "Cannot read Rsa Shadow key",
            )),
          }

        },
        None => {
        },
      }
      self.keyexch = None;
      self.crypter.init(Mode::Decrypt,&self.key[..],&self.buf[..]);
      Ok(true)
    } else {
      Ok(false)
    }
  }
 
  #[inline]
  fn shadow_iter<W : Write> (&mut self, m : &[u8], w : &mut W, sm : &Self::ShadowMode) -> IoResult<usize> {
    if *sm {
    // best case
    if self.bufix == 0 && m.len() == AES_256_BLOCK_SIZE {
      let r = self.crypter.update(m);
      w.write(&r[..])
    } else {
      let mut has_next = true;
      let mut mu = m;
      let mut res = 0;
      while has_next {
        let nbufix = self.bufix + mu.len();
        if nbufix < AES_256_BLOCK_SIZE {
          has_next = false;
          copy_memory(&mu[..], &mut self.buf[self.bufix..nbufix]);
          res += mu.len();
          self.bufix = nbufix;
        } else {
          let mix = self.buf.len() - self.bufix;
          copy_memory(&mu[..mix], &mut self.buf[self.bufix..]); // TODO no need to copy if same length : update directly on mu then update mu
          mu = &mu[mix..];
          let r = self.crypter.update(&self.buf[..]);
          res += try!(w.write(&r[..]));
          res -= self.bufix;
          self.bufix = 0;
        }

      }
      Ok(res)
    }
    } else {
      w.write(m)
    }
  }

  #[inline]
  fn read_shadow_iter<R : Read> (&mut self, r : &mut R, resbuf: &mut [u8], sm : &Self::ShadowMode) -> IoResult<usize> {
    if *sm {
      // is there something to write
      if self.bufix == 0 {
        let mut s = 0;
        let mut has_next = true;
        // try to fill buf
        while has_next && self.bufix < AES_256_BLOCK_SIZE {
          s = try!(r.read(&mut self.buf[self.bufix..]));
          has_next = s != 0;
          self.bufix += s;
        }
        let dec = if self.bufix == 0 {
          let mut dec = self.crypter.finalize();
          self.crypter.init(Mode::Decrypt,&self.key[..],&self.buf[..]); // only for next finalize to return 0
          if dec.len() == 0 {
            return Ok(0)
          };
          dec
        } else {
          // decode - call directly finalize as we have buf of blocksize
          let mut dec = self.crypter.update(&self.buf[..self.bufix]);
          if dec.len() == 0 {
            // padding possible for last block
        let mut rs = 0;
        let mut has_next = true;
        while has_next  {
          s = try!(r.read(&mut self.buf[rs..]));
          has_next = s != 0;
          rs += s;
        }
 
            if rs > 0 {
              dec = self.crypter.update(&self.buf[..rs]);
            }
            if dec.len() == 0 {
              dec = self.crypter.finalize();
              self.crypter.init(Mode::Decrypt,&self.key[..],&self.buf[..]); // only for next finalize to return 0
            }
          };
           dec
        };

        assert!(dec.len() > 0 && dec.len() <= AES_256_BLOCK_SIZE );
        if dec.len() == AES_256_BLOCK_SIZE {
          self.buf = dec;
          self.bufix = 0;
        } else {
          self.bufix = AES_256_BLOCK_SIZE - dec.len();
          copy_memory(&dec[..],&mut self.buf[self.bufix..]);
        }
      }
      let tow = AES_256_BLOCK_SIZE - self.bufix;
      // write result
      if resbuf.len() < tow {
        copy_memory(&self.buf[self.bufix..self.bufix + resbuf.len()], &mut resbuf[..]);
        self.bufix += resbuf.len();
        Ok(resbuf.len())
      } else {
        copy_memory(&self.buf[self.bufix..], &mut resbuf[..tow]); // TODO case where buf size match and no copy
        self.bufix = 0;
        Ok(tow)
      }
    } else {
      r.read(resbuf)
    }
  }
 
  #[inline]
  fn shadow_flush<W : Write> (&mut self, w : &mut W, sm : &Self::ShadowMode) -> IoResult<()> {
    // encrypt remaining content in buffer and write, + call to writer flush
    if *sm { 
     if self.bufix > 0 {
        let r = self.crypter.update(&self.buf[..self.bufix]);
        try!(w.write(&r[..]));
    }
    // always finalize (padding)
    let r2 = self.crypter.finalize();
    if r2.len() > 0 {
        try!(w.write(&r2[..]));
    }
    }
    w.flush()
  }
 
  #[inline]
  fn default_message_mode () -> Self::ShadowMode {
    true
  }
  #[inline]
  fn default_auth_mode () -> Self::ShadowMode {
    false
  }

}
#[cfg(test)]
fn rsa_shadower_test (input_length : usize, write_buffer_length : usize,
read_buffer_length : usize, smode : bool) {

  let fromP = RSAPeer::new("from".to_string(), None, utils::sa4(Ipv4Addr::new(127,0,0,1), 9000));
  let toP = RSAPeer::new("to".to_string(), None, utils::sa4(Ipv4Addr::new(127,0,0,1), 9001));
  let mut inputb = vec![0;input_length];
  OsRng::new().unwrap().fill_bytes(&mut inputb);
  let mut output = Cursor::new(Vec::new());
  let input = inputb;
  let mut fromShad = toP.get_shadower(true);
  let mut toShad = toP.get_shadower(false);

  fromShad.shadow_header(&mut output, &smode).unwrap();
  let mut ix = 0;
  while ix < input_length {
    if ix + write_buffer_length < input_length {
      ix += fromShad.shadow_iter(&input[ix..ix + write_buffer_length], &mut output, &smode).unwrap();
    } else {
      ix += fromShad.shadow_iter(&input[ix..], &mut output, &smode).unwrap();
    }
  }
  fromShad.shadow_flush(&mut output, &smode).unwrap();

  let mut inputV = Cursor::new(output.into_inner());

  let mode = toShad.read_shadow_header(&mut inputV).unwrap();
  assert!(smode == mode);


  let mut ix = 0;
  let mut readbuf = vec![0;read_buffer_length];
  while ix < input_length {
    let l = toShad.read_shadow_iter(&mut inputV, &mut readbuf, &smode).unwrap();
    assert!(l!=0);

    println!("{:?}",&input[ix..ix + l]);
    println!("{:?}",&readbuf[..l]);
    assert!(&readbuf[..l] == &input[ix..ix + l]);
    ix += l;
  }

  let l = toShad.read_shadow_iter(&mut inputV, &mut readbuf, &smode).unwrap();
  assert!(l==0);

}
#[test]
fn rsa_shadower1_test () {
  let smode = false;
  let input_length = 256;
  let write_buffer_length = 256;
  let read_buffer_length = 256;
  rsa_shadower_test (input_length, write_buffer_length, read_buffer_length, smode);
}

#[test]
fn rsa_shadower2_test () {
  let smode = false;
  let input_length = 7;
  let write_buffer_length = 256;
  let read_buffer_length = 256;
  rsa_shadower_test (input_length, write_buffer_length, read_buffer_length, smode);
}

#[test]
fn rsa_shadower3_test () {
  let smode = false;
  let input_length = 125;
  let write_buffer_length = 12;
  let read_buffer_length = 68;
  rsa_shadower_test (input_length, write_buffer_length, read_buffer_length, smode);
}
#[test]
fn rsa_shadower4_test () {
  let smode = false;
  let input_length = 125;
  let write_buffer_length = 68;
  let read_buffer_length = 12;
  rsa_shadower_test (input_length, write_buffer_length, read_buffer_length, smode);
}
#[test]
fn rsa_shadower5_test () {
  let smode = true;
  let input_length = AES_256_BLOCK_SIZE;
  let write_buffer_length = AES_256_BLOCK_SIZE;
  let read_buffer_length = AES_256_BLOCK_SIZE;
  rsa_shadower_test (input_length, write_buffer_length, read_buffer_length, smode);
}

#[test]
fn rsa_shadower6_test () {
  let smode = true;
  let input_length = 7;
  let write_buffer_length = 256;
  let read_buffer_length = 256;
  rsa_shadower_test (input_length, write_buffer_length, read_buffer_length, smode);
}

#[test]
fn rsa_shadower7_test () {
  let smode = true;
  let input_length = 125;
  let write_buffer_length = 12;
  let read_buffer_length = 68;
  rsa_shadower_test (input_length, write_buffer_length, read_buffer_length, smode);
}
#[test]
fn rsa_shadower8_test () {
  let smode = true;
  let input_length = 125;
  let write_buffer_length = 68;
  let read_buffer_length = 12;
  rsa_shadower_test (input_length, write_buffer_length, read_buffer_length, smode);
}

