//! msgenc : encoding of message exchanged between peers.

use readwrite_comp::{
  ExtRead,
  ExtWrite,
};
use keyval::{
  Attachment,
};
use peer::{Peer};
use mydhtresult::Result as MDHTResult;
use std::io::Write;
use std::io::Read;
use mydhtresult::Error;
use mydhtresult::{ErrorKind};
use std::fs::File;
use std::io::{Seek,SeekFrom};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use num::traits::ToPrimitive;
use utils;
use utils::Proto;
use self::send_variant::ProtoMessage as ProtoMessageSend;
use service::SpawnerYield;

/// Trait for message encoding between peers.
/// It use bytes which will be used by transport.
///
/// Message is use as mutable reference to allow complex construction (for instance with a single
/// use tunnel encoder)
/// TODO split in read/write and make mut??
/// TODO why sync
/// TODO trait change and is not encoded only oriented (extwrite and asyncyield added) -> create a
/// new trait ??
pub trait MsgEnc<P : Peer,M> : Send + 'static + Proto {
  //fn encode<P : Peer, V : KeyVal>(&self, &ProtoMessage<P,V>) -> Option<Vec<u8>>;
  
  /// encode
  fn encode_into<'a,W : Write,EW : ExtWrite,S : SpawnerYield> (&mut self, w : &mut W, &mut EW, &mut S, masg : &ProtoMessageSend<'a,P>) -> MDHTResult<()>
where <P as Peer>::Address : 'a;

  fn decode_from<R : Read,ER : ExtRead,S : SpawnerYield>(&mut self, &mut R, &mut ER, &mut S) -> MDHTResult<ProtoMessage<P>>;

  fn encode_msg_into<'a,W : Write,EW : ExtWrite,S : SpawnerYield> (&mut self, w : &mut W, &mut EW, &mut S, mesg : &mut M) -> MDHTResult<()>;

  fn attach_into<W : Write,EW : ExtWrite,S : SpawnerYield> (&mut self, &mut W, &mut EW, &mut S, &Attachment) -> MDHTResult<()>;

  /// decode
  fn decode_msg_from<R : Read,ER : ExtRead,S : SpawnerYield>(&mut self, &mut R, &mut ER, &mut S) -> MDHTResult<M>;

  /// error if attachment more than a treshold (0 if no limit).
  fn attach_from<R : Read,ER : ExtRead,S : SpawnerYield>(&mut self, &mut R, &mut ER, &mut S, usize) -> MDHTResult<Attachment>;
}


pub mod send_variant {
  use peer::{Peer};

