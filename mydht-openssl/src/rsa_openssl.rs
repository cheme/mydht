//! Openssl trait and shadower for mydht.
//! TODO a shadow mode for header (only asym cyph)

#[cfg(test)]
extern crate mydht_basetest;

use std::cmp::min;
use std::marker::PhantomData;
//use mydhtresult::Result as MDHTResult;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use serde::{Serializer,Serialize,Deserializer,Deserialize};
use std::io::Result as IoResult;
use openssl::hash::{Hasher,MessageDigest};
use openssl::pkey::{PKey};
use openssl::rsa;
use openssl::symm::{Crypter,Mode};
use openssl::symm::Cipher as SymmType;
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
};
use mydht_base::keyval::{KeyVal};
use mydht_base::peer::{ShadowBase,Shadow,ShadowSim};
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
pub struct PKeyExt<RT : OpenSSLConf>(pub Vec<u8>,pub Arc<PKey>,pub bool,pub PhantomData<RT>);
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
      self.2 = false;
      write!(f, "public : {:?} \n private : {:?}", self.0.to_hex(), self.1.save_priv().to_hex())
    }
  }
}
impl<RT : OpenSSLConf> Deref for PKeyExt<RT> {
  type Target = PKey;
  #[inline]
  fn deref<'a> (&'a self) -> &'a PKey {
    &self.1
  }
}
/// seems ok (a managed pointer to c struct with drop implemented)
unsafe impl<RT : OpenSSLConf> Send for PKeyExt<RT> {}
/// used in arc
unsafe impl<RT : OpenSSLConf> Sync for PKeyExt<RT> {}

impl<RT : OpenSSLConf> Serialize for PKeyExt<RT> {
  fn serialize<S:Serializer> (&self, s: &mut S) -> Result<S::Ok, S::Error> {
    s.emit_struct("pkey",2, |s| {
/*      s.emit_struct_field("type", 0, |s|{
        self.3.encode(s)
      })?;*/
      s.emit_struct_field("publickey", 0, |s|{
        self.0.encode(s)
      })?;
      if self.2 {
        s.emit_struct_field("privatekey", 1, |s|{
          self.1.save_priv().encode(s)
        })?;
      } else {
        self.2 = false;
        s.emit_struct_field("privatekey", 1, |s|{
          Vec::new().encode(s)
        })?;

      }
    })
  }
}

impl<'de, RT : OpenSSLConf> Deserialize<'de> for PKeyExt<RT> {
  fn deserialize<D:Deserializer<'de>> (d : &mut D) -> Result<PKeyExt<RT>, D::Error> {
    d.read_struct("pkey",2, |d| {
/*      let ktype : Result<KeyType, D::Error> = d.read_struct_field("type", 0, |d|{
        Deserialize::decode(d)
      });*/
 
      let publickey : Result<Vec<u8>, D::Error> = d.read_struct_field("publickey", 0, |d|{
        Deserialize::decode(d)
      });
      let privatekey : Result<Vec<u8>, D::Error>= d.read_struct_field("privatekey", 1, |d|{
        Deserialize::decode(d)
      });
      publickey.and_then(move |puk| privatekey.map (move |prk| {
        let mut res = PKey::new();
        res.load_pub(&puk[..]);
        if prk.len() > 0 {
          res.load_priv(&prk[..]);
        }
        PKeyExt(puk, Arc::new(res), false, PhantomData::new())
      }))
    })
  }
}

impl<RT : OpenSSLConf> PartialEq for PKeyExt<RT> {
  fn eq (&self, other : &PKeyExt) -> bool {
    self.0.public_eq(&other.0)
  }
}

impl<RT : OpenSSLConf> Eq for PKeyExt<RT> {}


/// This trait allows any keyval having a rsa pkey and any symm cipher to implement Shadow 
pub trait OpenSSLConf {
  const HASH_SIGN : MessageDigest;
  const HASH_KEY : MessageDigest;
  const RSA_SIZE : usize;
//  const KEY_TYPE : KeyType; Only RSA allows encoding data for openssl (currently)
  const SHADOW_TYPE : SymmType;
  const CRYPTER_KEY_ENC_SIZE : usize;
  const CRYPTER_KEY_DEC_SIZE : usize;
  const CRYPTER_BUFF_SIZE : usize;
  const CRYPTER_KEY_SIZE : usize;

