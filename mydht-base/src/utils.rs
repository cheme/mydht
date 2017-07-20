extern crate time;
use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};
use std::ops::Deref;
use std::str::FromStr;
use std::net::SocketAddr;
use std::net::SocketAddrV4;
use std::net::SocketAddrV6;
use std::net::Ipv4Addr;
use std::net::Ipv6Addr;
use num::bigint::{BigUint,RandBigInt};
use std::env;
use std::path::{Path,PathBuf};
use std::fs::{self,File};
use std::io::Result as IoResult;
//use rand::Rng;
use rand::thread_rng;

use std::sync::{Arc,Mutex,Condvar};
use std::fmt::{Formatter,Debug};
use std::fmt::Error as FmtError;
use std::time::Duration;
use self::time::Timespec;
use keyval::KeyVal;
use keyval::FileKeyVal;
use keyval::Attachment;
use keyval::SettableAttachment;

#[cfg(not(feature="openssl-impl"))]
#[cfg(feature="rust-crypto-impl")]
use std::io::{
  Seek,
  SeekFrom,
  Read,
};
#[cfg(not(feature="openssl-impl"))]
#[cfg(feature="rust-crypto-impl")]
use self::crypto::sha2::Sha256;
#[cfg(not(feature="openssl-impl"))]
#[cfg(feature="rust-crypto-impl")]
use self::crypto::digest::Digest;
#[cfg(feature="openssl-impl")]
use self::openssl::crypto::hash::{Hasher,Type};
#[cfg(test)]
use std::thread;

pub static NULL_TIMESPEC : Timespec = Timespec{ sec : 0, nsec : 0};

pub fn is_in_tmp_dir(f : &Path) -> bool {
//  Path::new(os::tmpdir().to_string()).is_ancestor_of(f)
  // TODO usage of start_with instead of is_ancestor_of not tested
  f.starts_with(&env::temp_dir())
}
 
// TODO rewrite with full new io and new path : this is so awfull + true uuid
// Error management...
pub fn create_tmp_file() -> IoResult<(PathBuf,File)> {
  let tmpdir = env::temp_dir();
  let mytmpdirpath = tmpdir.join(Path::new("./mydht"));
  try!(fs::create_dir_all(&mytmpdirpath));
  let fname = random_uuid(64).to_string();
  let fpath = mytmpdirpath.join(Path::new(&fname[..]));
  debug!("Creating tmp file : {:?}",fpath);
  let f = try!(File::create(&fpath)); 
  Ok((fpath, f))
}

fn random_uuid(hash_size : usize) -> BigUint {
   let mut rng = thread_rng();
   rng.gen_biguint(hash_size)
}



/// serializable option type for transient fields in struct : like option but do not serialize and
/// deserialize to none.
/// Futhermore implement Debug, Show, Eq by not displaying and being allways equal
#[derive(Clone)]
pub struct TransientOption<V> (pub Option<V>);
impl<V> Debug for TransientOption<V> {
  fn fmt (&self, f : &mut Formatter) -> Result<(),FmtError> {
    write!(f, "Skipped transient option")
  }
}

impl<V> Encodable for TransientOption<V> {
  fn encode<S:Encoder> (&self, _: &mut S) -> Result<(), S::Error> {
    Ok(())
  }
}

impl<V> Decodable for TransientOption<V> {
  fn decode<D:Decoder> (_ : &mut D) -> Result<TransientOption<V>, D::Error> {
    Ok(TransientOption(None))
  }
}

impl<V> Eq for TransientOption<V> {}
impl<V> PartialEq for TransientOption<V> {
  #[inline]
  fn eq(&self, _ : &Self) -> bool {
    true
  }
}


// TODO move to Mutex<Option<V>>?? (most of the time it is the sense : boolean, and easier init
// when complex value : default at start to none. TODO init function
/// for receiving one result only from other processes
//pub type OneResult<V : Send> = Arc<(Mutex<V>,Condvar)>;
/// TODO struct with optional val : all oneresult<bool> may switch to oneresult<()>
/// the bool is for spurious wakeup management (is true if it is not a spurious wakeup)
pub type OneResult<V> = Arc<(Mutex<(V,bool)>,Condvar)>;
 
#[inline]
pub fn new_oneresult<V>(v : V) -> OneResult<V>  {
  Arc::new((Mutex::new((v,false)),Condvar::new()))
}

