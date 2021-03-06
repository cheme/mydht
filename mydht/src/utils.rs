
#[cfg(feature="rust-crypto-impl")]
extern crate crypto;
#[cfg(feature="openssl-impl")]
extern crate openssl;
extern crate bincode;


// reexport from base
pub use mydht_base::utils::*;
// TODO move
pub use procs::{
  OptFrom,
  OptInto,
};
/*use self::readwrite_comp::{
  ExtRead,
  ExtWrite,
  CompExtWInner,
  CompExtRInner,
};*/
use rand::Rng;
use rand::thread_rng;
use keyval::{Attachment};
use msgenc::{MsgEnc,ProtoMessage};
use msgenc::send_variant::ProtoMessage as ProtoMessageSend;
//use keyval::{AsKeyValIf};
use peer::Peer;
#[cfg(feature="openssl-impl")]
use self::openssl::hash::{Hasher,MessageDigest};
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
use service::SpawnerYield;
//use std::iter;
//use std::borrow::ToOwned;
//use std::ffi::OsStr;
//use rustc_serialize::hex::{ToHex,FromHex};
use mydhtresult::Result as MDHTResult;




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

/*
pub fn receive_msg<P : Peer, M, T : Read, S : SpawnerYield, E : MsgEnc<P,M>, S : ExtRead>(t : &mut T, e : &mut E, s : &mut S) -> MDHTResult<ProtoMessage<P>> {
  let mut cr = CompExtRInner(t,s);
  let m = e.decode_from(&mut cr)?;
  Ok(m)
}
pub fn receive_msg_msg<P : Peer, M, T : Read, S : SpawnerYield, E : MsgEnc<P,M>, S : ExtRead>(t : &mut T, e : &mut E, s : &mut S) -> MDHTResult<M> {
  let mut cr = CompExtRInner(t,s);
  let m = e.decode_msg_from(&mut cr)?;
  Ok(m)
}

pub fn receive_att<P : Peer, M, T : Read, S : SpawnerYield, E : MsgEnc<P,M>, S : ExtRead>(t : &mut T, e : &mut E, s : &mut S, sl : usize) -> MDHTResult<Attachment> {
  let mut cr = CompExtRInner(t,s);
  let oa = e.attach_from(&mut cr,sl)?;
  Ok(oa)
}
*/
/*#[inline]
  fn decode_from<R : Read, ER : ExtRead, S : SpawnerYield>(&mut self, r : &mut R, sh : &mut ER, s : &mut S) -> MDHTResult<ProtoMessage<P>> {
pub fn shad_read_header<R : Read,ER : ExtRead,S : SpawnerYield>(r : &mut R, sh : &mut ER, s : &mut S) -> MDHTResult<()> {
  let mut ry = ReadYield(r,s);
  Ok(())
}
#[inline]
pub fn shad_read_end<T : Read, S : ExtRead>(s : &mut S, t : &mut T) -> MDHTResult<()> {
  s.read_end(t)?;
  Ok(())
}
#[inline]
pub fn shad_write_header<T : Write, S : ExtWrite>(s : &mut S, t : &mut T) -> MDHTResult<()> {
  s.write_header(t)?;
  Ok(())
}
#[inline]
pub fn shad_flush<T : Write, S : ExtWrite>(s : &mut S, t : &mut T) -> MDHTResult<()> {
  s.flush_into(t)?;
  Ok(())
}
#[inline]
pub fn shad_write_end<T : Write, S : ExtWrite>(s : &mut S, t : &mut T) -> MDHTResult<()> {
  s.write_end(t)?;
  Ok(())
}*/
/*
pub fn send_msg<P : Peer, M, T : Write, E : MsgEnc<P,M>, S : ExtWrite>(m : &ProtoMessageSend<P>, t : &mut T, e : &mut E, s : &mut S) -> MDHTResult<()> {
  let mut cw = CompExtWInner(t,s);
  e.encode_into(&mut cw,m)?;
  Ok(())
}
pub fn send_msg_msg<P : Peer, M, T : Write, E : MsgEnc<P,M>, S : ExtWrite>(m : &mut M, t : &mut T, e : &mut E, s : &mut S) -> MDHTResult<()> {

  let mut cw = CompExtWInner(t,s);
  e.encode_msg_into(&mut cw,m)?;
  Ok(())
}

pub fn send_att<P : Peer, M, T : Write, E : MsgEnc<P,M>, S : ExtWrite>(att : &Attachment, t : &mut T, e : &mut E, s : &mut S) -> MDHTResult<()> {
  let mut cw = CompExtWInner(t,s);
  e.attach_into(&mut cw,att)
}
*/



// TODO unused ? remove
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

// TODO unused ? remove
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

// TODO return result!! TODO unused??? : remove
#[cfg(feature="openssl-impl")]
pub fn hash_openssl(f : &mut File) -> Vec<u8> {
  let mut digest = Hasher::new(MessageDigest::sha256()).unwrap(); // TODO in filestore parameter with a supported hash enum // TODO return error
  let bbytes = 256/8;
  let mut vbuff = vec!(0;bbytes);
//  let bsize = 64;
//  let bbytes = ((bsize+7)/8);
//  let ressize = 256;
//  let outbytes = ((ressize+7)/8);
//  let outbytes = 32;
  let buf = vbuff.as_mut_slice();
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
        match digest.update(buf) {
          Ok(_) => (),
          Err(e) => {
            error!("failure to create hash for file : {:?}",e);
            return Vec::new(); // TODO correct error mgmt
          },
        };
      } else {
        debug!("nb{:?}",nb);
        // truncate buff
        match digest.update(&buf[..nb]) {
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
 
  digest.finish2().unwrap().to_vec()
}

