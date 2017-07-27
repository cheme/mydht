//! Openssl trait and shadower for mydht.
//! TODO a shadow mode for header (only asym cyph)

#[cfg(test)]
extern crate mydht_basetest;


use mydht_base::keyval::Key as KVContent;
use std::fmt;
use std::cmp::{max,min};
use std::marker::PhantomData;
//use mydhtresult::Result as MDHTResult;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use serde::{Serializer,Serialize,Deserializer,Deserialize};
use serde::de::{Visitor, SeqAccess, MapAccess,Unexpected,DeserializeOwned};
use serde::de;
use serde::ser::SerializeStruct;
use std::io::Result as IoResult;
use openssl::hash::{Hasher,MessageDigest};
use openssl::pkey::{PKey};
use openssl::rsa::Rsa;
use openssl::rsa;
use openssl::symm::{Crypter,Mode};
use openssl::symm::Cipher as SymmType;
use openssl::symm::Cipher;
use rand::os::OsRng;
use rand::Rng;
use std::fmt::{Formatter,Debug};
use std::fmt::Error as FmtError;
use hex::ToHex;
//use std::str::FromStr;
use std::cmp::PartialEq;
use std::cmp::Eq;
use std::sync::Arc;
use std::io::Write;
use std::io::Read;
use std::ops::Deref;
//use self::time::Timespec;
use readwrite_comp::{
  ExtRead,
  ExtWrite,
  ReadDefImpl,
};
use mydht_base::keyval::{KeyVal};
use mydht_base::peer::{ShadowBase,ShadowW,ShadowR};
#[cfg(test)]
use mydht_base::keyval::{Attachment,SettableAttachment};
#[cfg(test)]
use mydht_base::peer::{Peer};
#[cfg(test)]
use self::mydht_basetest::transport::LocalAdd;
#[cfg(test)]
use self::mydht_basetest::shadow::shadower_test;
/*
#[cfg(test)]
use self::mydht_basetest::tunnel::tunnel_test;
#[cfg(test)]
use mydht_base::tunnel::{
  TunnelShadowMode,
  TunnelMode,
};
*/

// firt is public key (to avoid multiple call to ffi just to get it) second is c openssl key
#[derive(Clone)]
/// Additional funtionalites over openssl lib PKey
/// last bool allow serializing private key (it defaults to false and revert to false at each
/// access)
pub struct PKeyExt<RT : OpenSSLConf>(pub Vec<u8>,pub Arc<Rsa>,pub bool,pub PhantomData<RT>);
pub struct PKeyExtSerPri<RT : OpenSSLConf>(pub PKeyExt<RT>);
/*#[derive(Clone,Serialize,Deserialize)]
pub enum KeyType {
  RSA,
  EC,
  DH,
  DSA,
}*/

impl<RT : OpenSSLConf> Debug for PKeyExt<RT> {
  fn fmt (&self, f : &mut Formatter) -> Result<(),FmtError> {
    if self.2 {
      write!(f, "public : {:?} \n private : *********", self.0.to_hex())
    } else {
      //self.2 = false;
      write!(f, "public : {:?} \n private : {:?}", self.0.to_hex(), self.1.private_key_to_der().unwrap_or(Vec::new()).to_hex())
    }
  }
}
/// seems ok (a managed pointer to c struct with drop implemented)
unsafe impl<RT : OpenSSLConf> Send for PKeyExt<RT> {}
/// used in arc
unsafe impl<RT : OpenSSLConf> Sync for PKeyExt<RT> {}

impl<RT : OpenSSLConf> Serialize for PKeyExtSerPri<RT> {
  fn serialize<S:Serializer> (&self, s: S) -> Result<S::Ok, S::Error> {
    let mut state = s.serialize_struct("pkey",2)?;
    state.serialize_field("publickey", &self.0)?;
    let a : Vec<u8> = Vec::new(); // TODO replace by empty vec cst(multiple place)
    state.serialize_field("privatekey", &a)?;
    state.end()
  }
}


impl<RT : OpenSSLConf> Serialize for PKeyExt<RT> {
  fn serialize<S:Serializer> (&self, s: S) -> Result<S::Ok, S::Error> {
    let mut state = s.serialize_struct("pkey",2)?;
    state.serialize_field("publickey", &self.0)?;
    let pk = if self.2 {
      Vec::new()
    } else {
      self.1.private_key_to_der().unwrap_or(Vec::new())
    };
    state.serialize_field("privatekey", &pk)?;
    state.end()
  }
}

impl<'de, RT : OpenSSLConf> Deserialize<'de> for PKeyExt<RT> {
  fn deserialize<D:Deserializer<'de>> (d : D) -> Result<PKeyExt<RT>, D::Error> {
        enum Field { Pub, Priv };

