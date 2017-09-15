
use rand::{thread_rng, Rng};
use std::io::Write;
use peer::{
  Peer,
  NoShadow,
  PeerMgmtMeths,
};
use procs::{
  //RunningContext, 
  RunningProcesses, 
  ArcRunningContext, 
  RunningTypes,
};
use std::sync::Arc;
use std::thread;

use keyval::{KeyVal};
use keyval::{Attachment,SettableAttachment};
use peer::PeerPriority;
//use utils;
use mydht_basetest::transport::LocalAdd;

// reexport
pub use mydht_basetest::peer::PeerTest;
pub use mydht_basetest::shadow::ShadowModeTest;

#[derive(Debug,Clone)]
/// Testing rules, simplified to have no dependencies (rules should use crypto to auth).
/// It does allways accept peer.
pub struct TestingRules {
  /// in percent
//  pub delay_variance : usize,
  /// in ms
  pub delay_ms_chal : u32,
  pub delay_ms_sign : u32,
  pub delay_ms_check : u32,
  pub delay_ms_accept : u32,
  pub delay_ms_accept_hook : u32,
}

impl TestingRules {
  pub fn new_no_delay() -> TestingRules {
    TestingRules {
//      delay_variance : 0,
      delay_ms_chal : 0,
      delay_ms_sign : 0,
      delay_ms_check : 0,
      delay_ms_accept : 0,
      delay_ms_accept_hook : 0,
    }
  }

  pub fn new_small_delay_light_accept() -> TestingRules {
    TestingRules {
      //delay_variance : 30,
      delay_ms_chal : 15,
      delay_ms_sign : 100,
      delay_ms_check : 50,
      delay_ms_accept : 50,
      delay_ms_accept_hook : 50,
    }
  }

  pub fn new_small_delay_heavy_accept(accdur : Option<u32>) -> TestingRules {
    TestingRules {
      //delay_variance : 30,
      delay_ms_chal : 15,
      delay_ms_sign : 100,
      delay_ms_check : 50,
      delay_ms_accept : accdur.unwrap_or(1000),
      delay_ms_accept_hook : 500,
    }
  }
}

impl<P : Peer, V : KeyVal> PeerMgmtMeths<P, V> for TestingRules {
  fn challenge (&self, _ : &P) -> Vec<u8> {
    thread::sleep_ms(self.delay_ms_chal);
    let mut s = vec![0; 4]; // four bytes challenge
    thread_rng().fill_bytes(&mut s);
    s
  }
  fn signmsg (&self, n : &P, chal : &[u8]) -> Vec<u8> {
    thread::sleep_ms(self.delay_ms_sign);
    format!("{:?}, {:?}",n.get_key(), chal).into_bytes()
  }
  fn checkmsg (&self, n : &P, chal : &[u8], sign : &[u8]) -> bool {
    thread::sleep_ms(self.delay_ms_check);
    format!("{:?}, {:?}",n.get_key(), chal).into_bytes() == sign
  }
  fn accept
  (&self, _ : &P)
  -> Option<PeerPriority> {
    thread::sleep_ms(self.delay_ms_accept);
    Some (PeerPriority::Normal)
  }
  fn for_accept_ping<M : PeerMgmtMeths<P,V>, RT : RunningTypes<P=P,V=V,A=P::Address,M=M>>
  (&self, n : &Arc<P>, _ : &RunningProcesses<RT>, _ : &ArcRunningContext<RT>) {
    thread::sleep_ms(self.delay_ms_accept_hook);
  }


}

/// test for PeerMgmtMeths auth primitives (challenge, signmsg, checkmsg)
pub fn basic_auth_test<P : Peer, R : PeerMgmtMeths<P,P>> (r : &R, p1 : &P, p2 : &P) {
  let chal1 = r.challenge(p1);
  let chal2 = r.challenge(p2);
  assert!(chal1 != chal2);
  let sign1 = r.signmsg(p1,&chal1);
  let sign2 = r.signmsg(p2,&chal2);
  assert!(r.checkmsg(p1,&chal1,&sign1));
  assert!(r.checkmsg(p2,&chal2,&sign2));
}

#[test]
fn test_testingrules () {
  let p1 = PeerTest {nodeid: "dummyID1".to_string(), address : LocalAdd(0), keyshift: 1, modeshauth : ShadowModeTest::NoShadow, modeshmsg : ShadowModeTest::SimpleShift};
  let p2 = PeerTest {nodeid: "dummyID2".to_string(), address : LocalAdd(1), keyshift: 2, modeshauth : ShadowModeTest::NoShadow, modeshmsg : ShadowModeTest::SimpleShift};
  let r = TestingRules::new_no_delay();
  basic_auth_test(&r,&p1,&p2);
}
