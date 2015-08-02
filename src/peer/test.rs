
use rand::{thread_rng, Rng};
use super::{
  Peer,
  PeerMgmtMeths,
};
use procs::{
  RunningContext, 
  RunningProcesses, 
  ArcRunningContext, 
  RunningTypes,
};
use std::sync::Arc;
use std::thread;

use keyval::{KeyVal};
use keyval::{Attachment,SettableAttachment};
use peer::PeerPriority;
use utils;
use transport::LocalAdd;



#[derive(RustcDecodable,RustcEncodable,Debug,PartialEq,Eq,Clone)]
/// Node using an usize as address (for use with transport tests)
pub struct PeerTest {
  pub nodeid  : String,
  pub address : LocalAdd,
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
  fn to_address(&self) -> Self::Address {
    self.address.clone()
  }
}




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
  fn challenge (&self, n : &P) -> String {
    thread::sleep_ms(self.delay_ms_chal);
    thread_rng().gen::<usize>().to_string()
  }
  fn signmsg (&self, n : &P, chal : &String) -> String {
    thread::sleep_ms(self.delay_ms_sign);
    format!("{:?}, {}",n.get_key(), chal)
  }
  fn checkmsg (&self, n : &P, chal : &String, sign : &String) -> bool {
    thread::sleep_ms(self.delay_ms_check);
    format!("{:?}, {}",n.get_key(), chal) == *sign
  }
  fn accept<RT : RunningTypes<P=P,V=V>>
  (&self, n : &P, _ : &RunningProcesses<RT>, _ : &ArcRunningContext<RT>) 
  -> Option<PeerPriority> {
    thread::sleep_ms(self.delay_ms_accept);
    Some (PeerPriority::Normal)
  }
  fn for_accept_ping<RT : RunningTypes<P=P,V=V>>
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
  let p1 = PeerTest {nodeid: "dummyID1".to_string(), address : LocalAdd(0)};
  let p2 = PeerTest {nodeid: "dummyID2".to_string(), address : LocalAdd(1)};
  let r = TestingRules::new_no_delay();
  basic_auth_test(&r,&p1,&p2);
}