        impl<'de> Deserialize<'de> for Field {
            fn deserialize<D>(deserializer: D) -> Result<Field, D::Error>
                where D: Deserializer<'de>
            {
                struct FieldVisitor;

                impl<'de> Visitor<'de> for FieldVisitor {
                    type Value = Field;

                    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                        formatter.write_str("`publickey` or `privatekey`")
                    }

                    fn visit_str<E>(self, value: &str) -> Result<Field, E>
                        where E: de::Error
                    {
                        match value {
                            "publickey" => Ok(Field::Pub),
                            "privatekey" => Ok(Field::Priv),
                            _ => Err(de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }

                deserializer.deserialize_identifier(FieldVisitor)
            }
        }

        struct PKeyVisitor<RT>(PhantomData<RT>);

        impl<'de,RT : OpenSSLConf> Visitor<'de> for PKeyVisitor<RT> {
            type Value = PKeyExt<RT>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct pkey")
            }

            fn visit_seq<V>(self, mut seq: V) -> Result<Self::Value, V::Error>
                where V: SeqAccess<'de>
            {
                let publickey : &[u8] = seq.next_element()?
                              .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let privatekey : &[u8] = seq.next_element()?
                               .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                let pk = if privatekey.len() > 0 {
                  Rsa::private_key_from_der(privatekey).map_err(|_|
                    de::Error::invalid_value(Unexpected::Bytes(privatekey),&" array byte not pkey"))?
                } else {
                  Rsa::public_key_from_der(publickey).map_err(|_|
                    de::Error::invalid_value(Unexpected::Bytes(publickey),&" array byte not pkey"))?
                };
                Ok(PKeyExt(publickey.to_vec(), Arc::new(pk), false, PhantomData))
            }

            fn visit_map<V>(self, mut map: V) -> Result<PKeyExt<RT>, V::Error>
                where V: MapAccess<'de>
            {
                let mut publickey = None;
                let mut privatekey = None;
                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Pub => {
                            if publickey.is_some() {
                                return Err(de::Error::duplicate_field("publickey"));
                            }
                            publickey = Some(map.next_value()?);
                        }
                        Field::Priv => {
                            if privatekey.is_some() {
                                return Err(de::Error::duplicate_field("privatekey"));
                            }
                            privatekey = Some(map.next_value()?);
                        }
                    }
                }

                let publickey : &[u8] = publickey.ok_or_else(|| de::Error::missing_field("publickey"))?;
                let privatekey : &[u8] = privatekey.ok_or_else(|| de::Error::missing_field("privatekey"))?;
                let pk = if privatekey.len() > 0 {
                  Rsa::private_key_from_der(privatekey).map_err(|_|
                    de::Error::invalid_value(Unexpected::Bytes(privatekey),&" array byte not pkey"))?
                } else {
                  Rsa::public_key_from_der(publickey).map_err(|_|
                    de::Error::invalid_value(Unexpected::Bytes(publickey),&" array byte not pkey"))?
                };
                Ok(PKeyExt(publickey.to_vec(), Arc::new(pk), false, PhantomData))

            }
        }

        const FIELDS: &'static [&'static str] = &["publickey", "privatekey"];
        d.deserialize_struct("pkey", FIELDS, PKeyVisitor(PhantomData))
  }
}

impl<RT : OpenSSLConf> PartialEq for PKeyExt<RT> {
  fn eq (&self, other : &PKeyExt<RT>) -> bool {
    self.0 == other.0
  }
}

impl<RT : OpenSSLConf> Eq for PKeyExt<RT> {}

/// This trait allows any keyval having a rsa pkey and any symm cipher to implement Shadow 
pub trait OpenSSLConf : KVContent {
  fn HASH_SIGN() -> MessageDigest;
  fn HASH_KEY() -> MessageDigest;
  const RSA_SIZE : u32;
//  const KEY_TYPE : KeyType; Only RSA allows encoding data for openssl (currently)
  fn SHADOW_TYPE() -> SymmType;
  const CRYPTER_KEY_ENC_SIZE : usize;
  const CRYPTER_KEY_DEC_SIZE : usize;

  const CRYPTER_ASYM_BUFF_SIZE_ENC : usize;
  const CRYPTER_ASYM_BUFF_SIZE_DEC : usize;
  fn  CRYPTER_BUFF_SIZE() -> usize;
//  const CRYPTER_KEY_SIZE : usize;
/*
  fn get_pkey<'a>(&'a self) -> &'a PKeyExt<Self>;
  fn get_pkey_mut<'a>(&'a mut self) -> &'a mut PKeyExt<Self>;
  fn derive_key (&self, key : &[u8]) -> IoResult<Vec<u8>> {
    let mut digest = Hasher::new(Self::HASH_KEY)?;
    digest.write_all(&self.get_pkey().0[..])?;
    let md = digest.finish2()?;
    Ok(md.to_vec())
  }
*/

/*  #[inline]
  fn ossl_content_sign (&self, to_sign : &[u8]) -> Vec<u8> {
    debug!("sign content {:?}", to_sign);
    debug!("with key {:?}", self.get_pkey().0);
    let sig = Self::sign_cont(&(*self.get_pkey().1), to_sign);
    debug!("out sign {:?}", sig);
    sig
  }
  fn ossl_init_content_sign (pk : &PKey, to_sign : &[u8]) -> Vec<u8> {
    Self::sign_cont(pk, to_sign)
  }
  fn ossl_content_check (&self, tocheckenc : &[u8], sign : &[u8]) -> bool {
    // some issue when signing big content so sign hash512 instead TODO recheck on later version
    debug!("chec content {:?}", tocheckenc);
    debug!("with sign {:?}", sign);
    debug!("with key {:?}", self.get_pkey().0);
    let mut digest = Hasher::new(Self::HASH_SIGN);
    digest.write_all(tocheckenc).is_ok() // TODO proper errror??
    && self.get_pkey().1.verify_with_hash(&digest.finish()[..], sign, Self::HASH_SIGN)
  }
  fn ossl_key_check (&self, key : &[u8]) -> bool {
    let mut digest = Hasher::new(Self::HASH_KEY);
    digest.write_all(&self.get_pkey().0[..]).is_ok() // TODO return proper error??
    && key == digest.finish()
  }

  fn sign_cont(pkey : &PKey, to_sign : &[u8]) -> Vec<u8> {
    let mut digest = Hasher::new(Self::HASH_SIGN);
    match digest.write_all(to_sign) { // TODO return result<vec<u8>>
      Ok(_) => (),
      Err(e) => {
        error!("Rsa peer digest failure : {:?}",e);
        return Vec::new();
      },
    }
    pkey.sign_with_hash(&digest.finish()[..], Self::HASH_SIGN)
  }
  */

}

