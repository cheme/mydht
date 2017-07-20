//! Openssl trait and shadower for mydht.
//! TODO a shadow mode for header (only asym cyph)

#[cfg(test)]
extern crate mydht_basetest;

use std::marker::PhantomData;
//use mydhtresult::Result as MDHTResult;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};
use rustc_serialize::hex::{ToHex};
use std::io::Result as IoResult;
use openssl::crypto::hash::{Hasher,Type};
use openssl::crypto::pkey::{PKey};
use openssl::crypto::symm::{Crypter,Mode};
use openssl::crypto::symm::Type as SymmType;
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
//use self::time::Timespec;
use readwrite_comp::{
  ExtRead,
  ExtWrite,
};
use mydht_base::keyval::{KeyVal};
use mydht_base::peer::{Shadow};
#[cfg(test)]
use mydht_base::keyval::{Attachment,SettableAttachment};
#[cfg(test)]
use mydht_base::peer::{Peer};
#[cfg(test)]
use self::mydht_basetest::transport::LocalAdd;
#[cfg(test)]
use self::mydht_basetest::shadow::shadower_test;
#[cfg(test)]
use self::mydht_basetest::tunnel::tunnel_test;
#[cfg(test)]
use mydht_base::tunnel::{
  TunnelShadowMode,
  TunnelMode,
};


// firt is public key (to avoid multiple call to ffi just to get it) second is c openssl key
#[derive(Clone)]
/// Additional funtionalites over openssl lib PKey
pub struct PKeyExt(pub Vec<u8>,pub Arc<PKey>);
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
  const HASH_SIGN : Type;
  const RSA_SIZE : usize;
  const SHADOW_TYPE : SymmType;
  const CRYPTER_KEY_ENC_SIZE : usize;
  const CRYPTER_KEY_DEC_SIZE : usize;
  const CRYPTER_BLOCK_SIZE : usize;
  const CRYPTER_KEY_SIZE : usize;

  fn get_pkey<'a>(&'a self) -> &'a PKeyExt;
  fn get_pkey_mut<'a>(&'a mut self) -> &'a mut PKeyExt;
  #[inline]
  fn rsa_content_sign (&self, to_sign : &[u8]) -> Vec<u8> {
    debug!("sign content {:?}", to_sign);
    debug!("with key {:?}", self.get_pkey().0);
    let sig = Self::sign_cont(&(*self.get_pkey().1), to_sign);
    debug!("out sign {:?}", sig);
    sig
  }
  fn rsa_init_content_sign (pk : &PKey, to_sign : &[u8]) -> Vec<u8> {
    Self::sign_cont(pk, to_sign)
  }
  fn rsa_content_check (&self, tocheckenc : &[u8], sign : &[u8]) -> bool {
    // some issue when signing big content so sign hash512 instead TODO recheck on later version
    debug!("chec content {:?}", tocheckenc);
    debug!("with sign {:?}", sign);
    debug!("with key {:?}", self.get_pkey().0);
    let mut digest = Hasher::new(Self::HASH_SIGN);
    digest.write_all(tocheckenc).is_ok() // TODO proper errror??
    && self.get_pkey().1.verify_with_hash(&digest.finish()[..], sign, Self::HASH_SIGN)
  }
  fn rsa_key_check (&self) -> bool {
    let mut digest = Hasher::new(Self::HASH_SIGN);
    digest.write_all(&self.get_pkey().0[..]).is_ok() // TODO return proper error??
    && self.get_key() == digest.finish()
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

}


pub struct AESShadower<RT : RSATruster> {
  /// buffer for cyper uncypher
  buf : Vec<u8>,
  /// index of content writen in buffer but not yet enciphered
  bufix : usize,
  crypter : Crypter,
  /// Symetric key, renew on connect (aka new object)
  key : Vec<u8>,
  /// if crypter was finalize (something wrong need this)
  finalize : bool, 
  /// key to send sym k
  keyexch : Arc<PKey>,
  /// is active
  mode : ASymSymMode,
  _p : PhantomData<RT>,

}

// Crypter is not send but lets try
unsafe impl<RT : RSATruster> Send for AESShadower<RT> {}

// TODO if bincode get include use it over ASYMSYMMode isnstead of those three constant
const SMODE_ASYM_ONLY_ENABLE : u8 = 2;
// TODO 
const SMODE_ENABLE : u8 = 1;
// TODO const in trait
const SMODE_DISABLE : u8 = 0;


/// currently only one scheme (still u8 at 1 in header to support more later).
/// Finalize is called on write_end.
impl<RT : RSATruster> AESShadower<RT> {
  /// TODO redesign AESShadower as an enum (NoShadow, STandard,
  /// Symetric only). : here useless Pkey
  fn new_empty() -> Self {
    let crypter = Crypter::new(RT::SHADOW_TYPE);
    crypter.pad(true);
    // TODO sim should be associated type of shadow or use enum
    let enckey : Arc<PKey> = Arc::new(PKey::new());
    AESShadower {
      buf : Vec::new(),
      bufix : 0,
      // sim key to use len 0 until exchanged
      key : Vec::new(), // 0 length sim key until exchanged
      crypter : crypter,
      finalize : false,
      // asym key to established simkey
      keyexch : enckey,
      mode : ASymSymMode::None,
      _p : PhantomData,
    }
  }
 
