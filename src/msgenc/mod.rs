//! msgenc : type of encoding.
//! there is two kind of encoding :
//! full encoding all frame are encode
//! partial : an intermediatory frame structure is used : some dht property may be directly forced by being set on decode and not include in msg : encode may force some configuring as their is the common serialize interface and a prior filter on protomessage. This should use a full encoding when not to specific.
//! Most of the time encode do not need to be a type but in some case it could : eg signing of
//! message and/or cryptiong content


use kvstore::{KeyVal};
use peer::{Peer};
use query::{QueryID,QueryConfMsg};
use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};

pub mod json;
pub mod bencode;
pub mod bincode;


/// Trait for message encoding between peers.
/// It use bytes which will be used by transport.
pub trait MsgEnc : Send + Sync + 'static {
  /// encode
  fn encode<P : Peer, V : KeyVal>(&self, &ProtoMessage<P,V>) -> Option<Vec<u8>>;
  /// decode
  fn decode<P : Peer, V : KeyVal>(&self, &[u8]) -> Option<ProtoMessage<P,V>>;
}

#[derive(RustcDecodable,RustcEncodable,Debug)]
/// Messages between peers
pub enum ProtoMessage<P : Peer, V : KeyVal> {
  /// Our node pinging plus challenge and message signing
  PING(P,String, String), 
  /// Ping reply with signature TODO add ourself in pong (in order to call for_accept_ping after
  /// and obviously update peer content (if peer can change (less likely than in a ping))).
  PONG(String),
  /// Ping reply with signature and challenge (TODO currently no persistence of the ping so bypass
  /// security)
  APONG(P,String, String), // peer challenge, mess sig, signature
  /// reply to query of propagate
  STORE_NODE(Option<QueryID>, Option<DistantEnc<P>>), // reply to synch or asynch query - note no mix in query mode -- no signature, since we go with a ping before adding a node (ping is signed) TODO allow a signing with primitive only for this ?? and possible not ping afterwad
  /// reply to query of propagate
  STORE_VALUE(Option<QueryID>, Option<DistantEnc<V>>), // reply to synch or asynch query - note no mix in query mode
  /// reply to query of propagate
  STORE_VALUE_ATT(Option<QueryID>, Option<DistantEncAtt<V>>), // same as store value but use encoding distant with attachment
  /// Query for Peer
  FIND_NODE(QueryConfMsg<P>, P::Key), // int is remaining nb hop -- TODO plus message signing for private node communication (add a primitive to check mess like those
  /// Query for Value
  FIND_VALUE(QueryConfMsg<P>, V::Key),
}

#[derive(Debug)]
/// Choice of an encoding without attachment
/// TODO evolve to DistantEnc( & V) : use case allow it (serialize run imediatly after), here a
/// useless clone involved
pub struct DistantEnc<V : KeyVal> (pub V);
#[derive(Debug)]
/// Choice of an encoding with attachment
/// TODO evolve to DistantEnc( & V) : use case allow it (serialize run imediatly after), here a
/// useless clone involved
pub struct DistantEncAtt<V : KeyVal> (pub V);

impl<V : KeyVal>  Encodable for DistantEnc<V>{
  fn encode<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
    self.0.encode_dist(s)
  }
}

impl<V : KeyVal>  Decodable for DistantEnc<V> {
  fn decode<D:Decoder> (d : &mut D) -> Result<DistantEnc<V>, D::Error> {
    <V as KeyVal>::decode_dist(d).map(|v|DistantEnc(v))
  }
}

impl<V : KeyVal>  Encodable for DistantEncAtt<V>{
  fn encode<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
        self.0.encode_dist_with_att(s)
  }
}

impl<V : KeyVal>  Decodable for DistantEncAtt<V> {
  fn decode<D:Decoder> (d : &mut D) -> Result<DistantEncAtt<V>, D::Error> {
    <V as KeyVal>::decode_dist_with_att(d).map(|v|DistantEncAtt(v))
  }
}