#[inline]
// TODO test in tcp loop
/// TODO return MyDHTResult!!
/// Racy accessor to OneResult value
pub fn one_result_val_clone<V : Clone + Send> (ores : &OneResult<V>) -> Option<V> {
  match ores.0.lock() {
    Ok(res) => Some(res.0.clone()),
    Err(m) => {
      error!("poisoned mutex for ping result : {:?}", m);
      None
    },
  }
}

#[inline]
pub fn one_result_spurious<V> (ores : &OneResult<V>) -> Option<bool> {
  match ores.0.lock() {
    Ok(res) => Some(res.1),
    Err(m) => {
      error!("poisoned mutex for ping result : {:?}", m);
      None
    },
  }
}
 

#[inline]
/// TODO return MyDHTResult!!
/// One result value return
pub fn ret_one_result<V : Send> (ores : &OneResult<V>, v : V) {
  match ores.0.lock() {
    Ok(mut res) => {
      res.1 = true;
      res.0 = v
    },
    Err(m) => error!("poisoned mutex for ping result : {:?}", m),
      
  }
  ores.1.notify_all();
}

/// unlocke TODO removed unused parameter
pub fn unlock_one_result<V : Send> (ores : &OneResult<V>, _ : V) {
  match ores.0.lock() {
    Ok(mut res) => {
      res.1 = true;
    },
    Err(m) => error!("poisoned mutex for ping result : {:?}", m),
  }
  ores.1.notify_all();
}


#[inline]
/// TODO return MyDHTResult!!
/// Racy change value of set result
pub fn change_one_result<V : Send> (ores : &OneResult<V>, v : V) {
  match ores.0.lock() {
    Ok(mut res) => res.0 = v,
    Err(m) => error!("poisoned mutex for ping result : {:?}", m),
  }
}

/// Racy change value but with condition
pub fn change_one_result_ifneq<V : Send + Eq> (ores : &OneResult<V>, neq : &V, v : V) {
  match ores.0.lock() {
    Ok(mut res) => if res.0 != *neq {res.0 = v},
    Err(m) => error!("poisoned mutex for ping result : {:?}", m),
  }
}

#[inline]
/// use only for small clonable stuff or arc it TODO return MyDHTResult!!
/// Second parameter let you specify a new value.
pub fn clone_wait_one_result<V : Clone + Send> (ores : &OneResult<V>, newval : Option<V>) -> Option<V> {
 let r = match ores.0.lock() {
    Ok(mut guard) => {
      let mut res = None;
      loop {
        match ores.1.wait(guard) {
          Ok(mut r) => {
            if r.1 {
              r.1 = false;
              let rv = r.0.clone();
              newval.map(|v| r.0 = v).is_some();
//          Some(*r)
              res = Some(rv);
              break;
            } else {
              debug!("spurious wait");
              guard = r;
            };
          },
          Err(_) => {
            error!("Condvar issue for return res");
            break;
          }, // TODO what to do??? panic?
        }
      };
      res
    },
    Err(m) => {
      error!("poisonned mutex on one res : {:?}", m);
      None
    }, // not logic
 };
 r
}
/// same as clone_wait_one_result but with condition, if condition is not reach value is returned
/// (value will be neq value condition)
pub fn clone_wait_one_result_ifneq<V : Clone + Send + Eq> (ores : &OneResult<V>, neqval : &V, newval : Option<V>) -> Option<V> {
 let r = match ores.0.lock() {
    Ok(mut guard) => {

      if guard.0 != *neqval {
        let mut ret = None;
        loop {
        match ores.1.wait(guard) {
          Ok(mut r) => {
            if r.1 {
              r.1 = false;
              let res = r.0.clone();
              newval.map(|v| r.0 = v).is_some();
//          Some(*r)
              ret = Some(res);
              break;
            } else {
              debug!("spurious wait");
              guard = r;
            }
          }
          Err(_) => {
            error!("Condvar issue for return res"); break;}, // TODO what to do??? panic?
        }
        };
        ret
      } else {
        let res = guard.0.clone();
        newval.map(|v| guard.0 = v).is_some();
        Some(res)
      }
    },
    Err(m) => {error!("poisonned mutex on one res : {:?}", m); None}, // not logic
 };
 r
}