pub struct OSSLSym<RT : OpenSSLConf> {
    /// sym cripter (use for write or read only
    crypter : Crypter,
    /// Symetric key, renew on connect (aka new object), contain salt if needed
    key : Vec<u8>,
    /// if crypter was finalize (create a new for next)
    finalize : bool,
    buff : Vec<u8>,
    _p : PhantomData<RT>,
}

pub struct OSSLSymW<RT : OpenSSLConf>(pub OSSLSym<RT>);
pub struct OSSLSymR<RT : OpenSSLConf>{
  sym : OSSLSym<RT>,
  underbuf : Option<Vec<u8>>,
  suix : usize,
  euix : usize
}

impl<RT : OpenSSLConf> OSSLSymR<RT> {
  fn from_read_sym(sym : OSSLSym<RT>) -> Self {
    OSSLSymR {
      sym : sym,
      underbuf : None,
      suix : 0,
      euix : 0, 
    }
  }
}
impl<RT : OpenSSLConf> OSSLSym<RT> {
  pub fn new_key() -> IoResult<Vec<u8>> {
    let mut rng = OsRng::new()?;
    let ivl = <RT as OpenSSLConf>::SHADOW_TYPE().iv_len().unwrap_or(0);
    let kl = <RT as OpenSSLConf>::SHADOW_TYPE().key_len();
    let mut s = vec![0; ivl + kl];
    rng.fill_bytes(&mut s);
    Ok(s)
  }
  pub fn new (key : Vec<u8>, send : bool) -> IoResult<OSSLSym<RT>> {
    let ivl = <RT as OpenSSLConf>::SHADOW_TYPE().iv_len().unwrap_or(0);
    let kl = <RT as OpenSSLConf>::SHADOW_TYPE().key_len();
    let mut bufsize = <RT as OpenSSLConf>::CRYPTER_BUFF_SIZE() + <RT as OpenSSLConf>::SHADOW_TYPE().block_size();
    assert!(key.len() == ivl + kl); // TODO replace panic by io error
    let mode = if send {
      Mode::Encrypt
    } else {
      Mode::Decrypt
    };
    let mut crypter = {
      let (iv,k) = key[..].split_at(ivl);
      let piv = if iv.len() == 0 {
        None
      } else {
        Some(iv)
      };
      Crypter::new(
        <RT as OpenSSLConf>::SHADOW_TYPE(),
        mode,
        k,
        piv)
    }?;
    crypter.pad(true);
    Ok(OSSLSym {
      crypter : crypter,
      key : key,
      finalize : true,
      buff : vec![0;bufsize],
      _p : PhantomData,
    })
  }
}

impl<RT : OpenSSLConf> OSSLSymR<RT> {
  pub fn new (key : Vec<u8>) -> IoResult<OSSLSymR<RT>> { 
    Ok(OSSLSymR::from_read_sym(OSSLSym::new(key,false)?))
  }
}
impl<RT : OpenSSLConf> OSSLSymW<RT> {
  pub fn new (key : Vec<u8>) -> IoResult<OSSLSymW<RT>> { 
    Ok(OSSLSymW(OSSLSym::new(key,true)?))
  }

}
impl<RT : OpenSSLConf> ExtRead for OSSLSymR<RT> {

  fn read_header<R : Read>(&mut self, r : &mut R) -> IoResult<()> {
    Ok(())
  }
  fn read_from<R : Read>(&mut self, r : &mut R, buf : &mut[u8]) -> IoResult<usize> {
    if self.euix > self.suix {
      let rem = self.euix - self.suix;
      let tocopy = min(buf.len(),rem);
      &mut buf[..tocopy].clone_from_slice(&self.underbuf.as_mut().unwrap()[self.suix..self.suix + tocopy]);
      self.suix += tocopy;
      Ok(tocopy)
    } else {
      let bs = <RT as OpenSSLConf>::SHADOW_TYPE().block_size();
  //    assert!(buf.len() > bs);
      let (tot,rec) = {
        let dest = if buf.len() > bs {
          &mut buf[..]
        } else {
          if self.underbuf.is_none() {
            // default to double block size
            self.underbuf = Some(vec![0;bs + bs]);
          }
          &mut self.underbuf.as_mut().unwrap()[..]
        };
        let sread = min(dest.len() - bs, self.sym.buff.len());

        let ir = r.read(&mut self.sym.buff[..sread])?;
        if ir != 0 {
          self.sym.finalize = false;
          let iu = self.sym.crypter.update(&self.sym.buff[..ir], dest)?;
          if iu == 0 {
            (0,true)
          } else {
            (iu,false)
          }
        } else {
          if !self.sym.finalize {
            self.sym.finalize = true;
            (self.sym.crypter.finalize(dest)?,false)
          } else {
            (0,false)
          }
        }
      };
      if buf.len() <= bs && tot > 0 {
        self.euix = tot;
        self.suix = 0;
        let tocopy = min(buf.len(),tot);
        buf.clone_from_slice(&self.underbuf.as_mut().unwrap()[self.suix..tocopy]);
        self.suix += tocopy;
        Ok(tocopy)
      } else {
      if rec {
        //recurse
        self.read_from(r,buf)
      } else {
        Ok(tot)
      }}
    }
  }
  fn read_end<R : Read>(&mut self, r : &mut R) -> IoResult<()> {
    Ok(())
  }
   
}
impl<RT : OpenSSLConf> ExtWrite for OSSLSymW<RT> {
  fn write_header<W : Write>(&mut self, w : &mut W) -> IoResult<()> {
    Ok(())
  }
  fn write_into<W : Write>(&mut self, w : &mut W, cont : &[u8]) -> IoResult<usize> {

    let bs = <RT as OpenSSLConf>::SHADOW_TYPE().block_size();
    let swrite = min(cont.len(), self.0.buff.len() - bs);
    let iu = self.0.crypter.update(&cont[..swrite], &mut self.0.buff[..])?;
    self.0.finalize = false;
    if iu != 0 {
      w.write_all(&self.0.buff[..iu])?;
    }
    Ok(swrite)

  }
  #[inline]
  fn flush_into<W : Write>(&mut self, w : &mut W) -> IoResult<()> {
    Ok(())
  }
  fn write_end<W : Write>(&mut self, w : &mut W) -> IoResult<()> {
    if !self.0.finalize {
      let i = self.0.crypter.finalize(&mut self.0.buff[..])?;
      self.0.finalize = true;
      if i > 0 {
        w.write_all(&self.0.buff[..i])
      } else { Ok(()) }
    } else {
      // TODO add a warning (write end call twice)
      Ok(())
    }
  }
}

