use serde_json as json;
use serde_json::error::Error as JSonError;
//use rustc_serialize::{Encodable,Decodable};
use super::MsgEnc;
use keyval::{KeyVal,Attachment};
use peer::{Peer};
use super::ProtoMessage;
use super::send_variant::ProtoMessage as ProtoMessageSend;
use std::io::Write;
use std::io::Read;
use mydhtresult::Result as MDHTResult;
use mydhtresult::{Error,ErrorKind};
use std::error::Error as StdError;
use super::write_attachment;
use super::read_attachment;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
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

//#[derive(RustcDecodable,RustcEncodable)]
//struct has_attachment(bool);

unsafe impl Send for Json {
}

impl MsgEnc for Json {
/*  fn encode<P : Peer, V : KeyVal> (&self, mesg : &ProtoMessage<P,V>) -> Option<Vec<u8>>{
    json::encode(mesg).ok().map(|m| m.into_bytes())
  }*/
 
/*  fn decode<P : Peer, V : KeyVal> (&self, buff : &[u8]) -> Option<ProtoMessage<P,V>>{
    //  should not & on deref of cow...
    json::decode(&(*String::from_utf8_lossy(buff))).ok()
  }*/
  fn encode_into<'a, W : Write, P : Peer + 'a, V : KeyVal + 'a> (&self, w : &mut W, mesg : &ProtoMessageSend<'a,P,V>) -> MDHTResult<()> 
where <P as Peer>::Address : 'a,
      <P as KeyVal>::Key : 'a,
      <V as KeyVal>::Key : 'a {
 
    tryfor!(JSonErr,json::to_vec(mesg).map(|bytes|{
      try!(w.write_u64::<LittleEndian>(bytes.len().to_u64().unwrap()));
      w.write_all(&bytes[..])
    })).map_err(|e|e.into())
  }

  /// attach into will simply add bytes afterward json cont (no hex or base64 costly enc otherwhise
  /// it would be into message)
  fn attach_into<W : Write> (&self, w : &mut W, a : Option<&Attachment>) -> MDHTResult<()> {
    let has_at = if a.is_some() {
      BUFF_TRUE
    } else {
      BUFF_FALSE
    };
    try!(w.write_all(&has_at));

    write_attachment(w,a)
  }

  fn decode_from<R : Read, P : Peer, V : KeyVal>(&self, r : &mut R) -> MDHTResult<ProtoMessage<P,V>> {
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

  fn attach_from<R : Read>(&self, r : &mut R) -> MDHTResult<Option<Attachment>> {
    let mut buf = [0];
    try!(r.read(&mut buf));
    match buf[0] {
      i if i == 0 => Ok(None),
      i if i == 1 => {
          debug!("Reding an attached file");
          read_attachment(r).map(|a|Some(a))
      },
      _ => Err(Error("Invalid attachment description".to_string(), ErrorKind::SerializingError, None)),
    }
  }


}


pub struct JSonErr(JSonError);
impl From<JSonErr> for Error {
  #[inline]
  fn from(e : JSonErr) -> Error {
    Error(e.0.description().to_string(), ErrorKind::SerializingError, Some(Box::new(e.0)))
  }
}


