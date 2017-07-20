
//! msgenc : encoding of message exchanged between peers.


use keyval::{KeyVal,Attachment};
use peer::{Peer};
use query::{QueryID,QueryMsg};
use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};
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
use byteorder::Error as BOError;
use std::error::Error as StdError;
use self::send_variant::ProtoMessage as ProtoMessageSend;


/// Trait for message encoding between peers.
/// It use bytes which will be used by transport.
pub trait MsgEnc : Send + Sync + 'static {
  //fn encode<P : Peer, V : KeyVal>(&self, &ProtoMessage<P,V>) -> Option<Vec<u8>>;
  
  /// encode
  fn encode_into<'a,W : Write, P : Peer + 'a, V : KeyVal + 'a> (&self, w : &mut W, mesg : &ProtoMessageSend<'a,P,V>) -> MDHTResult<()>
where <P as Peer>::Address : 'a,
      <P as KeyVal>::Key : 'a,
      <V as KeyVal>::Key : 'a ;
 
  fn attach_into<W : Write> (&self, &mut W, Option<&Attachment>) -> MDHTResult<()>;
//  fn decode<P : Peer, V : KeyVal>(&self, &[u8]) -> Option<ProtoMessage<P,V>>;
  /// decode
  fn decode_from<R : Read, P : Peer, V : KeyVal>(&self, &mut R) -> MDHTResult<ProtoMessage<P,V>>;
  fn attach_from<R : Read>(&self, &mut R) -> MDHTResult<Option<Attachment>>;
}


pub mod send_variant {
use keyval::{KeyVal};
use peer::{Peer};
use query::{QueryID,QueryMsg};
use rustc_serialize::{Encoder,Encodable};
use super::{DistantEnc,DistantEncAtt};

#[derive(RustcEncodable,Debug)]
pub enum ProtoMessage<'a,P : Peer + 'a, V : KeyVal + 'a> {
  PING(&'a P,Vec<u8>, Vec<u8>), 
  PONG(&'a P,Vec<u8>),
  STORENODE(Option<QueryID>, Option<DistantEnc<&'a P>>),
  STOREVALUE(Option<QueryID>, Option<DistantEnc<&'a V>>),
  STOREVALUEATT(Option<QueryID>, Option<DistantEncAtt<&'a V>>),
  FINDNODE(QueryMsg<P>, P::Key),
  FINDVALUE(QueryMsg<P>, V::Key),
  PROXY(Option<usize>), // No content as message decode will be done by reading following payload
}

}



#[derive(RustcDecodable,Debug)]
/// Messages between peers
/// TODO ref variant for send !!!!
pub enum ProtoMessage<P : Peer, V : KeyVal> {
  /// Our node pinging plus challenge and message signing
  PING(P,Vec<u8>, Vec<u8>), 
  /// Ping reply with signature with
  ///  - emitter
  ///  - signing of challenge
  ///  P is added for reping on lost PING, TODO could be remove and simply origin key in pong
  PONG(P,Vec<u8>),
  /// reply to query of propagate, if no queryid is used it is a node propagate
  STORENODE(Option<QueryID>, Option<DistantEnc<P>>), // reply to synch or asynch query - note no mix in query mode -- no signature, since we go with a ping before adding a node (ping is signed) TODO allow a signing with primitive only for this ?? and possible not ping afterwad
  /// reply to query of propagate, if no queryid is used it is a node propagate
  STOREVALUE(Option<QueryID>, Option<DistantEnc<V>>), // reply to synch or asynch query - note no mix in query mode
  /// reply to query of propagate
  STOREVALUEATT(Option<QueryID>, Option<DistantEncAtt<V>>), // same as store value but use encoding distant with attachment
  /// Query for Peer
  FINDNODE(QueryMsg<P>, P::Key), // int is remaining nb hop -- TODO plus message signing for private node communication (add a primitive to check mess like those
  /// Query for Value
  FINDVALUE(QueryMsg<P>, V::Key),
}


#[derive(Debug)]
/// Choice of an encoding without attachment
pub struct DistantEnc<V> (pub V);
#[derive(Debug)]
/// Choice of an encoding with attachment
pub struct DistantEncAtt<V> (pub V);

impl<'a, V : KeyVal> Encodable for DistantEnc<&'a V>{
  fn encode<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
    // not local without attach
    self.0.encode_kv(s, false, false)
  }
}

impl<V : KeyVal> Decodable for DistantEnc<V> {
  fn decode<D:Decoder> (d : &mut D) -> Result<DistantEnc<V>, D::Error> {
    // not local without attach
    <V as KeyVal>::decode_kv(d, false, false).map(|v|DistantEnc(v))
  }
}

impl<'a, V : KeyVal> Encodable for DistantEncAtt<&'a V>{
  fn encode<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
    // not local with attach
        self.0.encode_kv(s, false, true)
  }
}

impl<V : KeyVal>  Decodable for DistantEncAtt<V> {
  fn decode<D:Decoder> (d : &mut D) -> Result<DistantEncAtt<V>, D::Error> {
    // not local with attach
    <V as KeyVal>::decode_kv(d, false, true).map(|v|DistantEncAtt(v))
  }
}


/// common utility for encode implementation (attachment should allways be following bytes)
pub fn write_attachment<W : Write> (w : &mut W, a : Option<&Attachment>) -> MDHTResult<()> {
    a.map(|path| {
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
    }).unwrap_or(Ok(()))
}

pub fn read_attachment(s : &mut Read)-> MDHTResult<Attachment> {

  //let nbframe = try!(s.read_u32::<LittleEndian>()).to_usize().unwrap();
  //let bsize   = try!(s.read_u32::<LittleEndian>()).to_usize().unwrap();
  //let lfrsize = try!(s.read_u32::<LittleEndian>()).to_usize().unwrap();
  //let fsize = tryfor!(BOErr,s.read_u64::<LittleEndian>());
  let fsize = try!(s.read_u64::<LittleEndian>());
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