pub struct OSSLMixR<RT : OpenSSLConf> {
  keyexch : PKeyExt<RT>,
  sym : Option<OSSLSymR<RT>>,
  _p : PhantomData<RT>,
}

impl<RT : OpenSSLConf> OSSLMixR<RT> {
  pub fn new (pk : PKeyExt<RT>) -> OSSLMixR<RT> {
    OSSLMixR {
      keyexch : pk,
      sym : None,
      _p : PhantomData,
    }
  }
}

impl<RT : OpenSSLConf> ExtRead for OSSLMixR<RT> {

  fn read_header<R : Read>(&mut self, r : &mut R) -> IoResult<()> {
    let is = <RT as OpenSSLConf>::SHADOW_TYPE().iv_len().unwrap_or(0);
    let ks = <RT as OpenSSLConf>::SHADOW_TYPE().key_len();
    let ksbuf = max(ks, self.keyexch.1.size());

    // TODO if other use out of header put in osslmixr
    let mut ivk = vec![0;is + ksbuf];
    if is > 0 {
      r.read_exact(&mut ivk[..is])?;
    }
    // allways reinit sym crypter (not the case in previous impl
    //if self.key.len() == 0 {
    let mut enckey = vec![0;<RT as OpenSSLConf>::CRYPTER_KEY_ENC_SIZE]; // enc from 32 to 256
    r.read_exact(&mut enckey[..])?;
       /*let mut s = 0;
       while s < enckey.len() {
         let r =  try!(r.read(&mut enckey[s..]));
            if r == 0 {
                return Err(IoError::new (
                  IoErrorKind::Other,
                  "Cannot read Rsa Shadow key",
                ));
            };
            s += r;
          }*/
        // init key
     let kdl = self.keyexch.1.private_decrypt(&enckey[..],&mut ivk[is..],rsa::PKCS1_PADDING)?;

     if kdl != ks {
       return Err(IoError::new (
         IoErrorKind::Other,
         "Cannot read Rsa Shadow key",
       ));
     }
     ivk.truncate(is + ks);
     let sym = OSSLSymR::new(ivk)?;
     self.sym = Some(sym);
     Ok(())
  }
  fn read_from<R : Read>(&mut self, r : &mut R, buf : &mut[u8]) -> IoResult<usize> {
    match self.sym {
      Some(ref mut s) => s.read_from(r,buf),
      None => Err(IoError::new (
         IoErrorKind::Other,
         "Non initialize sym cipher",
       )),
    }
  }
  fn read_exact_from<R : Read>(&mut self, r : &mut R, buf : &mut[u8]) -> IoResult<()> {
    match self.sym {
      Some(ref mut s) => s.read_exact_from(r,buf),
      None => Err(IoError::new (
         IoErrorKind::Other,
         "Non initialize sym cipher",
       )),
    }
  }
  fn read_end<R : Read>(&mut self, r : &mut R) -> IoResult<()> {
    Ok(())
  }
}
pub struct OSSLMixW<RT : OpenSSLConf> {
  dest : PKeyExt<RT>,
  sym : Option<OSSLSymW<RT>>,
  _p : PhantomData<RT>,
}
impl<RT : OpenSSLConf> OSSLMixW<RT> {
  pub fn new (pk : PKeyExt<RT>) -> IoResult<OSSLMixW<RT>> {
    Ok(OSSLMixW {
      dest : pk,
      sym : None,
      _p : PhantomData,
    })
  }
}


impl<RT : OpenSSLConf> ExtWrite for OSSLMixW<RT> {
  fn write_header<W : Write>(&mut self, w : &mut W) -> IoResult<()> {
    let is = <RT as OpenSSLConf>::SHADOW_TYPE().iv_len().unwrap_or(0);
    let ks = <RT as OpenSSLConf>::SHADOW_TYPE().key_len();
    let ivk = <OSSLSym<RT>>::new_key()?;
    w.write_all(&ivk[..is])?;
    let mut enckey = vec![0;<RT as OpenSSLConf>::CRYPTER_KEY_ENC_SIZE];
    let ekeyl = self.dest.1.public_encrypt(&ivk[is..], &mut enckey[..], rsa::PKCS1_PADDING)?;
    assert!(ekeyl == <RT as OpenSSLConf>::CRYPTER_KEY_ENC_SIZE);
    w.write_all(&enckey[..])?;
    
    let sym = OSSLSymW::new(ivk)?;
    self.sym = Some(sym);
    Ok(())
  }

