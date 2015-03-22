use rustc_serialize::json;
use super::MsgEnc;
use kvstore::{KeyVal};
use peer::{Peer};
use super::ProtoMessage;


// standard usage of rust serialize with json over proto messages no intemediatory type all content
// is used : full encoding
#[derive(Debug,Clone)]
pub struct Json;


unsafe impl Send for Json {
}

impl MsgEnc for Json {
  fn encode<P : Peer, V : KeyVal> (&self, mesg : &ProtoMessage<P,V>) -> Option<Vec<u8>>{
    json::encode(mesg).ok().map(|m| m.into_bytes())
  }
 
  fn decode<P : Peer, V : KeyVal> (&self, buff : &[u8]) -> Option<ProtoMessage<P,V>>{
    // TODO should not & on deref of cow...
    json::decode(&(*String::from_utf8_lossy(buff))).ok()
  }

}

