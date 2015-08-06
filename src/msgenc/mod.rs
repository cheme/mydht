//! msgenc : type of encoding.
//! there is two kind of encoding :
//! full encoding all frame are encode
//! partial : an intermediatory frame structure is used : some dht property may be directly forced by being set on decode and not include in msg : encode may force some configuring as their is the common serialize interface and a prior filter on protomessage. This should use a full encoding when not to specific.
//! Most of the time encode do not need to be a type but in some case it could : eg signing of
//! message and/or cryptiong content


use keyval::{KeyVal,Attachment};
use peer::{Peer};
use query::{QueryID,QueryMsg};
use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};
use mydhtresult::Result as MDHTResult;
use std::io::Write;
use std::io::Read;
use mydhtresult::{Error,ErrorKind};
use std::fs::File;
use std::io::{Seek,SeekFrom};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use num::traits::ToPrimitive;
use utils;
use self::send_variant::ProtoMessage as ProtoMessageSend;

pub mod json;
pub mod bincode;
//pub mod bencode;

const BUFF_SIZE : usize = 10000; // use for attachment send/receive -- 21888 seems to be maxsize


/// Trait for message encoding between peers.
/// It use bytes which will be used by transport.
pub trait MsgEnc : Send + Sync + 'static {
  //fn encode<P : Peer, V : KeyVal>(&self, &ProtoMessage<P,V>) -> Option<Vec<u8>>;
  
  /// encode
  fn encode_into<'a,W : Write, P : Peer + 'a, V : KeyVal + 'a> (&self, w : &mut W, mesg : &ProtoMessageSend<'a,P,V>) -> MDHTResult<()>
where <P as Peer>::Address : 'a,
      <P as KeyVal>::Key : 'a,
      <V as KeyVal>::Key : 'a ;
 
  fn attach_into<W : Write> (&self, &mut W, Option<&Attachment>) -> MDHTResult<()>;
  /// decode
  fn decode<P : Peer, V : KeyVal>(&self, &[u8]) -> Option<ProtoMessage<P,V>>;
  fn decode_from<R : Read, P : Peer, V : KeyVal>(&self, &mut R) -> MDHTResult<ProtoMessage<P,V>>;
  fn attach_from<R : Read>(&self, &mut R) -> MDHTResult<Option<Attachment>>;
}

#[derive(RustcDecodable,Debug)]
/// Messages between peers
/// TODO ref variant for send !!!!
pub enum ProtoMessage<P : Peer, V : KeyVal> {
  /// Our node pinging plus challenge and message signing
  PING(P,String, String), 
  /// Ping reply with signature with
  ///  - emitter
  ///  - signing of challenge
  ///  P is added for reping on lost PING, TODO could be remove and simply origin key in pong
  PONG(P,String),
  /// reply to query of propagate, if no queryid is used it is a node propagate
  STORE_NODE(Option<QueryID>, Option<DistantEnc<P>>), // reply to synch or asynch query - note no mix in query mode -- no signature, since we go with a ping before adding a node (ping is signed) TODO allow a signing with primitive only for this ?? and possible not ping afterwad
  /// reply to query of propagate, if no queryid is used it is a node propagate
  STORE_VALUE(Option<QueryID>, Option<DistantEnc<V>>), // reply to synch or asynch query - note no mix in query mode
  /// reply to query of propagate
  STORE_VALUE_ATT(Option<QueryID>, Option<DistantEncAtt<V>>), // same as store value but use encoding distant with attachment
  /// Query for Peer
  FIND_NODE(QueryMsg<P>, P::Key), // int is remaining nb hop -- TODO plus message signing for private node communication (add a primitive to check mess like those
  /// Query for Value
  FIND_VALUE(QueryMsg<P>, V::Key),
}
pub mod send_variant {
use keyval::{KeyVal,Attachment};
use peer::{Peer};
use query::{QueryID,QueryMsg};
use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};
use super::{DistantEnc,DistantEncAtt};
use std::sync::Arc;