  fn get_pkey<'a>(&'a self) -> &'a PKeyExt;
  fn get_pkey_mut<'a>(&'a mut self) -> &'a mut PKeyExt;
  fn derive_key (&self, key : &[u8]) -> IoResult<Vec<u8>> {
    let mut digest = Hasher::new(Self::HASH_KEY);
    digest.write_all(&self.get_pkey().0[..])?;
    digest.finish()
  }


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
pub struct OSSLSymR<RT : OpenSSLConf>(pub OSSLSym<RT>);

impl<RT : OpenSSLConf> OSSLSym<RT> {
  pub fn new_key() -> IoResult<Vec<u8>> {
    let mut rng = OsRng::new()?;
    let ivl = <RT as OpenSSLConf>::SHADOW_TYPE.iv_len();
    let kl = <RT as OpenSSLConf>::SHADOW_TYPE.key_len();
    let mut s = vec![0; ivl + kl];
    rng.fill_bytes(&mut s);
    s
  }
  pub fn new (key : Vec<u8>, send : bool) -> IoResult<OSSLSym<RT>> {
    let ivl = <RT as OpenSSLConf>::SHADOW_TYPE.iv_len();
    let kl = <RT as OpenSSLConf>::SHADOW_TYPE.key_len();
    let bufsize = <RT as OpenSSLConf>::CRYPTER_BUFF_SIZE; + <RT as OpenSSLConf>::SHADOW_TYPE.block_size();
    assert!(key.len() == ivl + kl); // TODO replace panic by io error
    let mode = if send {
      Mode::Encrypt
    } else {
      Mode::Decrypt
    };
    let crypter = {
      let (iv,k) = key[..].split_at(kl);
      let piv = if iv.len() == 0 {
        None
      } else {
        Some(iv)
      };
      Crypter::new(
        <RT as OpenSSLConf>::SHADOW_TYPE,
        mode,
        k,
        iv)?
    };
    crypter.pad(true);
    Ok(OSSLSym {
      crypter : crypter,
      key : key,
      finalize : false,
      buff : vec![0;bufsize],
      _p : PhantomData,
    })
  }
}
impl<RT : OpenSSLConf> ExtRead for OSSLSymR<RT> {

