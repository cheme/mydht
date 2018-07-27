
use readwrite_comp::{
  ExtRead,
  ExtWrite,
  CompExtWInner,
  CompExtRInner,
};

use serde::{
  Serialize,
  Deserialize,
};
use serde::de::{
  DeserializeOwned,
};
use serde_json::{
  Deserializer,
};
use serde_json as json;
use serde_json::error::Error as JSonError;
//use rustc_serialize::{Encodable,Decodable};
use super::MsgEnc;
use keyval::{
  KeyVal,
  Attachment,
};
use peer::Peer;
use super::ProtoMessage;
use super::send_variant::ProtoMessage as ProtoMessageSend;
use std::io::Write;
use std::io::Read;
use mydhtresult::Result as MDHTResult;
use mydhtresult::{
  Error,
  ErrorKind,
};
use std::error::Error as StdError;
use super::write_attachment;
use super::read_attachment;
use byteorder::{
  LittleEndian,
  ReadBytesExt,
  WriteBytesExt,
};
use utils::Proto;
use service::{
  SpawnerYield,
  ReadYield,
  WriteYield,
};

/// standard usage of rust serialize with json over proto messages no intemediatory type all content
/// is used : full encoding
/// Warning current serializer is plugged to string, so all json content is put in memory, use of
/// attachment with json is recommended for big messages.
/// Json message began by le u64 specifying json protomessage length (in bytes).
/// Since json is included in rustc serialize this implementation is not in its own crate.
#[derive(Debug,Clone)]
pub struct Json;

impl Proto for Json {
  #[inline]
  fn get_new(&self) -> Self {
    Json
  }
}
// a technical limit to protomessage length TODO make it dynamic ( strored in Json struct)
//const MAX_BUFF : usize = 10000000; // use for attachment send/receive -- 21888 seems to be maxsize



unsafe impl Send for Json {
}

impl<P : Peer, M : Serialize + DeserializeOwned> MsgEnc<P,M> for Json {

  fn encode_into<'a, W : Write, EW : ExtWrite, S : SpawnerYield> (&mut self, w : &mut W, sh : &mut EW, s : &mut S, mesg : &ProtoMessageSend<'a,P>) -> MDHTResult<()> 
where <P as Peer>::Address : 'a,
      <P as KeyVal>::Key : 'a {
     let mut wy = WriteYield(w,s);
     let mut w = CompExtWInner(&mut wy,sh);

/*    tryfor!(JSonErr,json::to_vec(mesg).map(|bytes|{
      try!(w.write_u64::<LittleEndian>(bytes.len().to_u64().unwrap()));
      w.write_all(&bytes[..])
    })).map_err(|e|e.into())*/
    Ok(tryfor!(JSonErr,json::to_writer(&mut w,mesg)))
  }

  fn encode_msg_into<'a, W : Write, EW : ExtWrite, S : SpawnerYield> (&mut self, w : &mut W, sh : &mut EW, s : &mut S, mesg : &mut M) -> MDHTResult<()> {
    let mut wy = WriteYield(w,s);
    let mut w = CompExtWInner(&mut wy,sh);
 /*   tryfor!(JSonErr,json::to_vec(mesg).map(|bytes|{
      try!(w.write_u64::<LittleEndian>(bytes.len().to_u64().unwrap()));
      w.write_all(&bytes[..])
    })).map_err(|e|e.into())*/

    Ok(tryfor!(JSonErr,json::to_writer(&mut w,mesg)))
  }


  /// attach into will simply add bytes afterward json cont (no hex or base64 costly enc otherwhise
  /// it would be into message)
  fn attach_into<W : Write, EW : ExtWrite, S : SpawnerYield> (&mut self, w : &mut W, sh : &mut EW, s : &mut S, a : &Attachment) -> MDHTResult<()> {
    let mut wy = WriteYield(w,s);
    let mut w = CompExtWInner(&mut wy,sh);

    write_attachment(&mut w,a)
  }

  fn decode_msg_from<R : Read, ER : ExtRead, S : SpawnerYield>(&mut self, r : &mut R, sh : &mut ER, s : &mut S) -> MDHTResult<M> {
    let mut ry = ReadYield(r,s);
    let mut r = CompExtRInner(&mut ry,sh);
/*    let len = try!(r.read_u64::<LittleEndian>()) as usize;
    // TODO max len : easy overflow here
    if len > MAX_BUFF {
      return Err(Error(format!("Oversized protomessage, max length in bytes was {:?}", MAX_BUFF), ErrorKind::SerializingError, None));
    };
    let mut vbuf = vec![0; len];
    try!(r.read_exact(&mut vbuf[..]));
    // TODO this is likely break at the first utf8 char : test it
    Ok(tryfor!(JSonErr,json::from_slice(&mut vbuf[..])))*/

    let mut de = Deserializer::from_reader(&mut r);
    Ok(tryfor!(JSonErr,M::deserialize(&mut de)))
  }


  fn decode_from<R : Read, ER : ExtRead, S : SpawnerYield>(&mut self, r : &mut R, sh : &mut ER, s : &mut S) -> MDHTResult<ProtoMessage<P>> {
    let mut ry = ReadYield(r,s);
    let mut r = CompExtRInner(&mut ry,sh);
/*    let len = try!(r.read_u64::<LittleEndian>()) as usize;
    // TODO max len : easy overflow here
    if len > MAX_BUFF {
      return Err(Error(format!("Oversized protomessage, max length in bytes was {:?}", MAX_BUFF), ErrorKind::SerializingError, None));
    };
    let mut vbuf = vec![0; len];
    try!(r.read_exact(&mut vbuf[..]));
    // TODO this is likely break at the first utf8 char : test it
    Ok(tryfor!(JSonErr,json::from_slice(&mut vbuf[..])))*/
    //Ok(tryfor!(JSonErr,json::from_reader(&mut r)))

    let mut de = Deserializer::from_reader(&mut r);
    Ok(tryfor!(JSonErr,<ProtoMessage<P>>::deserialize(&mut de)))
  }

  fn attach_from<R : Read, ER : ExtRead, S : SpawnerYield>(&mut self, r : &mut R, sh : &mut ER, s : &mut S, mlen : usize) -> MDHTResult<Attachment> {
    let mut ry = ReadYield(r,s);
    let mut r = CompExtRInner(&mut ry,sh);
    read_attachment(&mut r,mlen)
  }


}


pub struct JSonErr(JSonError);
impl From<JSonErr> for Error {
  #[inline]
  fn from(e : JSonErr) -> Error {
    Error::with_chain(e.0, ErrorKind::Serializing("mydht json error".to_string()))
  }
}