#[derive(RustcEncodable,Debug)]
pub enum ProtoMessage<'a,P : Peer + 'a, V : KeyVal + 'a> {
  PING(&'a P,String, String), 
  PONG(&'a P,String),
  STORE_NODE(Option<QueryID>, Option<DistantEnc<&'a P>>),
  STORE_VALUE(Option<QueryID>, Option<DistantEnc<&'a V>>),
  STORE_VALUE_ATT(Option<QueryID>, Option<DistantEncAtt<&'a V>>),
  FIND_NODE(QueryMsg<P>, P::Key),
  FIND_VALUE(QueryMsg<P>, V::Key),
}

}
#[derive(Debug)]
/// Choice of an encoding without attachment
pub struct DistantEnc<V> (pub V);
#[derive(Debug)]
/// Choice of an encoding with attachment
pub struct DistantEncAtt<V> (pub V);

impl<'a, V : KeyVal> Encodable for DistantEnc<&'a V>{
  fn encode<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
    // not local without attach
    self.0.encode_kv(s, false, false)
  }
}

impl<V : KeyVal> Decodable for DistantEnc<V> {
  fn decode<D:Decoder> (d : &mut D) -> Result<DistantEnc<V>, D::Error> {
    // not local without attach
    <V as KeyVal>::decode_kv(d, false, false).map(|v|DistantEnc(v))
  }
}

impl<'a, V : KeyVal> Encodable for DistantEncAtt<&'a V>{
  fn encode<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
    // not local with attach
        self.0.encode_kv(s, false, true)
  }
}

impl<V : KeyVal>  Decodable for DistantEncAtt<V> {
  fn decode<D:Decoder> (d : &mut D) -> Result<DistantEncAtt<V>, D::Error> {
    // not local with attach
    <V as KeyVal>::decode_kv(d, false, true).map(|v|DistantEncAtt(v))
  }
}


/// common utility for encode implementation (attachment should allways be following bytes)
fn write_attachment<W : Write> (w : &mut W, a : Option<&Attachment>) -> MDHTResult<()> {
    a.map(|path| {
      let mut f = try!(File::open(&path));
      debug!("trynig nwriting att");
      // send over buff size
      let fsize = f.metadata().unwrap().len();
      let nbframe = (fsize / (BUFF_SIZE).to_u64().unwrap()).to_usize().unwrap();
      let lfrsize = (fsize - (BUFF_SIZE.to_u64().unwrap() * nbframe.to_u64().unwrap())).to_usize().unwrap();
      debug!("fsize{:?}",fsize);
      debug!("nwbfr{:?}",nbframe);
      debug!("frsiz{:?}",lfrsize);
      debug!("busize{:?}",BUFF_SIZE);
      // TODO less headers??
      try!(w.write_u64::<LittleEndian>(fsize));
      //try!(w.write_u32::<LittleEndian>(nbframe.to_u32().unwrap()));
      //try!(w.write_u32::<LittleEndian>(BUFF_SIZE.to_u32().unwrap()));
      //try!(w.write_u32::<LittleEndian>(lfrsize.to_u32().unwrap()));
      f.seek(SeekFrom::Start(0));
      let buf = &mut [0; BUFF_SIZE];
      for i in 0..(nbframe + 1) {
        debug!("fread : {:?}", i);
        let nb = try!(f.read(buf));
        if (nb == BUFF_SIZE) {
          try!(w.write_all(buf));
        } else {
          if (nb != lfrsize){
            return Err(Error("mismatch file size calc for tcp transport".to_string(), ErrorKind::IOError, None));
          };
          // truncate buff
          try!(w.write(&buf[..nb]));
        }
      };
      Ok(())
    }).unwrap_or(Ok(()))
}

fn read_attachment(s : &mut Read)-> MDHTResult<Attachment> {

  //let nbframe = try!(s.read_u32::<LittleEndian>()).to_usize().unwrap();
  //let bsize   = try!(s.read_u32::<LittleEndian>()).to_usize().unwrap();
  //let lfrsize = try!(s.read_u32::<LittleEndian>()).to_usize().unwrap();
  let fsize = try!(s.read_u64::<LittleEndian>());
  let nbframe = (fsize / (BUFF_SIZE).to_u64().unwrap()).to_usize().unwrap();
  let lfrsize = (fsize - (BUFF_SIZE.to_u64().unwrap() * nbframe.to_u64().unwrap())).to_usize().unwrap();
 
  // TODO change : buffer size in message is useless and dangerous
  debug!("bs{:?}",BUFF_SIZE);
  debug!("nwbfr{:?}",nbframe);
  debug!("frsiz{:?}",lfrsize);
  let (fp, mut f) = utils::create_tmp_file();
  let buf = &mut [0; BUFF_SIZE];
  for i in 0..(nbframe + 1) {
    let nb = try!(s.read(buf));
    if (nb == BUFF_SIZE) {
      f.write(buf);
    } else {
      if (nb != lfrsize){
        // todo delete file
        return Err(Error("mismatch received file size calc for tcp transport".to_string(), ErrorKind::IOError, None));
      };
      // truncate buff
      f.write(&buf[..nb]);
    }
  };

  try!(f.seek(SeekFrom::Start(0)));
  Ok(fp)
}




