use std::io::Cursor;
use mydht_base::msgenc::MsgEnc;

use mydht_base::msgenc::send_variant::ProtoMessage as ProtoMessageSend;
use mydht_base::msgenc::ProtoMessage;
use shadow::{
  ShadowModeTest,
};


use peer::PeerTest;
use transport::LocalAdd;

pub fn test_peer_enc<ME : MsgEnc> (e : ME) {
   let to_p = PeerTest {
    nodeid: "toid".to_string(),
    address : LocalAdd(1),
    keyshift: 2,
    modesh : ShadowModeTest::NoShadow,
  };
 
   let v1 = vec![1u8;155];
   let v2 = vec![3u8;30];
  let ms : ProtoMessageSend<PeerTest,PeerTest> = ProtoMessageSend::PING(&to_p,v1.clone(),v2.clone());
  let mut out = Cursor::new(Vec::new());
 
  e.encode_into(&mut out, &ms).unwrap();
  let mut input = Cursor::new(out.into_inner());
  let ms2 : ProtoMessage<PeerTest,PeerTest> = e.decode_from(&mut input).unwrap();
  if let ProtoMessage::PING(a,b,c) = ms2 {
    assert!(a == to_p);
    assert!(b == v1);
    assert!(c == v2);
  } else {
    panic!("wrong message type");
  }

}