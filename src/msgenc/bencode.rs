extern crate bencode;
use rustc_serialize::{Encodable,Decodable};
use super::MsgEnc;
use kvstore::{KeyVal};
use peer::{Peer};
use std::old_io::{IoResult,IoError};
use super::ProtoMessage;
use std::collections::BTreeMap;


// full bencode impl
#[derive(Debug,Clone)]
pub struct Bencode;

impl MsgEnc for Bencode {

  fn encode<P : Peer, V : KeyVal> (&self, mesg : &ProtoMessage<P,V>) -> Option<Vec<u8>>{
    debug!("encode msg {:?}", mesg);
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

}
