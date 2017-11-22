
use rand::{thread_rng, Rng};
use std::thread;
use std::time::Duration;
use peer::{
  Peer,
  PeerMgmtMeths,
};

use peer::PeerPriority;
//use utils;
use mydht_basetest::transport::LocalAdd;



// reexport
pub use mydht_basetest::peer::{
  PeerTest,
  basic_auth_test,
};
pub use mydht_basetest::shadow::ShadowModeTest;

#[derive(Debug,Clone)]
/// Testing rules, simplified to have no dependencies (rules should use crypto to auth).
/// It does allways accept peer.
pub struct TestingRules {
  /// in percent
//  pub delay_variance : usize,
  /// in ms
  pub delay_ms_chal : u64,
  pub delay_ms_sign : u64,
  pub delay_ms_check : u64,
  pub delay_ms_accept : u64,
  pub delay_ms_accept_hook : u64,
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
/* Do not delete : for next tests
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

  pub fn new_small_delay_heavy_accept(accdur : Option<u64>) -> TestingRules {
    TestingRules {
      //delay_variance : 30,
      delay_ms_chal : 15,
      delay_ms_sign : 100,
      delay_ms_check : 50,
      delay_ms_accept : accdur.unwrap_or(1000),
      delay_ms_accept_hook : 500,
    }
  }
  */
}

impl<P : Peer> PeerMgmtMeths<P> for TestingRules {
  fn challenge (&self, _ : &P) -> Vec<u8> {
    thread::sleep(Duration::from_millis(self.delay_ms_chal));
    let mut s = vec![0; 4]; // four bytes challenge
    thread_rng().fill_bytes(&mut s);
    s
  }
  fn signmsg (&self, n : &P, chal : &[u8]) -> Vec<u8> {
    thread::sleep(Duration::from_millis(self.delay_ms_sign));
    format!("{:?}, {:?}",n.get_key(), chal).into_bytes()
  }
  fn checkmsg (&self, n : &P, chal : &[u8], sign : &[u8]) -> bool {
    thread::sleep(Duration::from_millis(self.delay_ms_check));
    format!("{:?}, {:?}",n.get_key(), chal).into_bytes() == sign
  }
  fn accept
  (&self, _ : &P)
  -> Option<PeerPriority> {
    thread::sleep(Duration::from_millis(self.delay_ms_accept));
    Some (PeerPriority::Normal)
  }
  /*fn for_accept_ping<M : PeerMgmtMeths<P,V>, RT : RunningTypes<P=P,V=V,A=P::Address,M=M>>
  (&self, n : &Arc<P>, _ : &RunningProcesses<RT>, _ : &ArcRunningContext<RT>) {
    thread::sleep_ms(self.delay_ms_accept_hook);
  }*/


}
#[test]
fn test_testingrules () {
  let p1 = PeerTest {nodeid: "dummyID1".to_string(), address : LocalAdd(0), keyshift: 1, modeshauth : ShadowModeTest::NoShadow, modeshmsg : ShadowModeTest::SimpleShift};
  let p2 = PeerTest {nodeid: "dummyID2".to_string(), address : LocalAdd(1), keyshift: 2, modeshauth : ShadowModeTest::NoShadow, modeshmsg : ShadowModeTest::SimpleShift};
  let r = TestingRules::new_no_delay();
  basic_auth_test(&r,&p1,&p2);
}