#[inline]
/// same as clone_wait_one_result TODO duration as parameter
pub fn clone_wait_one_result_ifneq_timeout_ms<V : Clone + Send + Eq> (ores : &OneResult<V>, neqval : &V, newval : Option<V>, to : u32) -> Option<V> {
 let r = match ores.0.lock() {
    Ok(mut guard) => {

      if guard.0 != *neqval {
      let mut ret = None;
      loop {
      match ores.1.wait_timeout(guard, Duration::from_millis(to as u64)) {
        
        Ok(mut r) => {
          if !r.1.timed_out() {
            if (r.0).1 {
              (r.0).1  = false;
              let res = (r.0).0.clone();
              newval.map(|v| (r.0).0 = v).is_some();
              ret = Some(res);
              break;
            } else {
              debug!("spurious waitout");
              guard = r.0;
            }
          } else {
            debug!("timeout waiting for oneresult");
            break;
          }
        }
 
        Err(_) => {error!("Condvar issue for return res"); break}, // TODO what to do??? panic?
      }
      };
      ret
      }else {
        let res = guard.0.clone();
        newval.map(|v| guard.0 = v).is_some();
        Some(res)
      }
    },
    Err(m) => {error!("poisonned mutex on one res : {:?}", m); None}, // not logic
 };
 r
}

#[inline]
/// same as clone_wait_one_result but with timeout, return None on timeout TODO duration as
/// parameter
pub fn clone_wait_one_result_timeout_ms<V : Clone + Send> (ores : &OneResult<V>, newval : Option<V>, to : u32) -> Option<V> {
 let r = match ores.0.lock() {
    Ok(mut guard) => {
      let mut ret = None;
      loop {
      match ores.1.wait_timeout(guard, Duration::from_millis(to as u64)) {
        Ok(mut r) => {
          if !r.1.timed_out() {
            if (r.0).1 {
              (r.0).1  = false;
              let res = (r.0).0.clone();
              newval.map(|v| (r.0).0 = v).is_some();
              ret = Some(res);
              break;
            } else {
              debug!("spurious waitout");
              guard = r.0;
            }
          } else {
            debug!("timeout waiting for oneresult");
            break;
          }
        }
        Err(_) => {
          error!("Condvar issue for return res"); break}, // TODO what to do??? panic?
      }
      };
      ret
    },
    Err(m) => {error!("poisonned mutex on one res : {:?}", m); None}, // not logic
 };
 r
}

#[test]
pub fn test_oneresult () {
  

  let or = new_oneresult("testons");

  assert!("testons" == one_result_val_clone (&or).unwrap());

  change_one_result(&or, "testons2");
  assert!("testons2" == one_result_val_clone (&or).unwrap());
  assert!(None == clone_wait_one_result_timeout_ms(&or,Some("testons3"),500));

  ret_one_result(&or, "testons2");
  assert!(None == clone_wait_one_result_timeout_ms(&or,Some("testons3"),500));
  assert!("testons2" == one_result_val_clone (&or).unwrap());
  assert!(Some("testons2") == clone_wait_one_result_ifneq(&or,&"testons2",Some("testons3")));
  assert!("testons3" == one_result_val_clone (&or).unwrap());
  let or2 = or.clone();
  thread::spawn(move || {
    loop{
      // Warning change_one_result need to check if not unlock in mutexguard because it would be
      // racy otherwhise
      change_one_result_ifneq(&or2,&"unlock", "testonsREP");
    }
  });
//  assert!(None == clone_wait_one_result_timeout_ms(&or,Some("testons3"),1000));
//  assert!("testonsREP" == one_result_val_clone (&or).unwrap());
  let or3 = or.clone();
  thread::spawn(move || {
    change_one_result(&or3, "testonsREP");
    ret_one_result(&or3, "unlock");
    
  });
  assert!( Some("unlock") == clone_wait_one_result(&or,None));

}


pub fn sa4(a: Ipv4Addr, p: u16) -> SocketAddr {
 SocketAddr::V4(SocketAddrV4::new(a, p))
}
pub fn sa6(a: Ipv6Addr, p: u16) -> SocketAddr {
 SocketAddr::V6(SocketAddrV6::new(a, p, 0, 0))
}



#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ArcKV<KV : KeyVal> (pub Arc<KV>);