  fn write_into<W : Write>(&mut self, w : &mut W, cont : &[u8]) -> IoResult<usize> {
    match self.sym {
      Some(ref mut s) => s.write_into(w,cont),
      None => Err(IoError::new (
         IoErrorKind::Other,
         "Non initialize sym cipher",
       )),
    }
  }
  fn write_all_into<W : Write>(&mut self, w : &mut W, cont : &[u8]) -> IoResult<()> {
    match self.sym {
      Some(ref mut s) => s.write_all_into(w,cont),
      None => Err(IoError::new (
         IoErrorKind::Other,
         "Non initialize sym cipher",
       )),
    }
  }

  fn flush_into<W : Write>(&mut self, w : &mut W) -> IoResult<()> {
    match self.sym {
      Some(ref mut s) => s.flush_into(w),
      None => Err(IoError::new (
         IoErrorKind::Other,
         "Non initialize sym cipher",
       )),
    }
  }
 
  fn write_end<W : Write>(&mut self, w : &mut W) -> IoResult<()> {
    match self.sym {
      Some(ref mut s) => s.write_end(w),
      None => Err(IoError::new (
         IoErrorKind::Other,
         "Non initialize sym cipher",
       )),
    }
  }

}

/// Shadower based upon openssl symm and pky
pub struct OSSLShadowerR<RT : OpenSSLConf> {
    inner : OSSLMixR<RT>,
    mode : ASymSymMode,
    asymbufs : Option<(Vec<u8>,usize,Vec<u8>,usize)>,
}

impl<RT : OpenSSLConf> OSSLShadowerR<RT> {
  pub fn new (pk : PKeyExt<RT>) -> IoResult<Self> {
     Ok(OSSLShadowerR {
      inner : OSSLMixR::new(pk),
      mode : ASymSymMode::None,
      asymbufs : None,
    })
  }
}
impl<RT : OpenSSLConf> OSSLShadowerW<RT> {
  pub fn new (pk : PKeyExt<RT>) -> IoResult<Self> {
     Ok(OSSLShadowerW {
      inner : OSSLMixW::new(pk)?,
      mode : ASymSymMode::None,
      asymbufs : None,
    })
  }
}


pub struct OSSLShadowerW<RT : OpenSSLConf> {
    inner : OSSLMixW<RT>,
    mode : ASymSymMode,
    asymbufs : Option<(Vec<u8>,usize,Vec<u8>,usize)>,
}

#[derive(PartialEq,Eq,Debug,Clone,Serialize,Deserialize)]
pub enum ASymSymMode {
  ASymSym,
  ASymOnly,
  None,
}
// Crypter is not send but lets try
unsafe impl<RT : OpenSSLConf> Send for OSSLShadowerW<RT> {}
unsafe impl<RT : OpenSSLConf> Send for OSSLShadowerR<RT> {}

impl<RT : OpenSSLConf> ExtRead for OSSLShadowerR<RT> {
  #[inline]
  fn read_header<R : Read>(&mut self, r : &mut R) -> IoResult<()> {
    let mut tag = [0];
    try!(r.read(&mut tag));

    if tag[0] == SMODE_ENABLE {
      self.mode = ASymSymMode::ASymSym;
      self.inner.read_header(r)?;
    } else if tag[0] == SMODE_ASYM_ONLY_ENABLE {
      self.mode = ASymSymMode::ASymOnly;
      if self.asymbufs == None {
        let benc = vec![0;<RT as OpenSSLConf>::CRYPTER_ASYM_BUFF_SIZE_ENC];
        let bdec = vec![0;max(<RT as OpenSSLConf>::CRYPTER_ASYM_BUFF_SIZE_DEC,<RT as OpenSSLConf>::CRYPTER_ASYM_BUFF_SIZE_ENC)];
        self.asymbufs = Some((benc,0,bdec,0));
      }
    } else {
      self.mode = ASymSymMode::None;
    }
    Ok(())
  }
 
  #[inline]
  fn read_from<R : Read>(&mut self, r : &mut R, buf : &mut[u8]) -> IoResult<usize> {
    match self.mode {
      ASymSymMode::ASymSym => {
        self.inner.read_from(r,buf)
      },
      ASymSymMode::ASymOnly => {
        if let Some((ref mut benc, ref mut decixstart, ref mut bdec, ref mut decixend)) = self.asymbufs {
          if decixend == decixstart {
            *decixstart = 0;
            *decixend = 0;
            // no content to return, produce
            let mut encix = 0;
            while {
              let s = r.read(&mut benc[encix..])?;
              encix += s;
              s != 0
            } { }
            if encix == 0 {
              return Ok(0)
            }
            *decixend = self.inner.keyexch.1.private_decrypt(&benc[..encix], &mut bdec[..], rsa::PKCS1_PADDING)?;
          }
      
          let tocopy = min(buf.len(), *decixend - *decixstart);
          buf[..tocopy].clone_from_slice(&bdec[*decixstart..*decixstart + tocopy]);
          *decixstart += tocopy;
          Ok(tocopy)
        } else {
         Err(IoError::new (
          IoErrorKind::Other,
          "Asym buf reader",
          ))
        }
      },
      ASymSymMode::None => {
        r.read(buf)
      },
    }
  }
   #[inline]
  fn read_exact_from<R : Read>(&mut self, r : &mut R, buf : &mut[u8]) -> IoResult<()> {
    match self.mode {
      ASymSymMode::ASymSym => {
        self.inner.read_exact_from(r,buf)
      },
      ASymSymMode::ASymOnly => {
        // default trait impl
        let mut def = ReadDefImpl(self);
        def.read_exact_from(r,buf)
      },
      ASymSymMode::None => {
        r.read_exact(buf)
      },
    }
  }
 
