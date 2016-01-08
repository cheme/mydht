
#[cfg(feature="rust-crypto-impl")]
extern crate crypto;
extern crate time;
#[cfg(feature="openssl-impl")]
extern crate openssl;
extern crate bincode;

// reexport from base
pub use mydht_base::utils::*;


use num::bigint::{BigUint,RandBigInt};
use rand::Rng;
use rand::thread_rng;
use std::sync::{Arc,Mutex,Condvar};
use transport::{ReadTransportStream,WriteTransportStream};
use keyval::{Attachment,SettableAttachment};
use msgenc::{MsgEnc,ProtoMessage};
use msgenc::send_variant::ProtoMessage as ProtoMessageSend;
use keyval::{KeyVal};
use std::fmt::{Formatter,Debug};
use std::fmt::Error as FmtError;
//use keyval::{AsKeyValIf};
use keyval::{FileKeyVal};
use peer::Peer;
use peer::Shadow;
#[cfg(feature="openssl-impl")]
use self::openssl::crypto::hash::{Hasher,Type};
use std::io::Write;
use std::io::Read;
#[cfg(feature="rust-crypto-impl")]
use self::crypto::digest::Digest;
#[cfg(not(feature="openssl-impl"))]
#[cfg(feature="rust-crypto-impl")]
use self::crypto::sha2::Sha256;
use std::io::Seek;
use std::io::SeekFrom;
use std::fs::File;
use std::net::{SocketAddr, SocketAddrV4, SocketAddrV6, Ipv4Addr, Ipv6Addr};
use std::io::Result as IoResult;
use std::str::FromStr;
use std::env;
use std::fs;
//use std::iter;
//use std::borrow::ToOwned;
//use std::ffi::OsStr;
use std::path::{Path,PathBuf};
use self::time::Timespec;
use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};
//use rustc_serialize::hex::{ToHex,FromHex};
use std::ops::Deref;
use mydhtresult::Result as MDHTResult;

#[cfg(test)]
use std::thread;



#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Either<A,B> {
  Left(A),
  Right(B),
}

impl<A,B> Either<A,B> {
  pub fn to_options (self) -> (Option<A>, Option<B>) {
    match self {
      Either::Left(a) => (Some(a), None),
      Either::Right(b) => (None, Some(b)),
    }
  }
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
  pub fn left_ref (&self) -> Option<&A> {
    match self {
      &Either::Left(ref a) => Some(a),
      &Either::Right(_) => None,
    }
  }
  pub fn right_ref (&self) -> Option<&B> {
    match self {
      &Either::Right(ref b) => Some(b),
      &Either::Left(_) => None,
    }
  }

}

/*pub fn ref_and_then<T, U, F : FnOnce(&T) -> Option<U>>(o : &Option<T>, f : F) -> Option<U> {
  match o {
    &Some(ref x) => f(x),
    &None => None,
  }
}*/


pub fn random_bytes(size : usize) -> Vec<u8> {
   let mut rng = thread_rng();
   let mut bytes = vec![0; size];
   rng.fill_bytes(&mut bytes[..]);
   bytes
}



pub fn send_msg<'a,P : Peer + 'a, V : KeyVal + 'a, T : WriteTransportStream, E : MsgEnc, S : Shadow> (
   m : &ProtoMessageSend<'a,P,V>, 
   a : Option<&Attachment>, 
   t : &mut T, 
   e : &E,
   s : &mut S,
   smode : S::ShadowMode,
  ) -> MDHTResult<()> 
where <P as Peer>::Address : 'a,
      <P as KeyVal>::Key : 'a,
      <V as KeyVal>::Key : 'a {
  try!(s.shadow_header(t, &smode));
  let mut sws = StreamShadow(t,s,smode);
  try!(e.encode_into(&mut sws,m));
  try!(e.attach_into(&mut sws,a)); // TODO shadow that to!!!
  try!(sws.flush());
  Ok(())
}
struct StreamShadow<'a, 'b, T : 'a + WriteTransportStream, S : 'b + Shadow>
(&'a mut T, &'b mut S, <S as Shadow>::ShadowMode);

