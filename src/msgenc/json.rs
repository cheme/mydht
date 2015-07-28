use rustc_serialize::json;
use rustc_serialize::json::EncoderError as JSonEncError;
use rustc_serialize::json::DecoderError as JSonDecError;
use rustc_serialize::{Encodable,Decodable};
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
#[derive(Debug,Clone)]
pub struct Json;

/// a technical limit to protomessage length TODO make it dynamic ( strored in Json struct)
const MAX_BUFF : usize = 10000000; // use for attachment send/receive -- 21888 seems to be maxsize
#[derive(RustcDecodable,RustcEncodable)]
struct has_attachment(bool);

unsafe impl Send for Json {
}

impl MsgEnc for Json {
/*  fn encode<P : Peer, V : KeyVal> (&self, mesg : &ProtoMessage<P,V>) -> Option<Vec<u8>>{
    json::encode(mesg).ok().map(|m| m.into_bytes())
  }*/
 
  fn decode<P : Peer, V : KeyVal> (&self, buff : &[u8]) -> Option<ProtoMessage<P,V>>{
    // TODO should not & on deref of cow...
    json::decode(&(*String::from_utf8_lossy(buff))).ok()
  }
  fn encode_into<'a, W : Write, P : Peer + 'a, V : KeyVal + 'a> (&self, w : &mut W, mesg : &ProtoMessageSend<'a,P,V>) -> MDHTResult<()> 
where <P as Peer>::Address : 'a,
      <P as KeyVal>::Key : 'a,
      <V as KeyVal>::Key : 'a {
 
    try!(json::encode(mesg).map(|st|{
      let bytes = st.into_bytes();
      try!(w.write_u64::<LittleEndian>(bytes.len().to_u64().unwrap()));
      w.write_all(&bytes[..])
    }));
    Ok(())
  }

  /// attach into will simply add bytes afterward json cont (no hex or base64 costly enc otherwhise
  /// it would be into message)
  fn attach_into<W : Write> (&self, w : &mut W, a : Option<&Attachment>) -> MDHTResult<()> {
    // TODO static bytes of encoded has some true and same for has some false
    let has_at = has_attachment(a.is_some());
    let bytes = json::encode(&has_at).unwrap().into_bytes();
    try!(w.write_all(&bytes[..]));

    write_attachment(w,a)
  }

  fn decode_from<R : Read, P : Peer, V : KeyVal>(&self, r : &mut R) -> MDHTResult<ProtoMessage<P,V>> {
    let len = try!(r.read_u64::<LittleEndian>()).to_usize().unwrap();
    // TODO max len : easy overflow here
    if len > MAX_BUFF {
      return Err(Error(format!("Oversized protomessage, max length in bytes was {:?}", MAX_BUFF), ErrorKind::DecodingError, None));
    };
    let mut vbuf = vec![0; len];
    try!(r.read(&mut vbuf[..]));
    // TODO this is likely break at the first utf8 char : test it
    Ok(try!(json::decode(&(*String::from_utf8_lossy(&mut vbuf[..])))))
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
      _ => Err(Error("Invalid attachment description".to_string(), ErrorKind::DecodingError, None)),
    }
  }


}

impl From<JSonEncError> for Error {
  #[inline]
  fn from(e : JSonEncError) -> Error {
    Error(e.description().to_string(), ErrorKind::EncodingError, Some(Box::new(e)))
  }
}
impl From<JSonDecError> for Error {
  #[inline]
  fn from(e : JSonDecError) -> Error {
    Error(e.description().to_string(), ErrorKind::DecodingError, Some(Box::new(e)))
  }
}


