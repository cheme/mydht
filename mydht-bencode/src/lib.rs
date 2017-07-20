#[macro_use] extern crate log;
#[macro_use] extern crate mydht_base;
extern crate bencode;
extern crate byteorder;
extern crate rustc_serialize;

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use rustc_serialize::{Encodable,Decodable};
use mydht_base::msgenc::MsgEnc;
use mydht_base::keyval::{KeyVal,Attachment};
use mydht_base::peer::{Peer};
use mydht_base::msgenc::ProtoMessage;
use mydht_base::msgenc::{read_attachment,write_attachment};
use mydht_base::msgenc::BOErr;
use mydht_base::msgenc::send_variant::ProtoMessage as ProtoMessageSend;
use std::collections::VecDeque;
use std::io::Write;
use std::io::Read;
use mydht_base::mydhtresult::{Error,ErrorKind};
use mydht_base::mydhtresult::Result as MDHTResult;
use bencode::streaming::Error as BEError;
// full bencode impl
#[derive(Debug,Clone)]
pub struct Bencode;

impl MsgEnc for Bencode {
  fn encode_into<'a, W : Write, P : Peer + 'a, V : KeyVal + 'a> (&self, w : &mut W, mesg : &ProtoMessageSend<'a,P,V>) -> MDHTResult<()> 
where <P as Peer>::Address : 'a,
      <P as KeyVal>::Key : 'a,
      <V as KeyVal>::Key : 'a {
 
    debug!("encode msg {:?}", mesg);
    let r = try!(bencode::encode(mesg));
    tryfor!(BOErr,w.write_u64::<LittleEndian>(r.len() as u64));
    try!(w.write_all(&r));
    Ok(())

  }

  fn attach_into<W : Write> (&self, w : &mut W, a : Option<&Attachment>) -> MDHTResult<()> {
    try!(w.write_all(&[if a.is_some(){1}else{0}]));
    write_attachment(w,a)
  }

  fn decode_from<R : Read, P : Peer, V : KeyVal>(&self, r : &mut R) -> MDHTResult<ProtoMessage<P,V>> {

    let lenu64 = tryfor!(BOErr,r.read_u64::<LittleEndian>());
    let len = lenu64 as usize;
    let mut buf = vec![0; len];
    try!(r.read(&mut buf));
    Ok(tryfor!(BEErr,bencode::from_buffer(&buf).map(|benc| {
      let mut decoder = bencode::Decoder::new(&benc);
      Decodable::decode(&mut decoder).unwrap()
    })))

 
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



/*
  fn encode<P : Peer, V : KeyVal> (&self, mesg : &ProtoMessage<P,V>) -> Option<Vec<u8>>{
    let r = bencode::encode(mesg);
    r.ok()
  }

  fn decode<P : Peer, V : KeyVal> (&self, buff : &[u8]) -> Option<ProtoMessage<P,V>>{
    debug!("decode msg {:?}", buff);
    bencode::from_buffer(buff).ok().map(|benc| {
      let mut decoder = bencode::Decoder::new(&benc);
      Decodable::decode(&mut decoder).unwrap()
    })
  }
  */

}
#[derive(RustcDecodable,RustcEncodable,Debug,Clone)]
/// keep trace of route to avoid loop when proxying
pub enum LastSent<P : Peer> {
  LastSentHop(usize, VecDeque<P::Key>),
  LastSentPeer(usize, VecDeque<P::Key>),
}

pub struct BEErr(pub BEError);

impl From<BEErr> for Error {
  #[inline]
  fn from(e : BEErr) -> Error {
    Error(e.0.msg, ErrorKind::ExternalLib, None)
  }
}


