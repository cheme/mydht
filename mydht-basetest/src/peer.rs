
#[cfg(test)]
use readwrite_comp::{ExtWrite,ExtRead};

#[cfg(test)]
use std::io::{Write,Read};

#[cfg(test)]
use rand::thread_rng;
#[cfg(test)]
use rand::Rng;
#[cfg(test)]
use std::io::Cursor;
use keyval::{KeyVal};
use keyval::{Attachment,SettableAttachment};
//use utils;
use transport::LocalAdd;
use mydht_base::route::byte_rep::{
    DHTElemBytes,
};
use shadow::{
  ShadowTest,
  shadower_test,
};
use shadow::{
  ShadowModeTest,
};


// reexport
pub use mydht_base::peer::*;

#[derive(Deserialize,Serialize,Debug,PartialEq,Eq,Clone)]
/// Node using an usize as address (for use with transport tests)
pub struct PeerTest {
  pub nodeid  : String,
  pub address : LocalAdd,
  pub keyshift : u8,
  pub modeshauth : ShadowModeTest,
  pub modeshmsg : ShadowModeTest,
}

impl KeyVal for PeerTest {
  type Key = String;
  #[inline]
  fn get_key(& self) -> Self::Key {
    self.nodeid.clone()
  }
  #[inline]
  fn get_key_ref(&self) -> &Self::Key {
    &self.nodeid
  }
  noattachment!();
}

impl SettableAttachment for PeerTest { }

impl Peer for PeerTest {
  type Address = LocalAdd;
  type ShadowWAuth = ShadowTest;
  type ShadowRAuth = ShadowTest;
  type ShadowWMsg = ShadowTest;
  type ShadowRMsg = ShadowTest;
  fn get_address(&self) -> &Self::Address {
    &self.address
  }

  #[inline]
  fn get_shadower_w_auth (&self) -> Self::ShadowWAuth {
    ShadowTest(self.keyshift,0,self.modeshauth.clone()) // default to no shadow
  }
  #[inline]
  fn get_shadower_r_auth (&self) -> Self::ShadowRAuth {
    ShadowTest(self.keyshift,0,self.modeshauth.clone()) // default to no shadow
  }
  #[inline]
  fn get_shadower_w_msg (&self) -> Self::ShadowWMsg {
    ShadowTest(self.keyshift,0,self.modeshmsg.clone()) // default to no shadow
  }
  #[inline]
  fn get_shadower_r_msg (&self) -> Self::ShadowRMsg {
    ShadowTest(self.keyshift,0,self.modeshmsg.clone()) // default to no shadow
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
read_buffer_length : usize, smodeauth : ShadowModeTest, smodemsg : ShadowModeTest) {

/*  let fromP = PeerTest {
    nodeid: "fromid".to_string(),
    address : LocalAdd(0),
    keyshift: 1,
  };*/
  let to_p = PeerTest {
    nodeid: "toid".to_string(),
    address : LocalAdd(1),
    keyshift: 2,
    modeshauth : smodeauth.clone(),
    modeshmsg : smodemsg.clone(),
  };
 
  shadower_test(to_p.clone(),input_length,write_buffer_length,read_buffer_length);
  // non std
  shadower_sym(to_p,input_length,write_buffer_length,read_buffer_length);

}
#[cfg(test)]
pub fn shadower_sym (to_p : PeerTest, input_length : usize, write_buffer_length : usize,
read_buffer_length : usize) 
{

  let mut inputb = vec![0;input_length];
  thread_rng().fill_bytes(&mut inputb);
  let mut output = Cursor::new(Vec::new());
  let input = inputb;
  let mut from_shad = to_p.get_shadower_w_msg();
  let mut to_shad = to_p.get_shadower_r_msg();

  // sim test
  let sim_shad = ShadowTest::new_shadow_sim().unwrap();
  let mut ix = 0;
  let k = {
    let mut wkey = Cursor::new(Vec::new());
    sim_shad.send_shadow_simkey(&mut wkey).unwrap();
    wkey.into_inner()
  };
  let mut ki = Cursor::new(&k[..]);
  let mut shad_sim_w =  ShadowTest::init_from_shadow_simkey(&mut ki).unwrap();
  let mut ki = Cursor::new(&k[..]);
  let mut shad_sim_r =  ShadowTest::init_from_shadow_simkey(&mut ki).unwrap();
  let k2 = {
    let mut wkey = Cursor::new(Vec::new());
    shad_sim_r.send_shadow_simkey(&mut wkey).unwrap();
    wkey.into_inner()
  };
  assert_eq!(k,k2);
 
  while ix < input_length {
    if ix + write_buffer_length < input_length {
      ix += shad_sim_w.write_into(&mut output, &input[ix..ix + write_buffer_length]).unwrap();
    } else {
      ix += shad_sim_w.write_into(&mut output, &input[ix..]).unwrap();
    }
  }
  let el = output.get_ref().len();
  shad_sim_w.write_end(&mut output).unwrap();
  shad_sim_w.flush_into(&mut output).unwrap();
  output.flush().unwrap();
  let el = output.get_ref().len();
  ix = 0;
  let mut readbuf = vec![0;read_buffer_length];

  let mut input_v = Cursor::new(output.into_inner());
  while ix < input_length {
    let l = shad_sim_r.read_from(&mut input_v, &mut readbuf).unwrap();
    assert!(l!=0);

    assert!(&readbuf[..l] == &input[ix..ix + l]);
    ix += l;
  }

  let l = shad_sim_r.read_from(&mut input_v, &mut readbuf).unwrap();
  assert!(l==0);
  shad_sim_r.read_end(&mut input_v).unwrap();

}
#[test]
fn shadower1_test () {
  let smode = ShadowModeTest::NoShadow;
  let input_length = 256;
  let write_buffer_length = 256;
  let read_buffer_length = 256;
  peertest_shadower_test (input_length, write_buffer_length, read_buffer_length, ShadowModeTest::NoShadow, smode);
}

#[test]
fn shadower2_test () {
  let smode = ShadowModeTest::SimpleShiftNoHead;
  let input_length = 20;
  let write_buffer_length = 15;
  let read_buffer_length = 25;
  peertest_shadower_test (input_length, write_buffer_length, read_buffer_length, ShadowModeTest::NoShadow, smode);
}

#[test]
fn shadower3_test () {
  let smode = ShadowModeTest::SimpleShift;
  let input_length = 123;
  let write_buffer_length = 14;
  let read_buffer_length = 17;
  peertest_shadower_test (input_length, write_buffer_length, read_buffer_length, ShadowModeTest::NoShadow, smode);
}