struct ReadStreamShadow<'a, 'b, T : 'a + ReadTransportStream, S : 'b + Shadow>
(&'a mut T, &'b mut S, <S as Shadow>::ShadowMode);


impl<'a, 'b, T : 'a + WriteTransportStream, S : 'b + Shadow> Write for StreamShadow<'a,'b,T,S> {
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
      self.1.shadow_iter (buf, self.0, &self.2)
    }
    fn flush(&mut self) -> IoResult<()> {
      try!(self.1.shadow_flush(self.0, &self.2));
      self.0.flush()
    }
}

impl<'a, 'b, T : 'a + ReadTransportStream, S : 'b + Shadow> Read for ReadStreamShadow<'a,'b,T,S> {

  fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
    self.1.read_shadow_iter(self.0, buf, &self.2)
  }
}


// TODO return messg in result
#[inline]
pub fn send_msg3<'a,P : Peer + 'a, V : KeyVal + 'a, T : WriteTransportStream, E : MsgEnc>(m : &ProtoMessageSend<'a,P,V>, a : Option<&Attachment>, t : &mut T, e : &E) -> bool 
where <P as Peer>::Address : 'a,
      <P as KeyVal>::Key : 'a,
      <V as KeyVal>::Key : 'a {
 
  e.encode_into(t,m).is_ok()
    && e.attach_into(t,a).is_ok()
    && t.flush().is_ok()
}

pub fn receive_msg_tmp2<P : Peer, V : KeyVal, T : ReadTransportStream + Read, E : MsgEnc, S : Shadow>(t : &mut T, e : &E, s : &mut S) -> MDHTResult<(ProtoMessage<P,V>, Option<Attachment>)> {
  let sm = try!(s.read_shadow_header(t));
  let (m, oa) = { 
    let mut srs = ReadStreamShadow(t,s,sm);
    let m = try!(e.decode_from(&mut srs));
    let oa = try!(e.attach_from(&mut srs));
    (m, oa)
  };
  t.end_read_msg();
  Ok((m,oa))
}


// TODO switch receive to this iface
pub fn receive_msg_tmp<P : Peer, V : KeyVal, T : ReadTransportStream + Read, E : MsgEnc>(t : &mut T, e : &E) -> MDHTResult<(ProtoMessage<P,V>, Option<Attachment>)> {
    let m = try!(e.decode_from(t));
    let oa = try!(e.attach_from(t));
    t.end_read_msg();
    Ok((m,oa))
}

#[inline]
pub fn receive_msg<P : Peer, V : KeyVal, T : ReadTransportStream + Read, E : MsgEnc, S : Shadow>(t : &mut T, e : &E, s : &mut S) -> Option<(ProtoMessage<P,V>, Option<Attachment>)> {
  receive_msg_tmp2(t,e,s).ok()
}

#[inline]
pub fn receive_msg_old<P : Peer, V : KeyVal, T : ReadTransportStream + Read, E : MsgEnc>(t : &mut T, e : &E) -> Option<(ProtoMessage<P,V>, Option<Attachment>)> {
  receive_msg_tmp(t,e).ok()
}
/*
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
}*/
/*
pub fn sendUnconnectMsg<P : Per, V : KeyVal, T : TransportStream, E : MsgEnc>( p : Arc<P>, m : &ProtoMessage<P,V>, t : &mut T, e : &E ) -> bool {
    let mut sc : IoResult<T> = <T as TransportStream>::connectwith((*p).clone(), Duration::seconds(5));
    match sc {
      None => false,
      Some (mut s) => sendMsg(&s, e),
    }
}*/

