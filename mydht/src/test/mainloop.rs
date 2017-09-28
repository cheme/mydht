//! Test with main loop usage


extern crate mydht_tcp_loop;
extern crate mydht_slab;
use std::time::Instant;
use std::time::Duration;
use procs::storeprop::{
  KVStoreCommand,
};
use utils::{
  OneResult,
  new_oneresult,
  clone_wait_one_result,
};
use procs::OptInto;
use procs::api::{
  Api,
  ApiReply,
  ApiResult,
  ApiQueriable,
  ApiQueryId,
  ApiRepliable,
};
use procs::{
  ApiCommand,
  ApiSendIn,
  MyDHTService,
};
use procs::{
  MainLoopReply,
  MainLoopCommand,
};
use kvcache::KVCache;

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
use keyval::{
  KeyVal,
  GettableAttachments,
  SettableAttachments,
  Attachment,
};
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
  ShadowAuthType,
  Route,
};
use procs::{
  PeerCacheEntry,
  ChallengeEntry,
};
use procs::deflocal::{
  GlobalCommand,
  GlobalReply,
  DefLocalService,
};

use std::net::{SocketAddr,Ipv4Addr};
use transport::{
  Transport,
  SerSocketAddr,
};
use node::Node;
use service::{
  Service,
  MioChannel,
  SpawnChannel,
  MpscChannel,
  MpscChannelRef,
  NoChannel,
  NoRecv,
  LocalRcChannel,
  SpawnerYield,
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
use std::marker::PhantomData;
use rules::simplerules::{
  SimpleRules,
  DhtRules,
};

/// test service message
#[derive(Serialize,Deserialize,Debug)]
/// Messages between peers
/// TODO ref variant for send !!!!
#[serde(bound(deserialize = ""))]
pub enum TestMessage {
  Touch,
  TouchQ(Option<usize>,usize),
  TouchQR(Option<usize>),
}
impl GettableAttachments for TestMessage {
  fn get_attachments(&self) -> Vec<&Attachment> {
    Vec::new()
  }
}

impl SettableAttachments for TestMessage {
  fn attachment_expected_sizes(&self) -> Vec<usize> {
    Vec::new()
  }
  fn set_attachments(& mut self, at : &[Attachment]) -> bool {
    at.len() == 0
  }
}

pub enum TestCommand<MC : MyDHTConf> {
  Touch,
  /// first is query ix, second nb forward for q
  TouchQ(Option<usize>,usize),
  /// param is query ix from which we receive q : this is a distant reply to a touchq
  TouchQR(Option<usize>),
  Ph(PhantomData<MC>),
}
impl<MC : MyDHTConf> Clone for TestCommand<MC> {
  fn clone(&self) -> Self {
    match *self {
      TestCommand::Touch => TestCommand::Touch,
      TestCommand::TouchQ(a,b) => TestCommand::TouchQ(a,b),
      TestCommand::TouchQR(a) => TestCommand::TouchQR(a),
      TestCommand::Ph(..) => TestCommand::Ph(PhantomData),
    }
  }
}
// communicate peers ??
impl<MC : MyDHTConf> OptInto<KVStoreCommand<MC::Peer,MC::Peer>> for TestCommand<MC> {
  #[inline]
  fn can_into(&self) -> bool {
    false
  }

  #[inline]
  fn opt_into(self) -> Option<KVStoreCommand<MC::Peer,MC::Peer>> {
    None
  }

}
#[derive(Clone)]
pub enum TestReply {
  Touch,
  TouchQ(Option<usize>),
}
/*impl OptInto<TestMessage> for TestReply {
  fn can_into(&self) -> bool {
    match *self {
      TestReply::Touch => false,
      TestReply::TouchQ(_) => false,
    }
  }
  fn opt_into(self) -> Option<TestMessage> {
    None
  }
}
*/
impl<MC : MyDHTConf> Into<TestCommand<MC>> for TestMessage {
  fn into(self) -> TestCommand<MC> {
    match self {
      TestMessage::Touch => TestCommand::Touch,
      TestMessage::TouchQ(qid,nbfor) => TestCommand::TouchQ(qid,nbfor),
      TestMessage::TouchQR(qid) => TestCommand::TouchQR(qid),
    }
  }
}
impl<MC : MyDHTConf> OptInto<TestMessage> for TestCommand<MC> {
  fn can_into(&self) -> bool {
    match *self {
      TestCommand::Touch => true,
      TestCommand::TouchQ(..) => true,
      TestCommand::TouchQR(..) => true,
      TestCommand::Ph(..) => false,
    }
  }
  fn opt_into(self) -> Option<TestMessage> {
    match self {
      TestCommand::Touch => Some(TestMessage::Touch),
      TestCommand::TouchQ(qid,nbfor) => Some(TestMessage::TouchQ(qid,nbfor)),
      TestCommand::TouchQR(qid) => Some(TestMessage::TouchQR(qid)),
      TestCommand::Ph(..) => None,
    }
  }
}

impl<MC : MyDHTConf> ApiQueriable for TestCommand<MC> {
  #[inline]
  fn is_api_reply(&self) -> bool {
    match *self {
      TestCommand::Touch => false,
      TestCommand::TouchQ(qid,_) => true,
      TestCommand::TouchQR(..) => false,
      TestCommand::Ph(..) => false,
    }
  }
  #[inline]
  fn set_api_reply(&mut self, aid : ApiQueryId) {
    match *self {
      TestCommand::Touch => (),
      TestCommand::TouchQ(ref mut qid,_) => *qid = Some(aid.0),
      TestCommand::TouchQR(..) => (),
      TestCommand::Ph(..) => (),
    }

  }
}

impl ApiRepliable for TestReply {
  #[inline]
  fn get_api_reply(&self) -> Option<ApiQueryId> {
    match *self {
      TestReply::Touch => None,
      TestReply::TouchQ(ref qid) => qid.as_ref().map(|id|ApiQueryId(*id)),
    }
  }
}


pub struct TestRoute<MDC : MyDHTConf>(PhantomData<MDC>);


pub struct TestService<MDC : MyDHTConf>(PhantomData<MDC>);

/// peer name, listener port, is_multiplexed
pub struct TestMdhtConf (pub String, pub usize, pub bool);

mod test_tcp_all_block_thread {
  use super::*;

  impl Service for TestService<TestMdhtConf> 
  {
    type CommandIn = GlobalCommand<<TestMdhtConf as MyDHTConf>::PeerRef,<TestMdhtConf as MyDHTConf>::GlobalServiceCommand>;
    type CommandOut = GlobalReply<TestMdhtConf>;
    fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, async_yield : &mut S) -> Result<Self::CommandOut> {
      match req {
        GlobalCommand(_,TestCommand::Ph(..)) => unreachable!(),
        GlobalCommand(owith,TestCommand::Touch) => {
          println!("TOUCH!!!");
          Ok(GlobalReply::NoRep)
        },
        GlobalCommand(Some(p),TestCommand::TouchQ(id,nb_for)) => {
          // no local storage
          Ok(GlobalReply::Forward(Some(vec![p]),None,0,TestCommand::TouchQR(id)))
        },
        GlobalCommand(None,TestCommand::TouchQ(id,nb_for)) => {
          println!("TOUCHQ!!!{:?}",id);
          //Ok(GlobalReply(TestReply::TouchQ(id)))
          let mut res = Vec::with_capacity(1 + nb_for);
    
          res.push(GlobalReply::Api(TestReply::TouchQ(id)));
          for _ in 0 .. nb_for {
            res.push(GlobalReply::Forward(None,None,1,TestCommand::TouchQ(id,0)));
          }
          Ok(GlobalReply::Mult(res))
  //Forward(Option<Vec<MC::PeerRef>>,usize,MC::GlobalServiceCommand),
        },
        GlobalCommand(owith,TestCommand::TouchQR(id)) => {
          println!("TOUCHQR!!!{:?}",id);
          Ok(GlobalReply::Api(TestReply::TouchQ(id)))
        },
      }
    }
  }
  impl Route<TestMdhtConf> for TestRoute<TestMdhtConf> {

    fn route_global(&mut self, targetted_nb : usize, c : TestCommand<TestMdhtConf>,sl : &<TestMdhtConf as MyDHTConf>::Slab, cache : &<TestMdhtConf as MyDHTConf>::PeerCache) -> Result<(TestCommand<TestMdhtConf>,Vec<usize>)> {
      self.route(targetted_nb,c,sl,cache)
    }
    fn route(&mut self, targetted_nb : usize, c : TestCommand<TestMdhtConf>,_ : &<TestMdhtConf as MyDHTConf>::Slab, cache : &<TestMdhtConf as MyDHTConf>::PeerCache) -> Result<(TestCommand<TestMdhtConf>,Vec<usize>)> {
      let mut res = Vec::with_capacity(targetted_nb);
      match c {
        TestCommand::Ph(..) => unreachable!(),
        TestCommand::Touch | TestCommand::TouchQ(..) => {
          cache.strict_fold_c(&mut res,|res, kv|{
            if let Some(t) = kv.1.get_write_token() {
              if res.len() < targetted_nb {
                res.push(t);
              }
            }
            res
          });
        },
        TestCommand::TouchQR(..) => (),
      }
      Ok((c,res))
    }
  }

  impl MyDHTConf for TestMdhtConf {

    const loop_name : &'static str = "Conf test spawner";
    const events_size : usize = 1024;
    const send_nb_iter : usize = 1;
    type Route = TestRoute<Self>;
    type MainloopSpawn = ThreadPark;// -> failure to send into spawner cf command in of spawner need send so the mpsc channel recv could be send in impl -» need to change command in to commandin as ref :: toref
    //type MainloopSpawn = ThreadParkRef;// -> failure to send into spawner cf command in of spawner need send so the mpsc channel recv could be send in impl -» need to change command in to commandin as ref :: toref
//    type MainloopSpawn = Blocker;
    //type MainLoopChannelIn = MpscChannelRef;
    type MainLoopChannelIn = MpscChannel;
    type MainLoopChannelOut = MpscChannel;
    type Transport = Tcp;
    type MsgEnc = Json;
    type Peer = Node;
    type PeerRef = ArcRef<Self::Peer>;
    //type PeerRef = RcRef<Self::Peer>;
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

    // TODO default associated type must be set manually (TODO check if still needed with next
    // versions)
    type ProtoMsg = TestMessage;
    type LocalServiceCommand = TestCommand<Self>;
    type LocalServiceReply = TestReply;
  /*  type GlobalServiceCommand  = GlobalCommand<Self>; // def
    type LocalService = DefLocalService<Self>; // def
    const LOCAL_SERVICE_NB_ITER : usize = 1; // def
    type LocalServiceSpawn = Blocker; // def
    type LocalServiceChannelIn = NoChannel; // def*/
    nolocal!();
    type GlobalService = TestService<Self>;
    type GlobalServiceSpawn = ThreadPark;
    type GlobalServiceChannelIn = MpscChannel;
    type ApiReturn = OneResult<(Vec<ApiResult<Self>>,usize,usize)>;
    type ApiService = Api<Self,HashMap<ApiQueryId,(OneResult<(Vec<ApiResult<Self>>,usize,usize)>,Instant)>>;

    type ApiServiceSpawn = ThreadPark;
    type ApiServiceChannelIn = MpscChannel;




    fn init_ref_peer(&mut self) -> Result<Self::PeerRef> {
       let addr = utils::sa4(Ipv4Addr::new(127,0,0,1), self.1 as u16);
       let val = Node {nodeid: self.0.clone(), address : SerSocketAddr(addr)};
       Ok(ArcRef::new(val))
      // Ok(RcRef::new(val))
    }
    fn get_main_spawner(&mut self) -> Result<Self::MainloopSpawn> {
      //Ok(Blocker)
      Ok(ThreadPark)
//      Ok(ThreadParkRef)
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

    fn init_global_spawner(&mut self) -> Result<Self::GlobalServiceSpawn> {
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

    fn init_global_service(&mut self) -> Result<Self::GlobalService> {
      Ok(TestService(PhantomData))
    }

    fn init_global_channel_in(&mut self) -> Result<Self::GlobalServiceChannelIn> {
      Ok(MpscChannel)
    }

    fn init_route(&mut self) -> Result<Self::Route> {
      Ok(TestRoute(PhantomData))
    }

    fn init_api_service(&mut self) -> Result<Self::ApiService> {
      Ok(Api(HashMap::new(),Duration::from_millis(3000),0,PhantomData))
    }

    fn init_api_channel_in(&mut self) -> Result<Self::ApiServiceChannelIn> {
      Ok(MpscChannel)
    }
    fn init_api_spawner(&mut self) -> Result<Self::ApiServiceSpawn> {
      Ok(ThreadPark)
      //Ok(Blocker)
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
 //   let addr3 = utils::sa4(Ipv4Addr::new(127,0,0,1), port2 as u16);
 //   let command2 = ApiCommand::try_connect(SerSocketAddr(addr3));

  //  thread::sleep_ms(1000); // issue with this sleep : needded
    sendcommand1.send(command).unwrap();
    thread::sleep_ms(1000);
//    let touch = ApiCommand::local_service(TestCommand::Touch);
    let touch = ApiCommand::call_service(TestCommand::Touch);
   // let o_res = new_oneresult((Vec::with_capacity(1),1,1));
    let o_res = new_oneresult((Vec::with_capacity(2),2,2));
    let touchq = ApiCommand::call_service_reply(TestCommand::TouchQ(None,1),o_res.clone());
    sendcommand1.send(touch).unwrap();
    sendcommand1.send(touchq).unwrap();
    let o_res = clone_wait_one_result(&o_res,None).unwrap();
    assert!(o_res.0.len() == 2);
    for v in o_res.0.iter() {
      assert!(if let &ApiResult::GlobalServiceReply(TestReply::TouchQ(Some(1))) = v {true} else {false});
    }
//    sendcommand1.send(command2).unwrap();

    // no service to check connection, currently only for testing and debugging : sleep
    thread::sleep_ms(10000);

  }
}

/*mod test_dummy_all_block_thread {
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
    type GlobalServiceCommand = GlobalCommand<Self>;

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
}*/