  fn read_header<R : Read>(&mut self, r : &mut R) -> IoResult<()> {
    Ok(())
  }
  fn read_from<R : Read>(&mut self, r : &mut R, buf : &mut[u8]) -> IoResult<usize> {
    let bs = <RT as OpenSSLConf>::SHADOW_TYPE.block_size();
    assert!(buf.len() > bs);
    let sread = min(buf.len() - bs, self.buff.len());
    let ir = r.read(&mut self.buff[..sread])?;
    if ir != 0 {
      self.finalize = false;
      let iu = self.crypter.update(&self.buff[..ir], buf)?;
      if iu == 0 {
        // avoid returning 0 if possible
        self.read_from(r, buf)
      } else {
        Ok(iu)
      }
    } else {
      if !self.finalize {
        self.finalize = true;
        self.finalize(buf)
      } else {
        Ok(0)
      }
    }
  }
  fn read_end<R : Read>(&mut self, r : &mut R) -> IoResult<()> {
    Ok(())
  }
   
}
impl<RT : OpenSSLConf> ExtWrite for OSSLSymW<RT> {
  fn write_header<W : Write>(&mut self, w : &mut W) -> IoResult<()> {
    self.finalize = false;
    Ok(())
  }
  fn write_into<W : Write>(&mut self, w : &mut W, cont : &[u8]) -> IoResult<usize> {

    let bs = <RT as OpenSSLConf>::SHADOW_TYPE.block_size();
    let swrite = min(cont.len(), self.buff.len() - bs);
    let iu = self.crypter.update(&cont[..swrite], &mut self.buff[..])?;
    if iu != 0 {
      w.write_all(&self.buff[..iu])?;
    }
    Ok(swrite);

  }
  #[inline]
  fn flush_into<W : Write>(&mut self, _ : &mut W) -> IoResult<()> {
    Ok(())
  }
  fn write_end<W : Write>(&mut self, w : &mut W) -> IoResult<()> {
    if !self.finalize {
      let i = self.crypter.finalize(&mut self.buff[..]);
      self.finalize = true;
      if i > 0 {
        w.write_all(&self.buff[..i])
      }
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
TODO impl extr and shadow r with mode ()
impl<RT : OpenSSLConf> ExtRead for OSSLMixR<RT> {
}
pub struct OSSLMixW<RT : OpenSSLConf> {
  keyexch : PKeyExt<RT>,
  sym : OSSLSymW<RT>,
  _p : PhantomData<RT>,
}

impl<RT : OpenSSLConf> ExtWrite for OSSLMixW<RT> {
}

/// Shadower based upon openssl symm and pky
pub struct OSSLShadowerR<RT : OpenSSLConf> {
    inner : OSSLMixR<RT>,
    mode : ASymSymMode,
}
pub struct OSSLShadowerW<RT : OpenSSLConf> {
    inner : OSSLMixW<RT>,
    mode : ASymSymMode,
}

pub enum ASymSymMode {
  ASymSym,
  ASymOnly,
  None,
}
// Crypter is not send but lets try
unsafe impl<RT : OpenSSLConf> Send for OSSLShadowerW<RT> {}
unsafe impl<RT : OpenSSLConf> Send for OSSLShadowerR<RT> {}

impl<RT : OpenSSLConf> ExtRead for OSSLShadower<RT> {
  #[inline]
  fn read_header<R : Read>(&mut self, r : &mut R) -> IoResult<()> {
    let mut tag = [0];
    try!(r.read(&mut tag));
    if tag[0] == SMODE_ENABLE {
      try!(r.read_exact(&mut self.buf[..]));
      if self.key.len() == 0 {
          let mut enckey = vec![0;<RT as OpenSSLConf>::CRYPTER_KEY_ENC_SIZE]; // enc from 32 to 256
          let mut s = 0;
          while s < enckey.len() {
            let r =  try!(r.read(&mut enckey[s..]));
            if r == 0 {
                return Err(IoError::new (
                  IoErrorKind::Other,
                  "Cannot read Rsa Shadow key",
                ));
            };
            s += r;
          }
          // init key
          let k = (*self.keyexch).decrypt(&enckey[..]);
          if k.len() == 0 {
             return Err(IoError::new (
                  IoErrorKind::Other,
                  "Cannot read Rsa Shadow key",
               ));
          }

          self.key = k
      }
      
      self.crypter.init(Mode::Decrypt,&self.key[..],&self.buf[..]);
        self.bufix = 0;
        self.mode = ASymSymMode::ASymSym;
        if self.buf.len() != <RT as OpenSSLConf>::SHADOW_TYPE.block_size() {
          self.buf = vec![0;<RT as OpenSSLConf>::SHADOW_TYPE.block_size()];
        }

      Ok(())
    } else if tag[0] == SMODE_ASYM_ONLY_ENABLE {
        self.mode = ASymSymMode::ASymOnly;
        if self.buf.len() != <RT as OpenSSLConf>::CRYPTER_KEY_ENC_SIZE {
          self.buf = vec![0;<RT as OpenSSLConf>::CRYPTER_KEY_ENC_SIZE];
        };
        self.bufix = 0;
      Ok(())
    } else {
        self.mode = ASymSymMode::None;
      Ok(())
    }
  }
 
  #[inline]
  fn read_from<R : Read>(&mut self, r : &mut R, buf : &mut[u8]) -> IoResult<usize> {
    self.read_shadow_iter_sim (r, buf)
  }
 
  #[inline]
  fn read_end<R : Read>(&mut self, _ : &mut R) -> IoResult<()> {
    self.bufix = 0;
    Ok(())
  }
}


// TODO if bincode get include use it over ASYMSYMMode isnstead of those three constant
const SMODE_ASYM_ONLY_ENABLE : u8 = 2;
// TODO 
const SMODE_ENABLE : u8 = 1;
// TODO const in trait
const SMODE_DISABLE : u8 = 0;


impl<RT : OpenSSLConf> PKeyExt<RT> {

}
/// currently only one scheme (still u8 at 1 in header to support more later).
/// Finalize is called on write_end.
impl<RT : OpenSSLConf> OSSLShadower<RT> {
 
  pub fn new(dest : &PKeyExt, _ : bool) -> Self {

    let buf =  vec![0; <RT as OpenSSLConf>::CRYPTER_BLOCK_SIZE];
 

    let crypter = Crypter::new(RT::SHADOW_TYPE);
    crypter.pad(true);
    let enckey = dest.1.clone();
    OSSLShadower {
      buf : buf,
      bufix : 0,
      // sim key to use len 0 until exchanged
      key : Vec::new(), // 0 length sim key until exchanged
      crypter : crypter,
      finalize : false,
      // asym key to established simkey
      keyexch : enckey,
      mode : ASymSymMode::ASymSym, // default to ASymSym
      _p : PhantomData,
    }
  }



  #[inline]
  fn crypt (&mut self, m : &[u8], sym : bool) -> Vec<u8> {
    if sym {
      self.crypter.update(m)
    } else {
      (*self.keyexch).encrypt(m)
    }
  }
  #[inline]
  fn crypt_buf (&mut self, bix : usize, sym : bool) -> Vec<u8> {
    if sym {
      self.crypter.update(&self.buf[..bix])
    } else {
      (*self.keyexch).encrypt(&self.buf[..bix])
    }
  }
  #[inline]
  fn buf_size (&mut self, w : bool) -> usize {
    if let ASymSymMode::ASymOnly = self.mode {
      if w {
        <RT as OpenSSLConf>::CRYPTER_KEY_DEC_SIZE
      } else {
        <RT as OpenSSLConf>::CRYPTER_KEY_ENC_SIZE
      }
    } else {
      <RT as OpenSSLConf>::SHADOW_TYPE::block_size() 
    }
  }

  /// warning iter sim does not use a salt, please add one if needed (common use case is one key
  /// per message and salt is useless)
  /// TODO add try everywhere plus check if readwrite ext is not better indexwise
  fn shadow_iter_sim<W : Write> (&mut self, m : &[u8], w : &mut W) -> IoResult<usize> {

    match self.mode {
    ASymSymMode::ASymSym | ASymSymMode::ASymOnly => {
      let sym = self.mode == ASymSymMode::ASymSym;
      let bsize = self.buf_size(true);
    // best case
    if self.bufix == 0 && m.len() == bsize {
      let r = self.crypt(m,sym);
      w.write(&r[..])
    } else {
      let mut has_next = true;
      let mut mu = m;
      let mut res = 0;
      while has_next {
        let nbufix = self.bufix + mu.len();
        if nbufix < bsize {
          has_next = false;
          self.buf[self.bufix..nbufix].clone_from_slice(&mu[..]);
          res += mu.len();
          self.bufix = nbufix;
        } else {
          let mix = self.buf.len() - self.bufix;
          self.buf[self.bufix..].clone_from_slice(&mu[..mix]); // TODO no need to copy if same length : update directly on mu then update mu
          mu = &mu[mix..];
          let r = self.crypt_buf(bsize,sym);
          try!(w.write_all(&r[..]));
          res += mix;
          self.bufix = 0;
        }
      }
      Ok(res)
    }
    },
    ASymSymMode::None => {
      w.write(m)
    },
    }
  }

  #[inline]
  fn decrypt_buf (&mut self, ix : usize, sym : bool) -> Vec<u8> {
    if sym {
      self.crypter.update(&self.buf[..ix])
    } else {
      (*self.keyexch).decrypt(&self.buf[..ix])
    }
  }
 #[inline]
  fn dfinalize (&mut self, sym : bool) -> Vec<u8> {
    if sym {
      self.crypter.finalize()
    } else {
      Vec::new()
    }
  }


  /// same as write no salt (key should be unique or read a salt
  fn read_shadow_iter_sim<R : Read> (&mut self, r : &mut R, resbuf: &mut [u8]) -> IoResult<usize> {
    match self.mode {
    ASymSymMode::ASymSym | ASymSymMode::ASymOnly => {
      let bsize = self.buf_size(false);
      let sym = self.mode == ASymSymMode::ASymSym;
   
/*      if self.bufix == self.buf.len() {
        return Ok(0); // has been finalized
      }*/
     // feed buffer (one block length) if empty
      if resbuf.len() != 0 && self.bufix == 0 {
        if self.finalize {
          return Ok(0);
        }
 
        let mut s = 1;
        // try to fill buf
        while s != 0 {
          s = try!(r.read(&mut self.buf[self.bufix..]));
          self.bufix += s;
        }
        let dec = if self.bufix < bsize {
          // nothing to read, finalize
          let dec = self.dfinalize(sym);
          self.finalize = true;
//          self.crypter.init(Mode::Decrypt,&self.key[..],&self.buf[..]); // only for next finalize to return 0
          if dec.len() == 0 {
            return Ok(0)
          };
          dec
        } else {
          // decode - call directly finalize as we have buf of blocksize
          let bix = self.bufix;
          let dec = self.decrypt_buf(bix,sym);
          if dec.len() == 0 {
            self.bufix = 0;
            return self.read_shadow_iter_sim(r, resbuf);
          };
          dec
        };

        assert!(dec.len() > 0 && dec.len() <= bsize );
        if dec.len() == bsize {
          self.buf = dec;
          self.bufix = 0;
        } else {
          self.bufix = bsize - dec.len();
          self.buf[self.bufix..].clone_from_slice(&dec[..]);
        }
      }
      let tow = bsize - self.bufix;
      // write result
      if resbuf.len() < tow {
        let l = self.bufix + resbuf.len();
        resbuf[..].clone_from_slice(&self.buf[self.bufix..l]);
        self.bufix += resbuf.len();
        Ok(resbuf.len())
      } else {
        resbuf[..tow].clone_from_slice(&self.buf[self.bufix..]); // TODO case where buf size match and no copy
        self.bufix = 0;
        Ok(tow)
      }
    },
    ASymSymMode::None => {
      r.read(resbuf)
    },
    }
 
  }
  fn c_finalize<W : Write> (&mut self, w : &mut W) -> IoResult<()> {
    // encrypt remaining content in buffer and write, + call to writer flush
    match self.mode {
    ASymSymMode::ASymSym | ASymSymMode::ASymOnly => {
      let sym = self.mode == ASymSymMode::ASymSym;
      if self.bufix > 0 {
        let bix = self.bufix;
        let r = self.crypt_buf(bix,sym);
        try!(w.write(&r[..]));
      }
      // always finalize (padding)
      let r2 = self.dfinalize(sym);
      if r2.len() > 0 {
        try!(w.write(&r2[..]));
      }
      self.finalize = false;
    },
    _ => (),
    }
    //w.flush()
    Ok(())
  }


}

impl<RT : OpenSSLConf> Shadow for OSSLShadower<RT> {
  type ShadowMode = ASymSymMode; // TODO shadowmode allowing head to be RSA only

  #[inline]
  fn set_mode (&mut self, sm : Self::ShadowMode) {
    self.mode = sm;
    match self.mode {
    ASymSymMode::ASymSym => {
        if self.buf.len() != <RT as OpenSSLConf>::CRYPTER_BLOCK_SIZE {
          self.buf = vec![0;<RT as OpenSSLConf>::CRYPTER_BLOCK_SIZE];
        }
    },
    ASymSymMode::ASymOnly => {
        // set_mode is for write (read is read from header)
        if self.buf.len() != <RT as OpenSSLConf>::CRYPTER_KEY_DEC_SIZE {
          self.buf = vec![0;<RT as OpenSSLConf>::CRYPTER_KEY_DEC_SIZE];
        }
    },
    _ => (),
    }
 
  }
  #[inline]
  fn get_mode (&self) -> Self::ShadowMode {
    self.mode.clone()
  }
}

impl<RT : OpenSSLConf> ShadowSim for OSSLShadower<RT> {
  fn send_shadow_simkey<W : Write>(w : &mut W, m : <Self as Shadow>::ShadowMode ) -> IoResult<()> {
    match m {

    ASymSymMode::ASymSym => {
      try!(w.write(&[SMODE_ENABLE]));
      let k = OSSLShadower::<RT>::static_shadow_simkey();
      try!(w.write(&k[..]));
      let salt = OSSLShadower::<RT>::static_shadow_salt();
      try!(w.write(&salt[..]));
    },
    ASymSymMode::ASymOnly => {
      try!(w.write(&[SMODE_ASYM_ONLY_ENABLE]));
    },
    ASymSymMode::None => {
      try!(w.write(&[SMODE_DISABLE]));
    },
    }
    Ok(())
  }
 
  fn init_from_shadow_simkey<R : Read>(r : &mut R, _ : <Self as Shadow>::ShadowMode, write : bool) -> IoResult<Self> {
    let mut res = Self::new_empty();
    let mut tag = [0];
    try!(r.read(&mut tag));
    if tag[0]  == SMODE_DISABLE {
      res.mode = ASymSymMode::None;
      Ok(res)
    } else {
      res.mode = ASymSymMode::ASymSym;
      let mut enckey = vec![0;<RT as OpenSSLConf>::CRYPTER_KEY_SIZE]; // enc from 32 to 256
      try!(r.read(&mut enckey[..]));
      let mut salt = vec![0;<RT as OpenSSLConf>::CRYPTER_BLOCK_SIZE]; // enc from 32 to 256
      try!(r.read(&mut salt[..]));
      if write {
        res.crypter.init(Mode::Encrypt,&enckey[..],&salt[..]);
      } else {
        res.crypter.init(Mode::Decrypt,&enckey[..],&salt[..]);
      };
      if res.buf.len() != <RT as OpenSSLConf>::CRYPTER_BLOCK_SIZE {
          res.buf = vec![0;<RT as OpenSSLConf>::CRYPTER_BLOCK_SIZE];
      }
      res.crypter.pad(true);
      res.key = enckey;
      res.buf = salt;
      res.bufix = 0;
      Ok(res)
    }
  }
}


impl<RT : OpenSSLConf> ExtWrite for OSSLShadower<RT> {
  #[inline]
  fn write_header<W : Write>(&mut self, w : &mut W) -> IoResult<()> {
    match self.mode {
    ASymSymMode::ASymSym => {
 
      // change salt each time (if to heavy a counter to change each n time
      try!(w.write(&[SMODE_ENABLE]));
      OsRng::new().unwrap().fill_bytes(&mut self.buf);
      try!(w.write(&self.buf[..]));
    if self.key.len() == 0 { // TODO renew of simkey on write end??
      self.key = Self::static_shadow_simkey();
      let enckey = (*self.keyexch).encrypt(&self.key[..]);
 
      assert!(enckey.len() == <RT as OpenSSLConf>::CRYPTER_KEY_ENC_SIZE);
          try!(w.write(&enckey));
    }

      self.crypter.init(Mode::Encrypt,&self.key[..],&self.buf[..]);
      self.bufix = 0;
    },
    ASymSymMode::None => {

      try!(w.write(&[SMODE_DISABLE]));
    },
    ASymSymMode::ASymOnly => {
      try!(w.write(&[SMODE_ASYM_ONLY_ENABLE]));
      self.bufix = 0;
    },
    }
    Ok(())
  }

  #[inline]
  fn write_into<W : Write>(&mut self, w : &mut W, cont : &[u8]) -> IoResult<usize> {
    self.shadow_iter_sim (cont, w)
  }

  #[inline]
  fn flush_into<W : Write>(&mut self, _ : &mut W) -> IoResult<()> {
    Ok(())
  }
 
  #[inline]
  fn write_end<W : Write>(&mut self, w : &mut W) -> IoResult<()> {
    if let ASymSymMode::None = self.mode {
      //w.flush()
      Ok(())
    } else {
      self.c_finalize(w)
    }
  }

}


impl<RT : OpenSSLConf> ExtRead for OSSLShadower<RT> {
  #[inline]
  fn read_header<R : Read>(&mut self, r : &mut R) -> IoResult<()> {
    let mut tag = [0];
    try!(r.read(&mut tag));
    if tag[0] == SMODE_ENABLE {
      try!(r.read_exact(&mut self.buf[..]));
      if self.key.len() == 0 {
          let mut enckey = vec![0;<RT as OpenSSLConf>::CRYPTER_KEY_ENC_SIZE]; // enc from 32 to 256
          let mut s = 0;
          while s < enckey.len() {
            let r =  try!(r.read(&mut enckey[s..]));
            if r == 0 {
                return Err(IoError::new (
                  IoErrorKind::Other,
                  "Cannot read Rsa Shadow key",
                ));
            };
            s += r;
          }
          // init key
          let k = (*self.keyexch).decrypt(&enckey[..]);
          if k.len() == 0 {
             return Err(IoError::new (
                  IoErrorKind::Other,
                  "Cannot read Rsa Shadow key",
               ));
          }

          self.key = k
      }
      
      self.crypter.init(Mode::Decrypt,&self.key[..],&self.buf[..]);
        self.bufix = 0;
        self.mode = ASymSymMode::ASymSym;
        if self.buf.len() != <RT as OpenSSLConf>::CRYPTER_BLOCK_SIZE {
          self.buf = vec![0;<RT as OpenSSLConf>::CRYPTER_BLOCK_SIZE];
        }

      Ok(())
    } else if tag[0] == SMODE_ASYM_ONLY_ENABLE {
        self.mode = ASymSymMode::ASymOnly;
        if self.buf.len() != <RT as OpenSSLConf>::CRYPTER_KEY_ENC_SIZE {
          self.buf = vec![0;<RT as OpenSSLConf>::CRYPTER_KEY_ENC_SIZE];
        };
        self.bufix = 0;
      Ok(())
    } else {
        self.mode = ASymSymMode::None;
      Ok(())
    }
  }
  #[inline]
  fn read_from<R : Read>(&mut self, r : &mut R, buf : &mut[u8]) -> IoResult<usize> {
    self.read_shadow_iter_sim (r, buf)
  }
 
  #[inline]
  fn read_end<R : Read>(&mut self, _ : &mut R) -> IoResult<()> {
    self.bufix = 0;
    Ok(())
  }
}

#[cfg(feature="mydht-impl")]
#[cfg(test)]
pub mod mydhttest {
  use super::*;

#[derive(Debug, PartialEq, Eq, Clone,Serialize,Deserialize)]
/// Same as RSAPeer from mydhtwot but transport agnostic
pub struct RSAPeerTest<I> {
  /// key to use to identify peer, derived from publickey it is shorter
  key : Vec<u8>,
  /// is used as id/key TODO maybe two publickey use of a master(in case of compromition)
  publickey : PKeyExt,

  pub address : LocalAdd,

  ////// local info
  pub peerinfo : I,
  
}

#[cfg(feature="mydht-impl")]
#[cfg(test)]
impl<I> RSAPeerTest<I> {
  pub fn new (address : usize, info : I) -> RSAPeerTest<I> {
    let mut pkey = PKey::new();
    pkey.gen(<RSAPeerTest as OpenSSLConf>::RSA_SIZE);

    let private = pkey.save_priv();
    let public  = pkey.save_pub();

    let mut digest = Hasher::new(<RSAPeerTest as OpenSSLConf>::HASH_SIGN);
    digest.write_all(&public[..]).unwrap(); // digest should not panic
    let key = digest.finish();

    RSAPeerTest {
      key : key,
      publickey : PKeyExt(public, Arc::new(pkey)),
      address : LocalAdd(address),
      peerinfo : I,
    }
  }
}

#[cfg(feature="mydht-impl")]
#[cfg(test)]
impl<I> KeyVal for RSAPeerTest<I> {
  type Key = Vec<u8>;

  #[inline]
  fn get_key(&self) -> Vec<u8> {
    self.key.clone()
  }
  #[inline]
  fn encode_kv<S:Serializer> (&self, _: &mut S, _ : bool, _ : bool) -> Result<(), S::Error> {
    panic!("not used in tests");
  }
  #[inline]
  fn decode_kv<'de,D:Deserializer<'de>> (_ : D, _ : bool, _ : bool) -> Result<RSAPeerTest, D::Error> {
    panic!("not used in tests");
  }
  noattachment!();
}

#[cfg(feature="mydht-impl")]
#[cfg(test)]
impl<I> OpenSSLConf for RSAPeerTest<I> {
  const HASH_SIGN : MessageDigest = MessageDigest::sha512();
  const RSA_SIZE : usize = 2048;
//  const KEY_TYPE : KeyType = KeyType::RSA;
  const SHADOW_TYPE : SymmType = SymmType::aes_256_cbc();
  const CRYPTER_KEY_SIZE : usize = 32;
  const CRYPTER_KEY_ENC_SIZE : usize = 256;
  const CRYPTER_KEY_DEC_SIZE : usize = 214;

#[inline]
  fn get_pkey<'a>(&'a self) -> &'a PKeyExt {
    &self.publickey
  }
#[inline]
  fn get_pkey_mut<'a>(&'a mut self) -> &'a mut PKeyExt {
    &mut self.publickey
  }
}