  #[inline]
  fn read_end<R : Read>(&mut self, r : &mut R) -> IoResult<()> {
    match self.mode {
      ASymSymMode::ASymSym => self.inner.read_end(r)?,
      ASymSymMode::ASymOnly => (),
      ASymSymMode::None => (),
    }
    Ok(())
  }
}

impl<RT : OpenSSLConf> ExtWrite for OSSLShadowerW<RT> {

  fn write_header<W : Write>(&mut self, w : &mut W) -> IoResult<()> {
    match self.mode {
      ASymSymMode::ASymSym => {
        w.write(&[SMODE_ENABLE])?;
        self.inner.write_header(w)?;
      },
      ASymSymMode::ASymOnly => {
        w.write(&[SMODE_ASYM_ONLY_ENABLE])?;
        if self.asymbufs == None {
          let benc = vec![0;<RT as OpenSSLConf>::CRYPTER_ASYM_BUFF_SIZE_ENC];
          let bdec = vec![0;<RT as OpenSSLConf>::CRYPTER_ASYM_BUFF_SIZE_DEC];
          self.asymbufs = Some((benc,0,bdec,0));
        }
      },
      ASymSymMode::None => {
        w.write(&[SMODE_DISABLE])?;
      },
    }
    Ok(())
  }

  fn write_into<W : Write>(&mut self, w : &mut W, cont : &[u8]) -> IoResult<usize> {
    match self.mode {
      ASymSymMode::ASymSym => {
        self.inner.write_into(w,cont)
      },
      ASymSymMode::ASymOnly => {
        self.flush_into(w)?;
        if let Some((ref mut benc, _, ref mut bdec, ref mut decixend)) = self.asymbufs {
          let tocopy = min(bdec.len() - *decixend, cont.len());
          bdec[*decixend..*decixend + tocopy].clone_from_slice(&cont[..tocopy]);
          *decixend += tocopy;
      
          Ok(tocopy)
        } else {
         Err(IoError::new (
          IoErrorKind::Other,
          "Asym buf reader",
          ))
        }
      },
      ASymSymMode::None => {
        w.write(cont)
      },
    }
  }

  fn flush_into<W : Write>(&mut self, w : &mut W) -> IoResult<()> {
    match self.mode {
      ASymSymMode::ASymSym => self.inner.flush_into(w)?,
      ASymSymMode::ASymOnly => {
        if let Some((ref mut benc, _, ref mut bdec, ref mut decixend)) = self.asymbufs {
          // do not flush midbuffer with padding,warn use in write_into
          if *decixend == bdec.len() {
            let encix = self.inner.dest.1.public_encrypt(&bdec[..], &mut benc[..], rsa::PKCS1_PADDING)?;
            *decixend = 0;
            w.write_all(&benc[..encix])?;
          }
        } else {
         return Err(IoError::new (
          IoErrorKind::Other,
          "Asym buf reader",
          ))
        }
      },
      ASymSymMode::None => (),
    }

    Ok(())
  }
 
  fn write_end<W : Write>(&mut self, w : &mut W) -> IoResult<()> {
    match self.mode {
      ASymSymMode::ASymSym => self.inner.write_end(w)?,
      ASymSymMode::ASymOnly => {
        self.flush_into(w)?;
        if let Some((ref mut benc, _, ref mut bdec, ref mut decixend)) = self.asymbufs {
          // do not flush midbuffer with padding,warn use in write_into
          if *decixend != 0 {
            let encix = self.inner.dest.1.public_encrypt(&bdec[..*decixend], &mut benc[..], rsa::PKCS1_PADDING)?;
            *decixend = 0;
            w.write_all(&benc[..encix])?;
          }
        } else {
         return Err(IoError::new (
          IoErrorKind::Other,
          "Asym buf reader",
          ))
        }
      },
      ASymSymMode::None => (),
    }
    Ok(())
  }

}

impl<RT : OpenSSLConf> ShadowW for OSSLShadowerW<RT> {}
impl<RT : OpenSSLConf> ShadowR for OSSLShadowerR<RT> {}

// TODO if bincode get include use it over ASYMSYMMode isnstead of those three constant
const SMODE_ASYM_ONLY_ENABLE : u8 = 2;
// TODO 
const SMODE_ENABLE : u8 = 1;
// TODO const in trait
const SMODE_DISABLE : u8 = 0;


impl<RT : OpenSSLConf> PKeyExt<RT> {

  pub fn new(pk : Arc<Rsa>) -> Self {

    let pubk = pk.public_key_to_der().unwrap();
    PKeyExt(pubk,pk,false,PhantomData)
  }
  pub fn derive_key (&self) -> IoResult<Vec<u8>> {
      let mut digest = Hasher::new(<RT as OpenSSLConf>::HASH_KEY())?;
      digest.write_all(&self.0[..])?;
      let md = digest.finish2()?;
      Ok(md.to_vec())
  }


}

impl<RT : OpenSSLConf> ShadowBase for OSSLShadowerR<RT> {
  type ShadowMode = ASymSymMode; // TODO shadowmode allowing head to be RSA only

  #[inline]
  fn set_mode (&mut self, sm : Self::ShadowMode) {
    self.mode = sm;
  }
  #[inline]
  fn get_mode (&self) -> Self::ShadowMode {
    self.mode.clone()
  }
}
impl<RT : OpenSSLConf> ShadowBase for OSSLShadowerW<RT> {
  type ShadowMode = ASymSymMode;
  #[inline]
  fn set_mode (&mut self, sm : Self::ShadowMode) {
    self.mode = sm;
  }
  #[inline]
  fn get_mode (&self) -> Self::ShadowMode {
    self.mode.clone()
  }
}







