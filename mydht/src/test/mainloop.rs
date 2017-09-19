//! Test with main loop usage


extern crate mydht_tcp_loop;
extern crate mydht_slab;
use std::sync::Arc;
use std::mem::replace;
use std::thread;
use mydhtresult::{
  Result,
};
use std::sync::mpsc::{
  Receiver as MpscReceiver,
  Sender as MpscSender,
};

use std::collections::HashMap;
use keyval::KeyVal;
use msgenc::json::Json;
use msgenc::MsgEnc;
use self::mydht_slab::slab::{
  Slab,
};
use self::mydht_tcp_loop::{
  Tcp,
};
use procs::{
  MyDHTConf,
  RWSlabEntry,
};
use procs::{
  PeerCacheEntry,
  ChallengeEntry,
  ApiCommand,
};
use std::net::{SocketAddr,Ipv4Addr};
use transport::{
  Transport,
  SerSocketAddr,
};
use node::Node;
use service::{
  SpawnChannel,
  MpscChannel,
  MpscChannelRef,
  NoChannel,
  LocalRcChannel,

  SpawnSend,
  LocalRc,
 // MpscSender,
  NoSend,

  Spawner,
  Blocker,
  RestartOrError,
  Coroutine,
  RestartSameThread,
  ThreadBlock,
  ThreadPark,
  ThreadParkRef,

  CpuPool,
  CpuPoolFuture,
};
use super::DHTRULES_DEFAULT;
use mydht_basetest::transport::{
  LocalAdd,
};
use mydht_basetest::peer::{
  PeerTest,
};

use mydht_basetest::local_transport::{
  AsynchTransportTest,
};
use peer::test::{
  TestingRules,
  ShadowModeTest,
};
use utils;
use utils::{
  Ref,
  ArcRef,
  RcRef,
  CloneRef,
};
use simplecache::SimpleCache;

use rules::simplerules::{
  SimpleRules,
  DhtRules,
};

/// peer name, listener port, is_multiplexed
pub struct TestMdhtConf (pub String, pub usize, pub bool);

mod test_tcp_all_block_thread {
  use super::*;
  impl MyDHTConf for TestMdhtConf {

    const loop_name : &'static str = "Conf test spawner";
    const events_size : usize = 1024;
    const send_nb_iter : usize = 1;
    type MainloopSpawn = ThreadParkRef;// -> failure to send into spawner cf command in of spawner need send so the mpsc channel recv could be send in impl -» need to change command in to commandin as ref :: toref
//    type MainloopSpawn = Blocker;
    //type MainLoopChannelIn = MpscChannelRef;
    type MainLoopChannelIn = MpscChannel;
    type MainLoopChannelOut = MpscChannel;
    type Transport = Tcp;
    type MsgEnc = Json;
    type Peer = Node;
    type PeerRef = ArcRef<Self::Peer>;
    //type PeerRef = RcRef<Self::Peer>;
    type KeyVal = Node;
    type PeerMgmtMeths = TestingRules;
    type DHTRules = Arc<SimpleRules>;
    type Slab = Slab<RWSlabEntry<Self>>;
    type PeerCache = HashMap<<Self::Peer as KeyVal>::Key,PeerCacheEntry<Self::PeerRef>>;
    type ChallengeCache = HashMap<Vec<u8>,ChallengeEntry>;
    type PeerMgmtChannelIn = MpscChannel;
    type ReadChannelIn = MpscChannel;
    type ReadSpawn = ThreadPark;
    //type ReadSpawn = Blocker;
    type WriteDest = NoSend;
    type WriteChannelIn = MpscChannel;
//    type WriteChannelIn = LocalRcChannel;
   // type WriteSpawn = Blocker;
    type WriteSpawn = ThreadPark;

    fn init_ref_peer(&mut self) -> Result<Self::PeerRef> {
       let addr = utils::sa4(Ipv4Addr::new(127,0,0,1), self.1 as u16);
       let val = Node {nodeid: self.0.clone(), address : SerSocketAddr(addr)};
       Ok(ArcRef::new(val))
      // Ok(RcRef::new(val))
    }
    fn get_main_spawner(&mut self) -> Result<Self::MainloopSpawn> {
      //Ok(Blocker)
      //Ok(ThreadPark)
      Ok(ThreadParkRef)
    }