  #[derive(Serialize,Debug)]
  pub enum ProtoMessage<'a,P : Peer + 'a> {
    PING(&'a P,Vec<u8>,Vec<u8>), // TODO vec to &[u8]?? ort at least &Vec<u8> : yes TODO with challenge as ref refacto
    /// reply contain peer for update of distant peer info, for instance its listener address for a
    /// tcp transport.
    PONG(&'a P,Vec<u8>,Vec<u8>,Option<Vec<u8>>),
  }

}



#[derive(Deserialize,Debug)]
/// Messages between peers
#[serde(bound(deserialize = ""))]
pub enum ProtoMessage<P : Peer> {
  /// Our node pinging plus challenge and message signing
  PING(P,Vec<u8>,Vec<u8>), 
  /// Ping reply with signature with
  ///  - emitter
  ///  - challenge from ping (P is not always known at this point)
  ///  - signing of challenge
  ///  - Optionnally a challeng for authentifying back (second challenge)
  ///  P is added for reping on lost PING, TODO could be remove and simply origin key in pong
  PONG(P,Vec<u8>,Vec<u8>,Option<Vec<u8>>),
}



/// common utility for encode implementation (attachment should allways be following bytes)
pub fn write_attachment<W : Write> (w : &mut W, path : &Attachment) -> MDHTResult<()> {
  let mut f = try!(File::open(&path));
  debug!("trynig nwriting att");
  // send over buff size
  let fsize = f.metadata().unwrap().len();
  let nbframe = (fsize / (BUFF_SIZE).to_u64().unwrap()).to_usize().unwrap();
  let lfrsize = (fsize - (BUFF_SIZE.to_u64().unwrap() * nbframe.to_u64().unwrap())).to_usize().unwrap();
  debug!("fsize{:?}",fsize);
  debug!("nwbfr{:?}",nbframe);
  debug!("frsiz{:?}",lfrsize);
  debug!("busize{:?}",BUFF_SIZE);
  // TODO less headers??
  //tryfor!(BOErr,w.write_u64::<LittleEndian>(fsize));
  try!(w.write_u64::<LittleEndian>(fsize));
  //try!(w.write_u32::<LittleEndian>(nbframe.to_u32().unwrap()));
  //try!(w.write_u32::<LittleEndian>(BUFF_SIZE.to_u32().unwrap()));
  //try!(w.write_u32::<LittleEndian>(lfrsize.to_u32().unwrap()));
  try!(f.seek(SeekFrom::Start(0)));
  let buf = &mut [0; BUFF_SIZE];
  for i in 0..(nbframe + 1) {
    debug!("fread : {:?}", i);
    let nb = try!(f.read(buf));
    if nb == BUFF_SIZE {
      try!(w.write_all(buf));
    } else {
      if nb != lfrsize {
        return Err(Error("mismatch file size calc for tcp transport".to_string(), ErrorKind::IOError, None));
      };
      // truncate buff
      try!(w.write(&buf[..nb]));
    }
  };
  Ok(())
}

pub fn read_attachment(s : &mut Read, mlen : usize)-> MDHTResult<Attachment> {

  //let nbframe = try!(s.read_u32::<LittleEndian>()).to_usize().unwrap();
  //let bsize   = try!(s.read_u32::<LittleEndian>()).to_usize().unwrap();
  //let lfrsize = try!(s.read_u32::<LittleEndian>()).to_usize().unwrap();
  //let fsize = tryfor!(BOErr,s.read_u64::<LittleEndian>());
  let fsize = try!(s.read_u64::<LittleEndian>());
  if mlen > 0 && fsize > (mlen as u64) {
    return Err(Error("Attachment bigger than expected".to_string(), ErrorKind::SerializingError, None));
  }

  let nbframe = (fsize / (BUFF_SIZE).to_u64().unwrap()).to_usize().unwrap();
  let lfrsize = (fsize - (BUFF_SIZE.to_u64().unwrap() * nbframe.to_u64().unwrap())).to_usize().unwrap();
 
  // TODO change : buffer size in message is useless and dangerous
  debug!("bs{:?}",BUFF_SIZE);
  debug!("nwbfr{:?}",nbframe);
  debug!("frsiz{:?}",lfrsize);
  let (fp, mut f) = try!(utils::create_tmp_file());
  let buf = &mut [0; BUFF_SIZE];
  for _ in 0..(nbframe + 1) {
    let nb = try!(s.read(buf));
    if nb == BUFF_SIZE {
      try!(f.write(buf));
    } else {
      if nb != lfrsize {
        // todo delete file
        return Err(Error("mismatch received file size calc for tcp transport".to_string(), ErrorKind::IOError, None));
      };
      // truncate buff
      try!(f.write(&buf[..nb]));
    }
  };

  try!(f.seek(SeekFrom::Start(0)));
  Ok(fp)
}

const BUFF_SIZE : usize = 10000; // use for attachment send/receive -- 21888 seems to be maxsize for tcp read at least TODO parameterized this one (trait constant?)

//pub struct BOErr(pub BOError);
/*
impl From<BOErr> for Error {
  #[inline]
  fn from(e : BOErr) -> Error {
    Error(e.0.description().to_string(), ErrorKind::ExternalLib, Some(Box::new(e.0)))
  }
}*/


