
use serde::{
  Serializer,
  Serialize,
  Deserialize,
  Deserializer,
};
use serde::de::DeserializeOwned;
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
use num::traits::ToPrimitive;


/// standard usage of rust serialize with json over proto messages no intemediatory type all content
/// is used : full encoding
/// Warning current serializer is plugged to string, so all json content is put in memory, use of
/// attachment with json is recommended for big messages.
/// Json message began by le u64 specifying json protomessage length (in bytes).
/// Since json is included in rustc serialize this implementation is not in its own crate.
#[derive(Debug,Clone)]
pub struct Json;

/// a technical limit to protomessage length TODO make it dynamic ( strored in Json struct)
const MAX_BUFF : usize = 10000000; // use for attachment send/receive -- 21888 seems to be maxsize

const BUFF_TRUE : [u8; 1] = [1];
const BUFF_FALSE : [u8; 1] = [0];


unsafe impl Send for Json {
}

impl<P : Peer, M : Serialize + DeserializeOwned> MsgEnc<P,M> for Json {

  fn encode_into<'a, W : Write> (&self, w : &mut W, mesg : &ProtoMessageSend<'a,P>) -> MDHTResult<()> 
where <P as Peer>::Address : 'a,
      <P as KeyVal>::Key : 'a {
 
    tryfor!(JSonErr,json::to_vec(mesg).map(|bytes|{
      try!(w.write_u64::<LittleEndian>(bytes.len().to_u64().unwrap()));
      w.write_all(&bytes[..])
    })).map_err(|e|e.into())
  }
  fn encode_msg_into<'a, W : Write> (&self, w : &mut W, mesg : &M) -> MDHTResult<()> {
 
    tryfor!(JSonErr,json::to_vec(mesg).map(|bytes|{
      try!(w.write_u64::<LittleEndian>(bytes.len().to_u64().unwrap()));
      w.write_all(&bytes[..])
    })).map_err(|e|e.into())
  }


  /// attach into will simply add bytes afterward json cont (no hex or base64 costly enc otherwhise
  /// it would be into message)
  fn attach_into<W : Write> (&self, w : &mut W, a : &Attachment) -> MDHTResult<()> {
    write_attachment(w,a)
  }

  fn decode_msg_from<R : Read>(&self, r : &mut R) -> MDHTResult<M> {
    let len = try!(r.read_u64::<LittleEndian>()) as usize;
    // TODO max len : easy overflow here
    if len > MAX_BUFF {
      return Err(Error(format!("Oversized protomessage, max length in bytes was {:?}", MAX_BUFF), ErrorKind::SerializingError, None));
    };
    let mut vbuf = vec![0; len];
    try!(r.read(&mut vbuf[..]));
    // TODO this is likely break at the first utf8 char : test it
    Ok(tryfor!(JSonErr,json::from_slice(&mut vbuf[..])))
  }


  fn decode_from<R : Read>(&self, r : &mut R) -> MDHTResult<ProtoMessage<P>> {
    let len = try!(r.read_u64::<LittleEndian>()) as usize;
    // TODO max len : easy overflow here
    if len > MAX_BUFF {
      return Err(Error(format!("Oversized protomessage, max length in bytes was {:?}", MAX_BUFF), ErrorKind::SerializingError, None));
    };
    let mut vbuf = vec![0; len];
    try!(r.read(&mut vbuf[..]));
    // TODO this is likely break at the first utf8 char : test it
    Ok(tryfor!(JSonErr,json::from_slice(&mut vbuf[..])))
  }

  fn attach_from<R : Read>(&self, r : &mut R, mlen : usize) -> MDHTResult<Attachment> {
    read_attachment(r,mlen)
  }


}


pub struct JSonErr(JSonError);
impl From<JSonErr> for Error {
  #[inline]
  fn from(e : JSonErr) -> Error {
    Error(e.0.description().to_string(), ErrorKind::SerializingError, Some(Box::new(e.0)))
  }
}


