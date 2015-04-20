extern crate crypto;
extern crate num;
extern crate rand;
extern crate time;
extern crate openssl;
#[macro_use]
use self::num::bigint::RandBigInt;
use self::rand::Rng;
use self::rand::thread_rng;
use std::sync::{Arc,Mutex,Condvar};
use transport::{TransportStream};
use kvstore::{Attachment};
use msgenc::{MsgEnc,ProtoMessage};
use kvstore::{KeyVal};
use kvstore::{FileKeyVal};
use peer::{Peer};
use self::openssl::crypto::hash::{Hasher,Type};
use std::io::Write;
use std::io::Read;
use self::crypto::digest::Digest;
use std::io::Seek;
use std::io::SeekFrom;
use std::fs::File;
use std::net::{SocketAddr, SocketAddrV4, SocketAddrV6, Ipv4Addr, Ipv6Addr};
use std::io::Result as IoResult;
use std::str::FromStr;
use std::os;
use std::env;
use std::fs;
use std::iter;
use std::borrow::ToOwned;
use std::ffi::AsOsStr;
use std::ffi::OsStr;
use std::path::{Path,PathBuf};
use self::time::Timespec;
use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};
use rustc_serialize::hex::{ToHex,FromHex};
use std::ops::Deref;


pub static NULL_TIMESPEC : Timespec = Timespec{ sec : 0, nsec : 0};



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
    self.0.encode(s)
  }
}

impl<KV : KeyVal> Decodable for ArcKV<KV> {
  fn decode<D:Decoder> (d : &mut D) -> Result<ArcKV<KV>, D::Error> {
    let okv : Result<KV, D::Error>= Decodable::decode(d);
    okv.map(|kv|ArcKV(Arc::new(kv)))
  }
}


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
  }
#[inline]
    fn encode_dist_with_att<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
      self.0.encode_dist_with_att(s)
    }
#[inline]
    fn decode_dist_with_att<D:Decoder> (d : &mut D) -> Result<ArcKV<KV>, D::Error> {
      <KV as KeyVal>::decode_dist_with_att(d).map(|r|ArcKV::new(r))
    }
#[inline]
    fn encode_dist<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
      self.0.encode_dist(s)
    }
#[inline]
    fn decode_dist<D:Decoder> (d : &mut D) -> Result<ArcKV<KV>, D::Error> {
      <KV as KeyVal>::decode_dist(d).map(|r|ArcKV::new(r))
    }
#[inline]
    fn encode_loc_with_att<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error>{
      self.0.encode_loc_with_att(s)
    }
#[inline]
    fn decode_loc_with_att<D:Decoder> (d : &mut D) -> Result<ArcKV<KV>, D::Error>{
      <KV as KeyVal>::decode_loc_with_att(d).map(|r|ArcKV::new(r))
    }
#[inline]
    fn get_attachment(&self) -> Option<&Attachment>{
      self.0.get_attachment()
    }
#[inline]
    fn set_attachment(& mut self, fi:&Attachment) -> bool {
      // TODO (need reconstruct Arc) redesign with functional style
      // in fact this is only call when receiving a message (so arc never cloned)
      // should be done in decode of protomessage : TODO implement KeyVal for (attachment, KVMut) 
      // 
      // only solution : make unique and then new Arc : functional style : costy : a copy of every
      // keyval with an attachment not serialized in it.
      // Othewhise need a kvmut used for protomess only
      let kv = self.0.make_unique();
      kv.set_attachment(fi)
    }
}
impl<V : FileKeyVal> FileKeyVal for ArcKV<V> {
  #[inline]
  fn name(&self) -> String {
    self.0.name()
  }

