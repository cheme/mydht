
use rustc_serialize::{Encodable,Decodable};
use super::MsgEnc;
use keyval::{KeyVal,Attachment};
use peer::{Peer};
use super::ProtoMessage;
use std::collections::BTreeMap;
use std::io::Write;
use std::io::Read;
use super::write_attachment;
use super::read_attachment;
use mydhtresult::Result as MDHTResult;
use mydhtresult::{Error,ErrorKind};
use msgenc::send_variant::ProtoMessage as ProtoMessageSend;
use bincode;

// full bencode impl
#[derive(Debug,Clone)]
pub struct Bincode;

impl MsgEnc for Bincode {

/*  fn encode<P : Peer, V : KeyVal> (&self, mesg : &ProtoMessage<P,V>) -> Option<Vec<u8>>{
    debug!("encode msg {:?}", mesg);
    let r = bincode::encode(mesg, bincode::SizeLimit::Infinite);
    r.ok()
  }*/

  fn decode<P : Peer, V : KeyVal> (&self, buff : &[u8]) -> Option<ProtoMessage<P,V>>{
    debug!("decode msg {:?}", buff);
    bincode::decode(buff).ok()
  }

  fn encode_into<'a,W : Write, P : Peer + 'a, V : KeyVal + 'a> (&self, w : &mut W, mesg : &ProtoMessageSend<'a,P,V>) -> MDHTResult<()>
where <P as Peer>::Address : 'a,
      <P as KeyVal>::Key : 'a,
      <V as KeyVal>::Key : 'a {
 
     try!(bincode::encode_into(mesg, w, bincode::SizeLimit::Infinite));
     Ok(())
  }

  fn attach_into<W : Write> (&self, w : &mut W, a : Option<&Attachment>) -> MDHTResult<()> {
    try!(w.write_all(&[if a.is_some(){1}else{0}]));
    write_attachment(w,a)
  }

  fn decode_from<R : Read, P : Peer, V : KeyVal>(&self, r : &mut R) -> MDHTResult<ProtoMessage<P,V>> {
    Ok(try!(bincode::decode_from(r, bincode::SizeLimit::Infinite)))
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
      _ => return Err(Error("Invalid attachment description".to_string(), ErrorKind::DecodingError, None)),
    }
  }

}