#[cfg(feature="rust-crypto-impl")]
pub fn hash_buf_crypto(buff : &[u8], digest : &mut Digest) -> Vec<u8> {
  let bsize = digest.block_size();
  let bbytes = (bsize+7)/8;
  let ressize = digest.output_bits();
  let outbytes = (ressize+7)/8;
  debug!("{:?}:{:?}", bsize,ressize);

  let nbiter = if buff.len() == 0 {
      0
  }else {
    (buff.len() - 1) / bbytes
  };
  for i in 0 .. nbiter + 1 {
    let end = (i+1) * bbytes;
    if end < buff.len() {
      digest.input(&buff[i * bbytes .. end]);
    } else {
      digest.input(&buff[i * bbytes ..]);
    };
  };


  let mut rvec : Vec<u8> = vec![0; outbytes];
  let rbuf = rvec.as_mut_slice();
  digest.result(rbuf);
  rbuf.to_vec()
}


#[cfg(not(feature="openssl-impl"))]
#[cfg(feature="rust-crypto-impl")]
pub fn hash_file_crypto(f : &mut File, digest : &mut Digest) -> Vec<u8> {
  let bsize = digest.block_size();
  let bbytes = (bsize+7)/8;
  let ressize = digest.output_bits();
  let outbytes = (ressize+7)/8;
  debug!("{:?}:{:?}", bsize,ressize);
  let mut tmpvec : Vec<u8> = vec![0; bbytes];
  let buf = tmpvec.as_mut_slice();
  match f.seek(SeekFrom::Start(0)) {
    Ok(_) => (),
    Err(e) => {
      error!("failure to create hash for file : {:?}",e);
      return Vec::new(); // TODO correct error mgmt
    },
  };
  loop {
    match f.read(buf) {
      Ok(nb) => {
        if nb == bbytes {
          digest.input(buf);
        } else {
          error!("nb{:?}",nb);
          // truncate buff
          digest.input(&buf[..nb]);
          break;
        }
      },
      Err(e) => {
        error!("error happened when reading file for hashing : {:?}", e);
        return Vec::new();
    },
  };
  }
  // reset file reader to start of file
  match f.seek(SeekFrom::Start(0)) {
    Ok(_) => (),
    Err(e) => {
      error!("failure to create hash for file : {:?}",e);
      return Vec::new(); // TODO correct error mgmt
    },
  }
 
  let mut rvec : Vec<u8> = vec![0; outbytes];
  let rbuf = rvec.as_mut_slice();
  digest.result(rbuf);
  //rbuf.to_vec()
  rbuf.to_vec()
}

#[cfg(feature="openssl-impl")]
pub fn hash_openssl(f : &mut File) -> Vec<u8> {
  let mut digest = Hasher::new(Type::SHA256); // TODO in filestore parameter with a supported hash enum
//  let bsize = 64;
//  let bbytes = ((bsize+7)/8);
  let bbytes = 8;
//  let ressize = 256;
//  let outbytes = ((ressize+7)/8);
//  let outbytes = 32;
  let mut tmpvec : Vec<u8> = vec![0; bbytes];
  let buf = tmpvec.as_mut_slice();
  match f.seek(SeekFrom::Start(0)) {
    Ok(_) => (),
    Err(e) => {
      error!("failure to create hash for file : {:?}",e);
      return Vec::new(); // TODO correct error mgmt
    },
  };
  loop {
  match f.read(buf) {
    Ok(nb) => {
      if nb == bbytes {
        match digest.write_all(buf) {
          Ok(_) => (),
          Err(e) => {
            error!("failure to create hash for file : {:?}",e);
            return Vec::new(); // TODO correct error mgmt
          },
        };
      } else {
        debug!("nb{:?}",nb);
        // truncate buff
        match digest.write_all(&buf[..nb]) {
          Ok(_) => (),
          Err(e) => {
            error!("failure to create hash for file : {:?}",e);
            return Vec::new(); // TODO correct error mgmt
          },
        };
 
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
  match f.seek(SeekFrom::Start(0)) {
    Ok(_) => (),
    Err(e) => {
      error!("failure to create hash for file : {:?}",e);
      return Vec::new(); // TODO correct error mgmt
    },
  };
 
  digest.finish()
}