  #[inline]
  fn from_file(tmpf : &mut File) -> Option<ArcKV<V>> {
    <V as FileKeyVal>::from_file(tmpf).map(|v|ArcKV::new(v))
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
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Either<A,B> {
  Left(A),
  Right(B),
}

impl<A,B> Either<A,B> {
  pub fn left (self) -> Option<A> {
    match self {
      Either::Left(a) => Some(a),
      Either::Right(_) => None,
    }
  }
  pub fn right (self) -> Option<B> {
    match self {
      Either::Right(b) => Some(b),
      Either::Left(_) => None,
    }
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

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SocketAddrExt(pub SocketAddr);

impl Encodable for SocketAddrExt {
  fn encode<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
    s.emit_str(&self.0.to_string()[..])
  }
}

impl Decodable for SocketAddrExt {
  fn decode<D:Decoder> (d : &mut D) -> Result<SocketAddrExt, D::Error> {
    d.read_str().map(|ad| {
      SocketAddrExt(FromStr::from_str(&ad[..]).unwrap())
    })
  }
}
impl Deref for SocketAddrExt {
  type Target = SocketAddr;
  fn deref<'a> (&'a self) -> &'a SocketAddr {
    &self.0
  }
}

/*pub fn ref_and_then<T, U, F : FnOnce(&T) -> Option<U>>(o : &Option<T>, f : F) -> Option<U> {
  match o {
    &Some(ref x) => f(x),
    &None => None,
  }
}*/

// TODO rewrite with full new io and new path : this is so awfull + true uuid
pub fn create_tmp_file() -> File {
  let tmpdir = env::temp_dir();
  let mytmpdirpath = tmpdir.join(Path::new("./mydht"));
  fs::create_dir_all(&mytmpdirpath);
  let fname = random_uuid(64).to_string();
  let fpath = mytmpdirpath.join(Path::new(&fname[..]));
  debug!("Creating tmp file : {:?}",fpath);
  File::create(&fpath).unwrap()
}

pub fn is_in_tmp_dir(f : &Path) -> bool {
//  Path::new(os::tmpdir().to_string()).is_ancestor_of(f)
  // TODO usage of start_with instead of is_ancestor_of not tested
  f.starts_with(&env::temp_dir())
}

fn random_uuid(hash_size : usize) -> num::BigUint {
   let mut rng = thread_rng();
   rng.gen_biguint(hash_size)
}

pub fn random_bytes(size : usize) -> Vec<u8> {
   let mut rng = thread_rng();
   let mut bytes = vec![0; size];
   rng.fill_bytes(&mut bytes[..]);
   bytes
}



// TODO serializable option type for transient fields in struct : like option but serialize to none
// allways!! aka transiant option
#[derive(Debug,Eq,PartialEq)]
pub struct TransientOption<V> (Option<V>);


// TODO move to Mutex<Option<V>>?? (most of the time it is the sense : boolean, and easier init
// when complex value : default at start to none. TODO init function
/// for receiving one result only from other processes
//pub type OneResult<V : Send> = Arc<(Mutex<V>,Condvar)>;
pub type OneResult<V> = Arc<(Mutex<V>,Condvar)>;

 
macro_rules! static_buff {
  ($bname:ident, $bname_size:ident, $bsize:expr) => (
    static $bname_size : usize = $bsize;
    static $bname : &'static mut [u8; $bsize] = &mut [0u8; $bsize];
  )
}


#[inline]
pub fn ret_one_result<V : Send> (ores : OneResult<V>, v : V) {
  match ores.0.lock() {
    Ok(mut res) => *res = v,
    Err(m) => error!("poisoned mutex for ping result"),
  }
  ores.1.notify_all();
}


#[inline]
// use only for small clonable stuff or arc it
pub fn clone_wait_one_result<V : Clone + Send> (ores : OneResult<V>) -> Option<V> {
 let r = match ores.0.lock() {
    Ok(mut guard) => {
      match ores.1.wait(guard) {
        Ok(mut r) => {
//          Some(*r)
          Some(r.clone())
        }
        Err(_) => {error!("Condvar issue for return res"); None}, // TODO what to do??? panic?
      }
    },
    Err(poisoned) => {error!("poisonned mutex on one res"); None}, // not logic
 };
 r
}


// struct associating transport and msgenc
pub fn send_msg<P : Peer, V : KeyVal, T : TransportStream, E : MsgEnc>(m : &ProtoMessage<P,V>, a : Option<&Attachment>, t : &mut T, e : &E) -> bool {
  let omess = e.encode(m);
  debug!("sent {:?}",omess);
  match omess {
    Some(mess) => {
      t.streamwrite(&mess[..], a).is_ok()
    }
    None => false,
  }
}

pub fn receive_msg<P : Peer, V : KeyVal, T : TransportStream, E : MsgEnc>(t : &mut T, e : &E) -> Option<(ProtoMessage<P,V>, Option<Attachment>)> {
  let rs = t.streamread();
  match rs {
    Ok((m, at)) => {
      debug!("recv {:?}",m);
      let pm : Option<ProtoMessage<P,V>> = e.decode(&m[..]);
      pm.map(|r|(r, at))
    },
    Err(_) => None, // TODO check if an attachment
  }
}
/*
pub fn sendUnconnectMsg<P : Per, V : KeyVal, T : TransportStream, E : MsgEnc>( p : Arc<P>, m : &ProtoMessage<P,V>, t : &mut T, e : &E ) -> bool {
    let mut sc : IoResult<T> = <T as TransportStream>::connectwith((*p).clone(), Duration::seconds(5));
    match sc {
      None => false,
      Some (mut s) => sendMsg(&s, e),
    }
}*/

pub fn hash_buf_crypto(buff : &[u8], digest : &mut Digest) -> Vec<u8> {
  let bsize = digest.block_size();
  let bbytes = ((bsize+7)/8);
  let ressize = digest.output_bits();
  let outbytes = ((ressize+7)/8);
  debug!("{:?}:{:?}", bsize,ressize);
  let mut tmpvec : Vec<u8> = vec![0; bbytes];
  let buf = tmpvec.as_mut_slice();
  let nbiter = buff.len() / bbytes;
  for i in (0 .. nbiter) {
    // slice overflow ok??
    digest.input(&buff[i * bbytes .. (i+1) * bbytes]);
  };
//  digest.input(&buf[(nbiter -1)*bbytes .. ]);
  let mut rvec : Vec<u8> = vec![0; outbytes];
  let rbuf = rvec.as_mut_slice();
  digest.result(rbuf);
  rbuf.to_vec()
}


pub fn hash_file_crypto(f : &mut File, digest : &mut Digest) -> Vec<u8> {
  let bsize = digest.block_size();
  let bbytes = ((bsize+7)/8);
  let ressize = digest.output_bits();
  let outbytes = ((ressize+7)/8);
  debug!("{:?}:{:?}", bsize,ressize);
  let mut tmpvec : Vec<u8> = vec![0; bbytes];
  let buf = tmpvec.as_mut_slice();
  f.seek(SeekFrom::Start(0));
  loop{
  match f.read(buf) {
    Ok(nb) => {
      if (nb == bbytes) {
      digest.input(buf);
      } else {
        error!("nb{:?}",nb);
        // truncate buff
        digest.input(&buf[..nb]);
        break;
      }
    },
    Err(e) => {
      panic!("error happened when reading file for hashing : {:?}", e);
      break;
    },
  };
  }
  // reset file reader to start of file
  f.seek(SeekFrom::Start(0));
  let mut rvec : Vec<u8> = vec![0; outbytes];
  let rbuf = rvec.as_mut_slice();
  digest.result(rbuf);
  //rbuf.to_vec()
  rbuf.to_vec()
}

pub fn hash_openssl(f : &mut File) -> Vec<u8> {
  let mut digest = Hasher::new(Type::SHA256); // TODO in filestore parameter with a supported hash enum
  let bsize = 64;
//  let bbytes = ((bsize+7)/8);
  let bbytes = 8;
  let ressize = 256;
//  let outbytes = ((ressize+7)/8);
  let outbytes = 32;
  let mut tmpvec : Vec<u8> = vec![0; bbytes];
  let buf = tmpvec.as_mut_slice();
  f.seek(SeekFrom::Start(0));
  loop {
  match f.read(buf) {
    Ok(nb) => {
      if nb == bbytes {
        digest.write_all(buf);
      } else {
        debug!("nb{:?}",nb);
        // truncate buff
        digest.write_all(&buf[..nb]);
        break;
      }
    },
    Err(e) => {
      panic!("error happened when reading file for hashing : {:?}", e);
      //break;
    },
  };
  }
  // reset file writer to start of file
  f.seek(SeekFrom::Start(0));
  digest.finish()
}