    fn init_main_loop_slab_cache(&mut self) -> Result<Self::Slab> {
      Ok(Slab::new())
    }
    fn init_main_loop_peer_cache(&mut self) -> Result<Self::PeerCache> {
      Ok(HashMap::new())
    }
    fn init_main_loop_challenge_cache(&mut self) -> Result<Self::ChallengeCache> {
      Ok(HashMap::new())
    }


    fn init_main_loop_channel_in(&mut self) -> Result<Self::MainLoopChannelIn> {
      Ok(MpscChannel)
      //Ok(MpscChannelRef)
    }
    fn init_main_loop_channel_out(&mut self) -> Result<Self::MainLoopChannelOut> {
      Ok(MpscChannel)
    }


    fn init_read_spawner(&mut self) -> Result<Self::ReadSpawn> {
      Ok(ThreadPark)
      //Ok(Blocker)
    }

    fn init_write_spawner(&mut self) -> Result<Self::WriteSpawn> {
      Ok(ThreadPark)
      //Ok(Blocker)
    }

    fn init_write_spawner_out() -> Result<Self::WriteDest> {
      Ok(NoSend)
    }
    fn init_read_channel_in(&mut self) -> Result<Self::ReadChannelIn> {
      Ok(MpscChannel)
    }
    fn init_write_channel_in(&mut self) -> Result<Self::WriteChannelIn> {
//      Ok(LocalRcChannel)
      Ok(MpscChannel)
    }
    fn init_peermgmt_channel_in(&mut self) -> Result<Self::PeerMgmtChannelIn> {
      Ok(MpscChannel)
    }


    fn init_enc_proto(&mut self) -> Result<Self::MsgEnc> {
      Ok(Json)
    }

    fn init_transport(&mut self) -> Result<Self::Transport> {

      let addr = utils::sa4(Ipv4Addr::new(127,0,0,1), self.1 as u16);
      Ok(Tcp::new(&addr, None, self.2)?)
    }
    fn init_peermgmt_proto(&mut self) -> Result<Self::PeerMgmtMeths> {
      Ok(TestingRules::new_no_delay())
    }
    fn init_dhtrules_proto(&mut self) -> Result<Self::DHTRules> {
      Ok(Arc::new(SimpleRules::new(DHTRULES_DEFAULT)))
    }



  }


  #[test]
  fn test_connect_t() {
    let conf1 = TestMdhtConf("peer1".to_string(), 48880, true);
    let port2 = 48881;
    let conf2 = TestMdhtConf("peer2".to_string(), port2, true);

    //let state1 = conf1.init_state().unwrap();
    //let state2 = conf2.init_state().unwrap();

    let (sendcommand2,_) = conf2.start_loop().unwrap();
    // avoid connection refused TODO replace by a right connect test (ping address)
    thread::sleep_ms(100);
    let (mut sendcommand1,_) = conf1.start_loop().unwrap();
    let addr2 = utils::sa4(Ipv4Addr::new(127,0,0,1), port2 as u16);
    let command = ApiCommand::try_connect(SerSocketAddr(addr2));
    let addr3 = utils::sa4(Ipv4Addr::new(127,0,0,1), port2 as u16);
    let command2 = ApiCommand::try_connect(SerSocketAddr(addr3));

  //  thread::sleep_ms(1000); // issue with this sleep : needded
    sendcommand1.send(command).unwrap();
    thread::sleep_ms(1000);
    sendcommand1.send(command2).unwrap();

    // no service to check connection, currently only for testing and debugging : sleep
    thread::sleep_ms(10000);

  }
}

mod test_dummy_all_block_thread {
  use super::*;
  use std::time::Duration;
  pub struct TestMdhtConf1(pub TestMdhtConf,pub Option<AsynchTransportTest>);
  impl MyDHTConf for TestMdhtConf1 {

    const loop_name : &'static str = "Conf test spawner";
    const events_size : usize = 1024;
    const send_nb_iter : usize = 1;
    type MainloopSpawn = ThreadPark;
    type MainLoopChannelIn = MpscChannel;
    type MainLoopChannelOut = MpscChannel;
    type Transport = AsynchTransportTest;
    type MsgEnc = Json;
    type Peer = PeerTest;
    type PeerRef = ArcRef<Self::Peer>;
    type KeyVal = PeerTest;
    type PeerMgmtMeths = TestingRules;
    type DHTRules = Arc<SimpleRules>;
    type Slab = Slab<RWSlabEntry<Self>>;
    type PeerCache = HashMap<<Self::Peer as KeyVal>::Key,PeerCacheEntry<Self::PeerRef>>;
    type ChallengeCache = HashMap<Vec<u8>,ChallengeEntry>;
    type PeerMgmtChannelIn = MpscChannel;
    type ReadChannelIn = MpscChannel;
    type ReadSpawn = ThreadPark;
    type WriteDest = NoSend;
    type WriteChannelIn = MpscChannel;
    type WriteSpawn = ThreadPark;

