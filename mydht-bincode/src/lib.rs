#[macro_use] extern crate log;
#[macro_use] extern crate mydht_base;
extern crate bincode;


//use rustc_serialize::{Encodable,Decodable};
use mydht_base::msgenc::MsgEnc;
use mydht_base::keyval::{KeyVal,Attachment};
use mydht_base::peer::{Peer};
use mydht_base::msgenc::ProtoMessage;
//use std::collections::BTreeMap;
use std::io::Write;
use std::io::Read;
use mydht_base::msgenc::write_attachment;
use mydht_base::msgenc::read_attachment;
use mydht_base::mydhtresult::Result as MDHTResult;
use mydht_base::mydhtresult::{Error,ErrorKind};
use mydht_base::msgenc::send_variant::ProtoMessage as ProtoMessageSend;
use bincode::rustc_serialize as bincodeser;
use bincode::SizeLimit;
use std::error::Error as StdError;


//use std::marker::Reflect;
use bincode::rustc_serialize::EncodingError as BincError;
use bincode::rustc_serialize::DecodingError as BindError;

pub struct BincErr(BincError);
impl From<BincErr> for Error {
  #[inline]
  fn from(e : BincErr) -> Error {
    Error(e.0.description().to_string(), ErrorKind::EncodingError, Some(Box::new(e.0)))
  }
}
pub struct BindErr(BindError);
impl From<BindErr> for Error {
  #[inline]
  fn from(e : BindErr) -> Error {
    Error(e.0.description().to_string(), ErrorKind::DecodingError, Some(Box::new(e.0)))
  }
}



// full bencode impl
#[derive(Debug,Clone)]
pub struct Bincode;

impl MsgEnc for Bincode {

/*  fn encode<P : Peer, V : KeyVal> (&self, mesg : &ProtoMessage<P,V>) -> Option<Vec<u8>>{
    debug!("encode msg {:?}", mesg);
    let r = bincode::encode(mesg, bincode::SizeLimit::Infinite);
    r.ok()
  }*/

/*  fn decode<P : Peer, V : KeyVal> (&self, buff : &[u8]) -> Option<ProtoMessage<P,V>>{
    debug!("decode msg {:?}", buff);
    bincode::decode(buff).ok()
  }*/

  fn encode_into<'a,W : Write, P : Peer + 'a, V : KeyVal + 'a> (&self, w : &mut W, mesg : &ProtoMessageSend<'a,P,V>) -> MDHTResult<()>
where <P as Peer>::Address : 'a,
      <P as KeyVal>::Key : 'a,
      <V as KeyVal>::Key : 'a {
 
     tryfor!(BincErr,bincodeser::encode_into(mesg, w, SizeLimit::Infinite));
     Ok(())
  }

  fn attach_into<W : Write> (&self, w : &mut W, a : Option<&Attachment>) -> MDHTResult<()> {
    try!(w.write_all(&[if a.is_some(){1}else{0}]));
    write_attachment(w,a)
  }

  fn decode_from<R : Read, P : Peer, V : KeyVal>(&self, r : &mut R) -> MDHTResult<ProtoMessage<P,V>> {
    Ok(tryfor!(BindErr,bincodeser::decode_from(r, SizeLimit::Infinite)))
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


