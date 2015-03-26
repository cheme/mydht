extern crate bincode;
use rustc_serialize::{Encodable,Decodable};
use super::MsgEnc;
use kvstore::{KeyVal};
use peer::{Peer};
use super::ProtoMessage;
use std::collections::BTreeMap;


// full bencode impl
#[derive(Debug,Clone)]
pub struct Bincode;

impl MsgEnc for Bincode {

  fn encode<P : Peer, V : KeyVal> (&self, mesg : &ProtoMessage<P,V>) -> Option<Vec<u8>>{
    debug!("encode msg {:?}", mesg);
    let r = bincode::encode(mesg, bincode::SizeLimit::Infinite);
    r.ok()
  }

  fn decode<P : Peer, V : KeyVal> (&self, buff : &[u8]) -> Option<ProtoMessage<P,V>>{
    debug!("decode msg {:?}", buff);
    bincode::decode(buff).ok()
  }

}