  pub fn new(dest : &PKeyExt, _ : bool) -> Self {

    let buf =  vec![0; <RT as RSATruster>::CRYPTER_BLOCK_SIZE];
 

    let crypter = Crypter::new(RT::SHADOW_TYPE);
    crypter.pad(true);
    let enckey = dest.1.clone();
    AESShadower {
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
  fn static_shadow_simkey() -> Vec<u8> {
    let mut rng = OsRng::new().unwrap();
    let mut s = vec![0; <RT as RSATruster>::CRYPTER_KEY_SIZE];
    rng.fill_bytes(&mut s);
    s
  }
  fn static_shadow_salt() -> Vec<u8> {
    let mut rng = OsRng::new().unwrap();
    let mut s = vec![0; <RT as RSATruster>::CRYPTER_BLOCK_SIZE];
    rng.fill_bytes(&mut s);
    s
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
        <RT as RSATruster>::CRYPTER_KEY_DEC_SIZE
      } else {
        <RT as RSATruster>::CRYPTER_KEY_ENC_SIZE
      }
    } else {
      <RT as RSATruster>::CRYPTER_BLOCK_SIZE
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

#[derive(Debug, PartialEq, Eq, Clone,RustcEncodable,RustcDecodable)]
pub enum ASymSymMode {
  /// standard for encoding, a symetric key send into an asym envelope (good for big message)
  ASymSym,
  /// only asym encoding : only use for small messages
  ASymOnly,
  /// No enc
  None,
}

impl<RT : RSATruster> Shadow for AESShadower<RT> {
  type ShadowMode = ASymSymMode; // TODO shadowmode allowing head to be RSA only

  #[inline]
  fn set_mode (&mut self, sm : Self::ShadowMode) {
    self.mode = sm;
    match self.mode {
    ASymSymMode::ASymSym => {
        if self.buf.len() != <RT as RSATruster>::CRYPTER_BLOCK_SIZE {
          self.buf = vec![0;<RT as RSATruster>::CRYPTER_BLOCK_SIZE];
        }
    },
    ASymSymMode::ASymOnly => {
        // set_mode is for write (read is read from header)
        if self.buf.len() != <RT as RSATruster>::CRYPTER_KEY_DEC_SIZE {
          self.buf = vec![0;<RT as RSATruster>::CRYPTER_KEY_DEC_SIZE];
        }
    },
    _ => (),
    }
 
  }
  #[inline]
  fn get_mode (&self) -> Self::ShadowMode {
    self.mode.clone()
  }
  fn send_shadow_simkey<W : Write>(w : &mut W, m : Self::ShadowMode ) -> IoResult<()> {
    match m {

    ASymSymMode::ASymSym => {
      try!(w.write(&[SMODE_ENABLE]));
      let k = AESShadower::<RT>::static_shadow_simkey();
      try!(w.write(&k[..]));
      let salt = AESShadower::<RT>::static_shadow_salt();
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
 
  fn init_from_shadow_simkey<R : Read>(r : &mut R, _ : Self::ShadowMode, write : bool) -> IoResult<Self> {
    let mut res = Self::new_empty();
    let mut tag = [0];
    try!(r.read(&mut tag));
    if tag[0]  == SMODE_DISABLE {
      res.mode = ASymSymMode::None;
      Ok(res)
    } else {
      res.mode = ASymSymMode::ASymSym;
      let mut enckey = vec![0;<RT as RSATruster>::CRYPTER_KEY_SIZE]; // enc from 32 to 256
      try!(r.read(&mut enckey[..]));
      let mut salt = vec![0;<RT as RSATruster>::CRYPTER_BLOCK_SIZE]; // enc from 32 to 256
      try!(r.read(&mut salt[..]));
      if write {
        res.crypter.init(Mode::Encrypt,&enckey[..],&salt[..]);
      } else {
        res.crypter.init(Mode::Decrypt,&enckey[..],&salt[..]);
      };
      if res.buf.len() != <RT as RSATruster>::CRYPTER_BLOCK_SIZE {
          res.buf = vec![0;<RT as RSATruster>::CRYPTER_BLOCK_SIZE];
      }
      res.crypter.pad(true);
      res.key = enckey;
      res.buf = salt;
      res.bufix = 0;
      Ok(res)
    }
  }


}






impl<RT : RSATruster> ExtWrite for AESShadower<RT> {
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
 
      assert!(enckey.len() == <RT as RSATruster>::CRYPTER_KEY_ENC_SIZE);
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

impl<RT : RSATruster> ExtRead for AESShadower<RT> {
  #[inline]
  fn read_header<R : Read>(&mut self, r : &mut R) -> IoResult<()> {
    let mut tag = [0];
    try!(r.read(&mut tag));
    if tag[0] == SMODE_ENABLE {
      try!(r.read_exact(&mut self.buf[..]));
      if self.key.len() == 0 {
          let mut enckey = vec![0;<RT as RSATruster>::CRYPTER_KEY_ENC_SIZE]; // enc from 32 to 256
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
        if self.buf.len() != <RT as RSATruster>::CRYPTER_BLOCK_SIZE {
          self.buf = vec![0;<RT as RSATruster>::CRYPTER_BLOCK_SIZE];
        }

      Ok(())
    } else if tag[0] == SMODE_ASYM_ONLY_ENABLE {
        self.mode = ASymSymMode::ASymOnly;
        if self.buf.len() != <RT as RSATruster>::CRYPTER_KEY_ENC_SIZE {
          self.buf = vec![0;<RT as RSATruster>::CRYPTER_KEY_ENC_SIZE];
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


#[cfg(test)]
#[derive(Debug, PartialEq, Eq, Clone,RustcEncodable,RustcDecodable)]
/// Same as RSAPeer from mydhtwot but transport agnostic
pub struct RSAPeerTest {
  /// key to use to identify peer, derived from publickey it is shorter
  key : Vec<u8>,
  /// is used as id/key TODO maybe two publickey use of a master(in case of compromition)
  publickey : PKeyExt,

  pub address : LocalAdd,

  ////// local info 
  
  /// warning must only be serialized locally!!!
  privatekey : Option<Vec<u8>>,
}

#[cfg(test)]
impl RSAPeerTest {
  pub fn new (address : usize) -> RSAPeerTest {
    let mut pkey = PKey::new();
    pkey.gen(<RSAPeerTest as RSATruster>::RSA_SIZE);

    let private = pkey.save_priv();
    let public  = pkey.save_pub();

    let mut digest = Hasher::new(<RSAPeerTest as RSATruster>::HASH_SIGN);
    digest.write_all(&public[..]).unwrap(); // digest should not panic
    let key = digest.finish();

    RSAPeerTest {
      key : key,
      publickey : PKeyExt(public, Arc::new(pkey)),
      address : LocalAdd(address),
      privatekey : Some(private),
    }
  }
}

#[cfg(test)]
impl KeyVal for RSAPeerTest {
  type Key = Vec<u8>;

  #[inline]
  fn get_key(&self) -> Vec<u8> {
    self.key.clone()
  }
  #[inline]
  fn encode_kv<S:Encoder> (&self, _: &mut S, _ : bool, _ : bool) -> Result<(), S::Error> {
    panic!("not used in tests");
  }
  #[inline]
  fn decode_kv<D:Decoder> (_ : &mut D, _ : bool, _ : bool) -> Result<RSAPeerTest, D::Error> {
    panic!("not used in tests");
  }
  noattachment!();
}

#[cfg(test)]
impl RSATruster for RSAPeerTest {
  const HASH_SIGN : Type = Type::SHA512;
  const RSA_SIZE : usize = 2048;
  const SHADOW_TYPE : SymmType = SymmType::AES_256_CBC;
  const CRYPTER_BLOCK_SIZE : usize = 16;
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

#[cfg(test)]
impl SettableAttachment for RSAPeerTest {}

#[cfg(test)]
impl Peer for RSAPeerTest {
  type Address = LocalAdd;
  type Shadow = AESShadower<RSAPeerTest>;
  #[inline]
  fn to_address(&self) -> LocalAdd {
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
  let input_length = <RSAPeerTest as RSATruster>::CRYPTER_BLOCK_SIZE;
  let write_buffer_length = <RSAPeerTest as RSATruster>::CRYPTER_BLOCK_SIZE;
  let read_buffer_length = <RSAPeerTest as RSATruster>::CRYPTER_BLOCK_SIZE;
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
  //let input_length = <RSAPeerTest as RSATruster>::CRYPTER_BLOCK_SIZE;
  let input_length = <RSAPeerTest as RSATruster>::CRYPTER_BLOCK_SIZE;
  let write_buffer_length = <RSAPeerTest as RSATruster>::CRYPTER_BLOCK_SIZE;
  let read_buffer_length = <RSAPeerTest as RSATruster>::CRYPTER_BLOCK_SIZE;
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
fn tunnel_public_test(nbpeer : usize, tmode : TunnelShadowMode, input_length : usize, write_buffer_length : usize, read_buffer_length : usize, shead : ASymSymMode, scont : ASymSymMode) {
  let tmode = TunnelMode::PublicTunnel((nbpeer as u8) - 1,tmode);
  let mut route = Vec::new();
  let pt = peer_tests();
  for i in 0..nbpeer {
    route.push(pt[i].clone());
  }
  tunnel_test(route, input_length, write_buffer_length, read_buffer_length, tmode, shead, scont); 
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