#[cfg(feature="mydhtimpl")]
#[cfg(test)]
pub mod mydhttest {
  use super::*;

#[derive(Debug,PartialEq,Eq,Clone,Serialize,Deserialize)]
/// Same as RSAPeer from mydhtwot but transport agnostic
pub struct RSAPeerTest<I> {
  /// key to use to identify peer, derived from publickey it is shorter
  key : Vec<u8>,
  /// is used as id/key TODO maybe two publickey use of a master(in case of compromition)
  publickey : PKeyExt<RSAPeerConf>,

  pub address : LocalAdd,

  ////// local info
  pub peerinfo : I,
  
}

impl<I> RSAPeerTest<I> {
  pub fn new (address : usize, info : I) -> IoResult<RSAPeerTest<I>> {
    let pkeyrsa = Rsa::generate(<RSAPeerConf as OpenSSLConf>::RSA_SIZE)?;

    let pkeyext = PKeyExt::new(Arc::new(pkeyrsa));
    let key = pkeyext.derive_key()?;

    Ok(RSAPeerTest {
      key : key,
      publickey : pkeyext,
      address : LocalAdd(address),
      peerinfo : info,
    })
  }
}


impl<I : KVContent> KeyVal for RSAPeerTest<I> {
  type Key = Vec<u8>;

  #[inline]
  fn get_key(&self) -> Vec<u8> {
    self.key.clone()
  }
  #[inline]
  fn encode_kv<S:Serializer> (&self, _: S, _ : bool, _ : bool) -> Result<S::Ok, S::Error> {
    panic!("not used in tests");
  }
  #[inline]
  fn decode_kv<'de,D:Deserializer<'de>> (_ : D, _ : bool, _ : bool) -> Result<RSAPeerTest<I>, D::Error> {
    panic!("not used in tests");
  }
  noattachment!();
}

#[derive(PartialEq,Eq,Debug,Clone,Serialize,Deserialize)]
pub struct RSAPeerConf;

impl OpenSSLConf for RSAPeerConf {
  #[inline]
  fn HASH_SIGN() -> MessageDigest { MessageDigest::sha512() }
  #[inline]
  fn HASH_KEY() -> MessageDigest { MessageDigest::sha512() }
  const RSA_SIZE : u32 = 2048;
//  const KEY_TYPE : KeyType = KeyType::RSA;
  #[inline]
  fn SHADOW_TYPE() -> Cipher { Cipher::aes_256_cbc()}
//  const CRYPTER_KEY_SIZE : usize = 32;
  /// size must allow no padding
  const CRYPTER_KEY_ENC_SIZE : usize = 256;
  /// size must allow no padding
  const CRYPTER_KEY_DEC_SIZE : usize = 214;
  #[inline]
  fn CRYPTER_BUFF_SIZE() -> usize { Self::SHADOW_TYPE().block_size() }// TODO try bigger

