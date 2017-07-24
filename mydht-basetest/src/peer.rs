

use keyval::{KeyVal};
use keyval::{Attachment,SettableAttachment};
//use utils;
use transport::LocalAdd;
use mydht_base::route::byte_rep::{
    DHTElemBytes,
  };
use shadow::{
  ShadowTest,
};

use shadow::{
  ShadowModeTest,
  shadower_test,
};


// reexport
pub use mydht_base::peer::*;

#[derive(Deserialize,Serialize,Debug,PartialEq,Eq,Clone)]
/// Node using an usize as address (for use with transport tests)
pub struct PeerTest {
  pub nodeid  : String,
  pub address : LocalAdd,
  pub keyshift : u8,
  pub modesh : ShadowModeTest,
}

impl KeyVal for PeerTest {
  type Key = String;
  #[inline]
  fn get_key(& self) -> Self::Key {
    self.nodeid.clone()
  }
/* 
  #[inline]
  fn get_key_ref<'a>(&'a self) -> &'a NodeID {
    &self.nodeid
  }*/
  noattachment!();
}

impl SettableAttachment for PeerTest { }

impl Peer for PeerTest {
  type Address = LocalAdd;
  type Shadow = ShadowTest;
  fn get_address(&self) -> &Self::Address {
    &self.address
  }

  #[inline]
  fn get_shadower (&self, _ : bool) -> Self::Shadow {
    ShadowTest(self.keyshift,0,self.modesh.clone()) // default to no shadow
  }
  fn default_auth_mode(&self) -> <Self::Shadow as Shadow>::ShadowMode {
    ShadowModeTest::NoShadow
  }
  fn default_message_mode(&self) -> <Self::Shadow as Shadow>::ShadowMode {
    ShadowModeTest::SimpleShift
  }
  fn default_header_mode(&self) -> <Self::Shadow as Shadow>::ShadowMode {
    ShadowModeTest::SimpleShiftNoHead
  }

}

impl<'a> DHTElemBytes<'a> for PeerTest {
    // return ing Vec<u8> is stupid but it is for testing
    type Bytes = Vec<u8>;
    fn bytes_ref_keb (&'a self) -> Self::Bytes {
      self.nodeid.bytes_ref_keb()
      // res.push((self.address).0 as u8); // should be key related
    }
    fn kelem_eq_keb(&self, other : &Self) -> bool {
      self.nodeid == other.nodeid
      && self.address == other.address
    }
}


#[cfg(test)]
fn peertest_shadower_test (input_length : usize, write_buffer_length : usize,
read_buffer_length : usize, smode : ShadowModeTest) {

/*  let fromP = PeerTest {
    nodeid: "fromid".to_string(),
    address : LocalAdd(0),
    keyshift: 1,
  };*/
  let to_p = PeerTest {
    nodeid: "toid".to_string(),
    address : LocalAdd(1),
    keyshift: 2,
    modesh : smode.clone(),
  };
 
  shadower_test(to_p,input_length,write_buffer_length,read_buffer_length,smode);

}

#[test]
fn shadower1_test () {
  let smode = ShadowModeTest::NoShadow;
  let input_length = 256;
  let write_buffer_length = 256;
  let read_buffer_length = 256;
  peertest_shadower_test (input_length, write_buffer_length, read_buffer_length, smode);
}

#[test]
fn shadower2_test () {
  let smode = ShadowModeTest::SimpleShiftNoHead;
  let input_length = 20;
  let write_buffer_length = 15;
  let read_buffer_length = 25;
  peertest_shadower_test (input_length, write_buffer_length, read_buffer_length, smode);
}

#[test]
fn shadower3_test () {
  let smode = ShadowModeTest::SimpleShift;
  let input_length = 123;
  let write_buffer_length = 14;
  let read_buffer_length = 17;
  peertest_shadower_test (input_length, write_buffer_length, read_buffer_length, smode);
}