    fn init_ref_peer(&mut self) -> Result<Self::PeerRef> {
      Ok(ArcRef::new(PeerTest {
        nodeid  : (self.0).0.clone(),
        address : LocalAdd((self.0).1),
        keyshift : 5,
        modeshauth : ShadowModeTest::NoShadow,
        modeshmsg : ShadowModeTest::SimpleShift,
      }))
    }
    fn get_main_spawner(&mut self) -> Result<Self::MainloopSpawn> {
      Ok(ThreadPark)
    }

    fn init_main_loop_slab_cache(&mut self) -> Result<Self::Slab> {
      Ok(Slab::new())
    }
    fn init_main_loop_peer_cache(&mut self) -> Result<Self::PeerCache> {
      Ok(HashMap::new())
    }
    fn init_main_loop_challenge_cache(&mut self) -> Result<Self::ChallengeCache> {
      Ok(HashMap::new())
    }


    fn init_main_loop_channel_in(&mut self) -> Result<Self::MainLoopChannelIn> {
      Ok(MpscChannel)
    }
    fn init_main_loop_channel_out(&mut self) -> Result<Self::MainLoopChannelOut> {
      Ok(MpscChannel)
    }


    fn init_read_spawner(&mut self) -> Result<Self::ReadSpawn> {
      Ok(ThreadPark)
    }

    fn init_write_spawner(&mut self) -> Result<Self::WriteSpawn> {
      Ok(ThreadPark)
    }

    fn init_write_spawner_out() -> Result<Self::WriteDest> {
      Ok(NoSend)
    }
    fn init_read_channel_in(&mut self) -> Result<Self::ReadChannelIn> {
      Ok(MpscChannel)
    }
    fn init_write_channel_in(&mut self) -> Result<Self::WriteChannelIn> {
      Ok(MpscChannel)
    }
    fn init_peermgmt_channel_in(&mut self) -> Result<Self::PeerMgmtChannelIn> {
      Ok(MpscChannel)
    }

    fn init_enc_proto(&mut self) -> Result<Self::MsgEnc> {
      Ok(Json)
    }
    fn init_peermgmt_proto(&mut self) -> Result<Self::PeerMgmtMeths> {
      Ok(TestingRules::new_no_delay())
    }
    fn init_dhtrules_proto(&mut self) -> Result<Self::DHTRules> {
      Ok(Arc::new(SimpleRules::new(DHTRULES_DEFAULT)))
    }

 
    fn init_transport(&mut self) -> Result<Self::Transport> {
      let addr = LocalAdd((self.0).1);
      let tr = replace(&mut self.1,None);
      Ok(tr.unwrap())
    }

  }


#[test]
fn test_connect_l() {
 let is_mult = true;
    //let tr = AsynchTransportTest::create_transport (nb : usize, multiplex : bool, managed : bool, conn : Duration, sen : Duration, rec : Duration) -> Vec<AsynchTransportTest>();
    let mut tr = AsynchTransportTest::create_transport (2, is_mult, true, Duration::from_millis(500), Duration::from_millis(100), Duration::from_millis(100));

    tr.reverse();
    let conf1 = TestMdhtConf1(TestMdhtConf("peer1".to_string(), 0, true),tr.pop());
    let conf2 = TestMdhtConf1(TestMdhtConf("peer2".to_string(), 1, true),tr.pop());

    //let state1 = conf1.init_state().unwrap();
    //let state2 = conf2.init_state().unwrap();

    let (mut sendcommand1,_) = conf1.start_loop().unwrap();
    let (sendcommand2,_) = conf2.start_loop().unwrap();
    let addr1 = LocalAdd(1);
    let command = ApiCommand::try_connect(addr1);
    //  No support for double connect with AsynchTransportTest!!!
//    let addr2 = LocalAdd(1);
//    let command2 = ApiCommand::try_connect(addr2);

  //  thread::sleep_ms(1000); // issue with this sleep : needded
    sendcommand1.send(command).unwrap();
/*    thread::sleep_ms(1000);
    sendcommand1.send(command2).unwrap();*/

    // no service to check connection, currently only for testing and debugging : sleep
    thread::sleep_ms(10000);

  }
}