#[cfg(feature="mydht-impl")]
#[cfg(test)]
impl<I> SettableAttachment for RSAPeerTest<I> {}

#[cfg(feature="mydht-impl")]
#[cfg(test)]
impl<I> Peer for RSAPeerTest<I> {
  type Address = LocalAdd;
  type Shadow = OSSLShadower<RSAPeerTest>;
  #[inline]
  fn to_address(&self) -> LocalAdd {
    self.address.clone()
  }
  #[inline]
  fn get_shadower (&self, write : bool) -> Self::Shadow {
    OSSLShadower::new(self.get_pkey(), write)
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

#[cfg(feature="mydht-impl")]
#[cfg(test)]
fn rsa_shadower_test (input_length : usize, write_buffer_length : usize,
read_buffer_length : usize, smode : ASymSymMode) {

  let to_p = RSAPeerTest::new(1);
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
  let input_length = <RSAPeerTest as OpenSSLConf>::SHADOW_TYPE.block_size();
  let write_buffer_length = <RSAPeerTest as OpenSSLConf>::SHADOW_TYPE.block_size();
  let read_buffer_length = <RSAPeerTest as OpenSSLConf>::SHADOW_TYPE.block_size();
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
  let input_length = <RSAPeerTest as OpenSSLConf>::SHADOW_TYPE.block_size();
  let write_buffer_length = <RSAPeerTest as OpenSSLConf>::SHADOW_TYPE.block_size();
  let read_buffer_length = <RSAPeerTest as OpenSSLConf>::SHADOW_TYPE.block_size();
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
fn peer_tests () -> Vec<RSAPeerTest> {
[ 
  RSAPeerTest::new(1),
  RSAPeerTest::new(2),
  RSAPeerTest::new(3),
  RSAPeerTest::new(4),
  RSAPeerTest::new(5),
  RSAPeerTest::new(6),
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

#[cfg(test)]
pub mod commontest {
use super::*;
#[test]
fn asym_test () {
    let mut pkey = PKey::new();
    pkey.gen(2048);
    let input = [1,2,3,4,5];
    let out = pkey.encrypt(&input);
    let in2 = pkey.decrypt(&out);
    assert_eq!(&input[..],&in2[..]);
    let out = pkey.encrypt(&input);
    let in2 = pkey.decrypt(&out);
    assert_eq!(&input[..],&in2[..]);
    let out = pkey.encrypt(&input);
    let in2 = pkey.decrypt(&out);
    assert_eq!(&input[..],&in2[..]);
    let input_length = 500;
    let buff = 214; // max buf l TODO check in impl
    let mut inputb = vec![0;input_length];
    OsRng::new().unwrap().fill_bytes(&mut inputb);
    let mut ix = 0;
//    let mut tot = 0;
    while ix < input_length {
    let out = if ix + buff < input_length {
      pkey.encrypt(&inputb[ix..ix + buff])
    } else {
      pkey.encrypt(&inputb[ix..])
    };
//    tot += out.len();
    let in2 = pkey.decrypt(&out);
    if ix + buff < input_length {
    assert_eq!(&inputb[ix..ix + buff],&in2[..]);
    } else {
    assert_eq!(&inputb[ix..],&in2[..]);
    };
    ix += buff;
    }
//    assert!(false)
 
 
  }

}