impl<KV : KeyVal> Encodable for ArcKV<KV> {
  fn encode<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
    // default to local without att
    self.0.encode_kv(s, true, false)
  }
}

impl<KV : KeyVal> Decodable for ArcKV<KV> {
  fn decode<D:Decoder> (d : &mut D) -> Result<ArcKV<KV>, D::Error> {
    // default to local without att
    Self::decode_kv(d, true, false)
  }
}
/*
impl<KV : KeyVal> AsKeyValIf for ArcKV<KV> 
  {
  type KV = KV;
  type BP = ();
  fn as_keyval_if(& self) -> & Self::KV {
    &(*self.0)
  }
  fn build_from_keyval(_ : (), kv : Self::KV) -> Self {
    ArcKV::new(kv)
  }
  fn decode_bef<D:Decoder> (d : &mut D, is_local : bool, with_att : bool) -> Result<Self::BP, D::Error> {Ok(())}
}
*/
impl<KV : KeyVal> ArcKV<KV> {
  #[inline]
  pub fn new(kv : KV) -> ArcKV<KV> {
    ArcKV(Arc::new(kv))
  }
}

impl<V : KeyVal> Deref for ArcKV<V> {
  type Target = V;
  fn deref<'a> (&'a self) -> &'a V {
    &self.0
  }
}



impl<KV : KeyVal> KeyVal for ArcKV<KV> {
  type Key = <KV as KeyVal>::Key;
  #[inline]
  fn get_key(&self) -> <KV as KeyVal>::Key {
    self.0.get_key()
  }/*
  #[inline]
  fn get_key_ref<'a>(&'a self) -> &'a <KV as KeyVal>::Key {
    self.0.get_key_ref()
  }*/
  #[inline]
  fn get_attachment(&self) -> Option<&Attachment> {
    self.0.get_attachment() 
  }
  #[inline]
  fn encode_kv<S:Encoder> (&self, s: &mut S, is_local : bool, with_att : bool) -> Result<(), S::Error> {
    self.0.encode_kv(s, is_local, with_att)
  }
  #[inline]
  fn decode_kv<D:Decoder> (d : &mut D, is_local : bool, with_att : bool) -> Result<Self, D::Error> {
    <KV as KeyVal>::decode_kv(d, is_local, with_att).map(|r|ArcKV::new(r))
  }
}

impl<KV : KeyVal> SettableAttachment for ArcKV<KV> {
  #[inline]
  /// set attachment, 
  fn set_attachment(& mut self, fi:&Attachment) -> bool {
    // only solution : make unique and then new Arc : functional style : costy : a copy of every
    // keyval with an attachment not serialized in it.
    // Othewhise need a kvmut used for protomess only
    // currently no use of weak pointer over our Arc, so when used after receiving a message
    // (unique arc) no clone may occurs (see fn doc).
    let kv = Arc::make_mut(&mut self.0);
    kv.set_attachment(fi)
  }

}
impl<V : FileKeyVal> FileKeyVal for ArcKV<V> {
  #[inline]
  fn name(&self) -> String {
    self.0.name()
  }

  #[inline]
  fn from_path(tmpf : PathBuf) -> Option<ArcKV<V>> {
    <V as FileKeyVal>::from_path(tmpf).map(|v|ArcKV::new(v))
  }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct TimeSpecExt(pub Timespec);
impl Deref for TimeSpecExt {
  type Target = Timespec;
  #[inline]
  fn deref<'a> (&'a self) -> &'a Timespec {
    &self.0
  }
}
impl Encodable for TimeSpecExt {
  fn encode<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
    let pair = (self.0.sec,self.0.nsec);
    pair.encode(s)
  }
}

impl Decodable for TimeSpecExt {
  fn decode<D:Decoder> (d : &mut D) -> Result<TimeSpecExt, D::Error> {
    let tisp : Result<(i64,i32), D::Error>= Decodable::decode(d);
    tisp.map(|(sec,nsec)| TimeSpecExt(Timespec{sec:sec,nsec:nsec}))
  }
}
/*pub fn ref_and_then<T, U, F : FnOnce(&T) -> Option<U>>(o : &Option<T>, f : F) -> Option<U> {
  match o {
    &Some(ref x) => f(x),
    &None => None,
  }
}*/