  /// padding is use
  const CRYPTER_ASYM_BUFF_SIZE_ENC : usize = 256;
  /// padding is use
  const CRYPTER_ASYM_BUFF_SIZE_DEC : usize = 214;
}

impl<I> SettableAttachment for RSAPeerTest<I> {}

impl<I : KVContent> Peer for RSAPeerTest<I> {
  type Address = LocalAdd;
  type ShadowW = OSSLShadowerW<RSAPeerConf>;
  type ShadowR = OSSLShadowerR<RSAPeerConf>;
  #[inline]
  fn get_address(&self) -> &LocalAdd {
    &self.address
  }
 
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
}

fn rsa_shadower_test (input_length : usize, write_buffer_length : usize,
read_buffer_length : usize, smode : ASymSymMode) {

  let to_p = RSAPeerTest::new(1,()).unwrap();
  shadower_test(to_p,input_length,write_buffer_length,read_buffer_length,smode);

}

#[test]
fn rsa_shadower1_test () {
  let smode = ASymSymMode::None;
  let input_length = 256;
  let write_buffer_length = 256;
  let read_buffer_length = 256;
  rsa_shadower_test (input_length, write_buffer_length, read_buffer_length, smode);
}

#[test]
fn rsa_shadower2_test () {
  let smode = ASymSymMode::None;
  let input_length = 7;
  let write_buffer_length = 256;
  let read_buffer_length = 256;
  rsa_shadower_test (input_length, write_buffer_length, read_buffer_length, smode);
}

#[test]
fn rsa_shadower3_test () {
  let smode = ASymSymMode::None;
  let input_length = 125;
  let write_buffer_length = 12;
  let read_buffer_length = 68;
  rsa_shadower_test (input_length, write_buffer_length, read_buffer_length, smode);
}


#[test]
fn rsa_shadower4_test () {
  let smode = ASymSymMode::None;
  let input_length = 125;
  let write_buffer_length = 68;
  let read_buffer_length = 12;
  rsa_shadower_test (input_length, write_buffer_length, read_buffer_length, smode);
}
#[test]
fn rsa_shadower5_test () {
  let smode = ASymSymMode::ASymSym;
  let input_length = RSAPeerConf::SHADOW_TYPE().block_size();
  let write_buffer_length = RSAPeerConf::SHADOW_TYPE().block_size();
  let read_buffer_length = RSAPeerConf::SHADOW_TYPE().block_size();
  rsa_shadower_test (input_length, write_buffer_length, read_buffer_length, smode);
}

#[test]
fn rsa_shadower6_test () {
  let smode = ASymSymMode::ASymSym;
  let input_length = 7;
  let write_buffer_length = 256;
  let read_buffer_length = 256;
  rsa_shadower_test (input_length, write_buffer_length, read_buffer_length, smode);
}

#[test]
fn rsa_shadower7_test () {
  let smode = ASymSymMode::ASymSym;
  let input_length = 125;
  let write_buffer_length = 12;
  let read_buffer_length = 68;
  rsa_shadower_test (input_length, write_buffer_length, read_buffer_length, smode);
}

#[test]
fn rsa_shadower8_test () {
  let smode = ASymSymMode::ASymSym;
  let input_length = 125;
  let write_buffer_length = 68;
  let read_buffer_length = 12;
  rsa_shadower_test (input_length, write_buffer_length, read_buffer_length, smode);
}
#[test]
fn rsa_shadower9_test () {
  let smode = ASymSymMode::ASymOnly;
  //let input_length = <RSAPeerTest as OpenSSLConf>::CRYPTER_BLOCK_SIZE;
  let input_length = RSAPeerConf::SHADOW_TYPE().block_size();
  let write_buffer_length = RSAPeerConf::SHADOW_TYPE().block_size();
  let read_buffer_length = RSAPeerConf::SHADOW_TYPE().block_size();
  rsa_shadower_test (input_length, write_buffer_length, read_buffer_length, smode);
}
#[test]
fn rsa_shadowera_test () {
  let smode = ASymSymMode::ASymOnly;
  let input_length = 7;
  let write_buffer_length = 256;
  let read_buffer_length = 256;
  rsa_shadower_test (input_length, write_buffer_length, read_buffer_length, smode);
}

#[test]
fn rsa_shadowerb_test () {
  let smode = ASymSymMode::ASymOnly;
  let input_length = 125;
  let write_buffer_length = 12;
  let read_buffer_length = 68;
  rsa_shadower_test (input_length, write_buffer_length, read_buffer_length, smode);
}

#[test]
fn rsa_shadowerc_test () {
  let smode = ASymSymMode::ASymOnly;
  let input_length = 700;
  let write_buffer_length = 68;
  let read_buffer_length = 12;
  rsa_shadower_test (input_length, write_buffer_length, read_buffer_length, smode);
}

#[cfg(test)]
fn peer_tests () -> Vec<RSAPeerTest<()>> {
[ 
  RSAPeerTest::new(1,()).unwrap(),
  RSAPeerTest::new(2,()).unwrap(),
  RSAPeerTest::new(3,()).unwrap(),
  RSAPeerTest::new(4,()).unwrap(),
  RSAPeerTest::new(5,()).unwrap(),
  RSAPeerTest::new(6,()).unwrap(),
].to_vec()
}

/*
 * Tunnel refactor : should be use as a mydht transport
#[cfg(test)]
fn tunnel_public_test(nbpeer : usize, tmode : TunnelShadowMode, input_length : usize, write_buffer_length : usize, read_buffer_length : usize, shead : ASymSymMode, scont : ASymSymMode) {
  let tmode = TunnelMode::PublicTunnel((nbpeer as u8) - 1,tmode);
  let mut route = Vec::new();
  let pt = peer_tests();
  for i in 0..nbpeer {
    route.push(pt[i].clone());
  }
  tunnel_test(route, input_length, write_buffer_length, read_buffer_length, tmode, shead, scont); 
}
#[test]
fn tunnel_nohop_publictunnel_1() {
  tunnel_public_test(2, TunnelShadowMode::Last, 500, 360, 130, ASymSymMode::ASymOnly, ASymSymMode::ASymSym);
}
#[test]
fn tunnel_onehop_publictunnel_1() {
  tunnel_public_test(3, TunnelShadowMode::Last, 500, 360, 130, ASymSymMode::ASymSym, ASymSymMode::ASymSym);
}
#[test]
fn tunnel_onehop_publictunnel_2() {
  tunnel_public_test(3, TunnelShadowMode::Full, 500, 130, 360, ASymSymMode::ASymSym, ASymSymMode::ASymSym);
}
#[test]
fn tunnel_fourhop_publictunnel_2() {
  tunnel_public_test(6, TunnelShadowMode::Full, 500, 130, 360, ASymSymMode::ASymSym, ASymSymMode::ASymSym);
}
#[test]
fn tunnel_fourhop_publictunnel_3() {
  tunnel_public_test(4, TunnelShadowMode::Last, 500, 130, 360, ASymSymMode::ASymOnly, ASymSymMode::ASymSym);
}
*/
}
/*
#[cfg(test)]
pub mod commontest {
use super::*;
#[test]
fn asym_test () {
    let mut pkey = Rsa::generate(2048);
    let input = [1,2,3,4,5];
    let out = pkey.public_encrypt(&input);
    let in2 = pkey.private_decrypt(&out);
    assert_eq!(&input[..],&in2[..]);
    let out = pkey.public_encrypt(&input);
    let in2 = pkey.private_decrypt(&out);
    assert_eq!(&input[..],&in2[..]);
    let out = pkey.public_encrypt(&input);
    let in2 = pkey.private_decrypt(&out);
    assert_eq!(&input[..],&in2[..]);
    let input_length = 500;
    let buff = 214; // max buf l TODO check in impl
    let mut inputb = vec![0;input_length];
    OsRng::new().unwrap().fill_bytes(&mut inputb);
    let mut ix = 0;
//    let mut tot = 0;
    while ix < input_length {
    let out = if ix + buff < input_length {
      pkey.public_encrypt(&inputb[ix..ix + buff])
    } else {
      pkey.public_encrypt(&inputb[ix..])
    };
//    tot += out.len();
    let in2 = pkey.private_decrypt(&out);
    if ix + buff < input_length {
    assert_eq!(&inputb[ix..ix + buff],&in2[..]);
    } else {
    assert_eq!(&inputb[ix..],&in2[..]);
    };
    ix += buff;
    }
//    assert!(false)
 
 
  }

}*/
