//! Test with main loop usage


extern crate mydht_tcp_loop;
extern crate mydht_slab;
use kvstore::StoragePriority;
use query::{
  //Query,
 // QReply,
  QueryModeMsg,
  QueryMsg,
//  PropagateMsg,
  //QueryPriority,
};
use utils::{
  Ref,
  SRef,
  SToRef,
};

use kvcache::KVCache;
use service::{
  SpawnSend,
};

use query::simplecache::{
  SimpleCacheQuery,
  HashMapQuery,
};
use std::time::Instant;
use std::time::Duration;
use procs::storeprop::{
  KVStoreCommand,
  KVStoreProtoMsg,
  //KVStoreProtoMsgSend,
};
use utils::{
  OneResult,
  new_oneresult,
  replace_wait_one_result,
};
use procs::{
  OptInto,
  OptFrom,
  MCCommand,
  MCReply,
  MCReplySend,
};
use procs::api::{
  Api,
  ApiResult,
  ApiResultSend,
  ApiQueriable,
  ApiQueryId,
  ApiRepliable,
};
use procs::{
  ApiCommand,
  FWConf,
};

use std::sync::Arc;
use std::thread;
use mydhtresult::{
  Result,
};

use std::collections::HashMap;
use keyval::{
  KeyVal,
  GettableAttachments,
  SettableAttachments,
  Attachment,
};
use msgenc::json::Json;
use self::mydht_slab::slab::{
  Slab,
};
use self::mydht_tcp_loop::{
  Tcp,
};
use procs::{
  MyDHTConf,
  RWSlabEntry,
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

use std::net::{Ipv4Addr};
use transport::{
  SerSocketAddr,
};
use node::Node;
use service::{
  Service,
 // MioChannel,
 // SpawnChannel,
  MpscChannel,
  MpscChannelRef,
  //NoRecv,
  LocalRcChannel,
  SpawnerYield,
 // LocalRc,
 // MpscSender,
  NoSend,

  Blocker,
  RestartOrError,
 // Coroutine,
 // RestartSameThread,
 // ThreadBlock,
  ThreadPark,
  ThreadParkRef,

  NoSpawn,
  NoChannel,
 // CpuPool,
 // CpuPoolFuture,
};
use super::DHTRULES_DEFAULT;
/*use mydht_basetest::transport::{
  LocalAdd,
};
use mydht_basetest::peer::{
  PeerTest,
};
use mydht_basetest::local_transport::{
  AsynchTransportTest,
};*/
use peer::test::{
  TestingRules,
};
use utils;
use utils::{
  ArcRef,
  RcRef,
 // CloneRef,
};
use peer::Peer;
use simplecache::SimpleCache;
use std::marker::PhantomData;
use rules::simplerules::{
  SimpleRules,
};

/// test service message
#[derive(Serialize,Deserialize,Debug)]
/// Messages between peers
/// TODO ref variant for send !!!!
#[serde(bound(deserialize = ""))]
pub enum TestMessage<MC : MyDHTConf> {
  Touch,
  TouchQ(Option<usize>,usize),
  TouchQR(Option<usize>),
  PeerMgmt(KVStoreProtoMsg<MC::Peer,MC::Peer,MC::PeerRef>),
}
/*
#[derive(Serialize,Debug)]
pub enum TestMessageSend<'a,P : Peer> {
  Touch,
  TouchQ(Option<usize>,usize),
  TouchQR(Option<usize>),
  PeerMgmt(KVStoreProtoMsgSend<'a,P,P>),
}
*/
impl<MC : MyDHTConf> GettableAttachments for TestMessage<MC> {
  fn get_attachments(&self) -> Vec<&Attachment> {
    Vec::new()
  }
}

impl<MC : MyDHTConf> SettableAttachments for TestMessage<MC> {
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
  _Ph(PhantomData<MC>),
}
impl<MC : MyDHTConf> Clone for TestCommand<MC> {
  fn clone(&self) -> Self {
    match *self {
      TestCommand::Touch => TestCommand::Touch,
      TestCommand::TouchQ(a,b) => TestCommand::TouchQ(a,b),
      TestCommand::TouchQR(a) => TestCommand::TouchQR(a),
      TestCommand::_Ph(..) => TestCommand::_Ph(PhantomData),
    }
  }
}
impl<MC : MyDHTConf> SRef for TestCommand<MC> {
  type Send = Self;
  fn get_sendable(self) -> Self::Send {
    self
  }
}
impl<MC : MyDHTConf> SToRef<TestCommand<MC>> for TestCommand<MC> {
  fn to_ref(self) -> TestCommand<MC> {
    self
  }
}


#[derive(Clone)]
pub enum TestReply {
  TouchQ(Option<usize>),
}

impl SRef for TestReply {
  type Send = Self;
  fn get_sendable(self) -> Self::Send {
    self
  }
}
impl SToRef<TestReply> for TestReply {
  fn to_ref(self) -> TestReply {
    self
  }
}


impl<MC : MyDHTConf> OptInto<TestMessage<MC>> for TestCommand<MC> {
  fn can_into(&self) -> bool {
    match *self {
      TestCommand::Touch => true,
      TestCommand::TouchQ(..) => true,
      TestCommand::TouchQR(..) => true,
      TestCommand::_Ph(..) => false,
    }
  }
  fn opt_into(self) -> Option<TestMessage<MC>> {
    match self {
      TestCommand::Touch => Some(TestMessage::Touch),
      TestCommand::TouchQ(qid,nbfor) => Some(TestMessage::TouchQ(qid,nbfor)),
      TestCommand::TouchQR(qid) => Some(TestMessage::TouchQR(qid)),
      TestCommand::_Ph(..) => None,
    }
  }
}

impl<MC : MyDHTConf> ApiQueriable for TestCommand<MC> {
  #[inline]
  fn is_api_reply(&self) -> bool {
    match *self {
      TestCommand::Touch => false,
      TestCommand::TouchQ(..) => true,
      TestCommand::TouchQR(..) => false,
      TestCommand::_Ph(..) => false,
    }
  }
  #[inline]
  fn set_api_reply(&mut self, aid : ApiQueryId) {
    match *self {
      TestCommand::Touch => (),
      TestCommand::TouchQ(ref mut qid,_) => *qid = Some(aid.0),
      TestCommand::TouchQR(..) => (),
      TestCommand::_Ph(..) => (),
    }

  }
  #[inline]
  fn get_api_reply(&self) -> Option<ApiQueryId> {
    match *self {
      TestCommand::TouchQ(ref qid,_) =>  qid.as_ref().map(|id|ApiQueryId(*id)),
      _ => None,
    }

  }

}

impl ApiRepliable for TestReply {
  #[inline]
  fn get_api_reply(&self) -> Option<ApiQueryId> {
    match *self {
      TestReply::TouchQ(ref qid) => qid.as_ref().map(|id|ApiQueryId(*id)),
    }
  }
}


pub struct TestRoute<MC : MyDHTConf>(PhantomData<MC>);

pub struct TestService<MC : MyDHTConf>(PhantomData<MC>);

sref_self_mc!(TestService);

/*impl<MC : MyDHTConf> SRef for  GlobalDest<MC> {
  type Send = Self;
  #[inline]
  fn get_sendable(self) -> Self::Send { self }
}
impl<MC : MyDHTConf> SToRef<GlobalDest<MC>> for  GlobalDest<MC> {
  fn to_ref(self) -> GlobalDest<MC> { self }
}*/



macro_rules! service_conf_test{($testconf:ident,$testtest:ident,$startport:expr,$apires:ident,$mcrep:ident) => (
  /// peer name, listener port, is_multiplexed
  pub struct $testconf (pub String, pub usize, pub bool);

  impl Service for TestService<$testconf> 
  {
    type CommandIn = GlobalCommand<<$testconf as MyDHTConf>::PeerRef,<$testconf as MyDHTConf>::GlobalServiceCommand>;
    type CommandOut = GlobalReply<<$testconf as MyDHTConf>::Peer,<$testconf as MyDHTConf>::PeerRef,<$testconf as MyDHTConf>::GlobalServiceCommand,<$testconf as MyDHTConf>::GlobalServiceReply>;
    fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, _async_yield : &mut S) -> Result<Self::CommandOut> {
      match req {
        GlobalCommand(_,TestCommand::_Ph(..)) => unreachable!(),
        GlobalCommand(owith,TestCommand::Touch) => {
          println!("TOUCH!!!{:?}",owith.is_some());
          Ok(GlobalReply::NoRep)
        },
        GlobalCommand(Some(p),TestCommand::TouchQ(id,_nb_for)) => {
          println!("TOUCHQ dist !!!{:?}",id);
          // no local storage
          Ok(GlobalReply::Forward(Some(vec![p]),None,FWConf{nb_for : 0, discover:false},TestCommand::TouchQR(id)))
        },
        GlobalCommand(None,TestCommand::TouchQ(id,nb_for)) => {
          println!("TOUCHQ!!!{:?}",id);
          //Ok(GlobalReply(TestReply::TouchQ(id)))
          let mut res = Vec::with_capacity(1 + nb_for);
    
          res.push(GlobalReply::Api(TestReply::TouchQ(id)));
          for _ in 0 .. nb_for {
            res.push(GlobalReply::Forward(None,None,FWConf{nb_for : 1, discover:false},TestCommand::TouchQ(id,0)));
          }
          Ok(GlobalReply::Mult(res))
  //Forward(Option<Vec<MC::PeerRef>>,usize,MC::GlobalServiceCommand),
        },
        GlobalCommand(owith,TestCommand::TouchQR(id)) => {
          println!("TOUCHQR!!!{:?} , with {:?}",id,owith.is_some());
          Ok(GlobalReply::Api(TestReply::TouchQ(id)))
        },
      }
    }
  }
  impl Route<$testconf> for TestRoute<$testconf> {

    fn route(&mut self, targetted_nb : usize, c : MCCommand<$testconf>,_ : &mut <$testconf as MyDHTConf>::Slab, cache : &mut <$testconf as MyDHTConf>::PeerCache) -> Result<(MCCommand<$testconf>,Vec<usize>)> {
      let mut res = Vec::with_capacity(targetted_nb);
      cache.strict_fold_c(&mut res,|res, kv|{
        if let Some(t) = kv.1.get_write_token() {
          if res.len() < targetted_nb {
            res.push(t);
          }
        }
        res
      });
      Ok((c,res))
    }
  }
  impl Into<MCCommand<$testconf>> for TestMessage<$testconf> {
    fn into(self) -> MCCommand<$testconf> {
      match self {
        TestMessage::Touch => MCCommand::Local(TestCommand::Touch),
        TestMessage::TouchQ(qid,nbfor) => MCCommand::Local(TestCommand::TouchQ(qid,nbfor)),
        TestMessage::TouchQR(qid) => MCCommand::Local(TestCommand::TouchQR(qid)),
        TestMessage::PeerMgmt(pmess) => MCCommand::PeerStore(pmess.into()),
      }
    }
  }
  impl OptFrom<MCCommand<$testconf>> for TestMessage<$testconf> {
    fn can_from(c : &MCCommand<$testconf>) -> bool {
      match *c {
        MCCommand::Local(ref lc) => {
          <TestCommand<$testconf> as OptInto<TestMessage<$testconf>>>::can_into(lc)
        },
        MCCommand::Global(ref gc) => {
          <TestCommand<$testconf> as OptInto<TestMessage<$testconf>>>::can_into(gc)
        },
        MCCommand::PeerStore(ref pc) => {
          <KVStoreProtoMsg<_,_,_> as OptFrom<KVStoreCommand<_,_,_,_>>>::can_from(pc)
        },
        MCCommand::TryConnect(..) => {
          false
        },

      }
    }
    fn opt_from(c : MCCommand<$testconf>) -> Option<Self> {
      match c {
        MCCommand::Local(lc) => {
          lc.opt_into()
        },
        MCCommand::Global(gc) => {
          gc.opt_into()
        },
        MCCommand::PeerStore(pc) => {
          pc.opt_into().map(|t|TestMessage::PeerMgmt(t))
        },
        MCCommand::TryConnect(..) => {
          None
        },
      }
    }
  }

  #[test]
  fn $testtest() {
    let start_port = $startport;
    let conf1 = $testconf("peer1".to_string(), start_port, true);
    let port2 = start_port + 1;
    let conf2 = $testconf("peer2".to_string(), port2, true);

    //let state1 = conf1.init_state().unwrap();
    //let state2 = conf2.init_state().unwrap();

    let (_sendcommand2,_) = conf2.start_loop().unwrap();
    // avoid connection refused TODO replace by a right connect test (ping address)
    thread::sleep(Duration::from_millis(100));
    let (mut sendcommand1,_) = conf1.start_loop().unwrap();
    let addr2 = utils::sa4(Ipv4Addr::new(127,0,0,1), port2 as u16);
    let command = ApiCommand::try_connect(SerSocketAddr(addr2));
 //   let addr3 = utils::sa4(Ipv4Addr::new(127,0,0,1), port2 as u16);
 //   let command2 = ApiCommand::try_connect(SerSocketAddr(addr3));

  //  thread::sleep_ms(1000); // issue with this sleep : needded
    sendcommand1.send(command).unwrap();
    thread::sleep(Duration::from_millis(1000));
//    let touch = ApiCommand::local_service(TestCommand::Touch);
    let touch = ApiCommand::call_service(TestCommand::Touch);
   // let o_res = new_oneresult((Vec::with_capacity(1),1,1));
    let o_res = new_oneresult((Vec::with_capacity(2),2,2));
    let touchq = ApiCommand::call_service_reply(TestCommand::TouchQ(None,1),o_res.clone());
    sendcommand1.send(touch).unwrap();
    sendcommand1.send(touchq).unwrap();
    let o_res = replace_wait_one_result(&o_res,(Vec::new(),0,999)).unwrap();
    assert!(o_res.0.len() == 2);
    for v in o_res.0.iter() {
      assert!(if let &$apires::ServiceReply($mcrep::Global(TestReply::TouchQ(Some(1)))) = v {true} else {false});
    }
    let o_res = new_oneresult((Vec::with_capacity(2),2,2));
    let peer_local = ApiCommand::call_peer_reply(KVStoreCommand::Find(query_message_1(),"peer2".to_string(),None),o_res.clone());
    let peer_self = ApiCommand::call_peer_reply(KVStoreCommand::Find(query_message_1(),"peer1".to_string(),None),o_res.clone());
    sendcommand1.send(peer_local).unwrap();
    sendcommand1.send(peer_self).unwrap();
    let o_res = replace_wait_one_result(&o_res,(Vec::new(),0,999)).unwrap();
    assert!(o_res.0.len() == 2);
  //Find(QueryMsg<P>, V::Key,Option<ApiQueryId>),
//    sendcommand1.send(command2).unwrap();

    // no service to check connection, currently only for testing and debugging : sleep
    thread::sleep(Duration::from_millis(3000));

  }

)}

pub fn query_message_1<P : Peer>() -> QueryMsg<P> {
  QueryMsg {
    mode_info : QueryModeMsg::AProxy(0),
    hop_hist : None,
    // TODO delete storage prio
    storage : StoragePriority::Local,
    rem_hop : 1,
    nb_forw : 1,
    prio : 0,
    nb_res : 1,
  }
}


service_conf_test!(TestAllThConf,test_connect_all_th,48880,ApiResult,MCReply);

impl MyDHTConf for TestAllThConf {

  const LOOP_NAME : &'static str = "Conf test spawner threads";
  const EVENTS_SIZE : usize = 1024;
  const SEND_NB_ITER : usize = 1;
  type Route = TestRoute<Self>;
  type MainloopSpawn = ThreadPark;// -> failure to send into spawner cf command in of spawner need send so the mpsc channel recv could be send in impl -» need to change command in to commandin as ref :: toref
  //type MainloopSpawn = ThreadParkRef;// -> failure to send into spawner cf command in of spawner need send so the mpsc channel recv could be send in impl -» need to change command in to commandin as ref :: toref
//    type MainloopSpawn = Blocker;
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
  type ChallengeCache = HashMap<Vec<u8>,ChallengeEntry<Self>>;
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
  type ProtoMsg = TestMessage<Self>;
  type LocalServiceCommand = TestCommand<Self>;
  type LocalServiceReply = TestReply;
/*  type GlobalServiceCommand  = GlobalCommand<Self>; // def
  type LocalService = DefLocalService<Self>; // def
  const LOCAL_SERVICE_NB_ITER : usize = 1; // def
  type LocalServiceSpawn = Blocker; // def
  type LocalServiceChannelIn = NoChannel; // def*/
  localproxyglobal!();
  type GlobalService = TestService<Self>;
  type GlobalServiceSpawn = ThreadPark;
  type GlobalServiceChannelIn = MpscChannel;
  type ApiReturn = OneResult<(Vec<ApiResult<Self>>,usize,usize)>;
  type ApiService = Api<Self,HashMap<ApiQueryId,(OneResult<(Vec<ApiResult<Self>>,usize,usize)>,Instant)>>;

  type ApiServiceSpawn = ThreadPark;
  type ApiServiceChannelIn = MpscChannel;


//`, `PeerKVStore`, `PeerKVStoreInit`, ``, `, `init_peer_kvstore
  type PeerStoreQueryCache = SimpleCacheQuery<Self::Peer,Self::PeerRef,Self::PeerRef,HashMapQuery<Self::Peer,Self::PeerRef,Self::PeerRef>>;
  type PeerStoreServiceSpawn = ThreadPark; // TODO should behave with local return suspend
  type PeerStoreServiceChannelIn = MpscChannel;
  type PeerKVStore = SimpleCache<Self::Peer,HashMap<<Self::Peer as KeyVal>::Key,Self::Peer>>;

  type SynchListenerSpawn = ThreadPark;

  const NB_SYNCH_CONNECT : usize = 0;
  type SynchConnectChannelIn = NoChannel;
  type SynchConnectSpawn = NoSpawn;


  fn init_peer_kvstore(&mut self) -> Result<Box<Fn() -> Result<Self::PeerKVStore> + Send>> {
    Ok(Box::new(
      ||{
        Ok(SimpleCache::new(None))
      }
    ))
  }
  fn do_peer_query_forward_with_discover(&self) -> bool {
    false
  }
  fn init_peer_kvstore_query_cache(&mut self) -> Result<Box<Fn() -> Result<Self::PeerStoreQueryCache> + Send>> {
    Ok(Box::new(
      ||{
        // non random id
        Ok(SimpleCacheQuery::new(false))
      }
    ))
  }
  fn init_peerstore_channel_in(&mut self) -> Result<Self::PeerStoreServiceChannelIn> {
    Ok(MpscChannel)
  }
  fn init_peerstore_spawner(&mut self) -> Result<Self::PeerStoreServiceSpawn> {
    Ok(ThreadPark)
  }
//impl<P : Peer, V : KeyVal, RP : Ref<P>> SimpleCacheQuery<P,V,RP,HashMapQuery<P,V,RP>> {
// QueryCache<Self::Peer,Self::PeerRef,Self::PeerRef>;
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
  fn init_synch_listener_spawn(&mut self) -> Result<Self::SynchListenerSpawn> {
    Ok(ThreadPark)
  }
   fn init_synch_connect_spawn(&mut self) -> Result<Self::SynchConnectSpawn> {
    Ok(NoSpawn)
  }
  fn init_synch_connect_channel_in(&mut self) -> Result<Self::SynchConnectChannelIn> {
    Ok(NoChannel)
  }
}

service_conf_test!(TestLocalConf,test_connect_all_local,48883,ApiResultSend,MCReplySend);

impl MyDHTConf for TestLocalConf {

  const LOOP_NAME : &'static str = "Conf test spawner local";
  const EVENTS_SIZE : usize = 1024;
  const SEND_NB_ITER : usize = 1;
  type Route = TestRoute<Self>;
  type MainloopSpawn = ThreadParkRef;
  type MainLoopChannelIn = MpscChannelRef;
  type MainLoopChannelOut = MpscChannelRef;
  type Transport = Tcp;
  type MsgEnc = Json;
  type Peer = Node;
  type PeerRef = RcRef<Self::Peer>;
  //type PeerRef = ArcRef<Self::Peer>;
  type PeerMgmtMeths = TestingRules;
  type DHTRules = Arc<SimpleRules>;
  type Slab = Slab<RWSlabEntry<Self>>;
  type PeerCache = HashMap<<Self::Peer as KeyVal>::Key,PeerCacheEntry<Self::PeerRef>>;
  type ChallengeCache = HashMap<Vec<u8>,ChallengeEntry<Self>>;
  type PeerMgmtChannelIn = MpscChannelRef;
  type ReadChannelIn = MpscChannelRef;
  type ReadSpawn = ThreadParkRef;
  type WriteDest = NoSend;
  type WriteChannelIn = MpscChannelRef;
  type WriteSpawn = ThreadParkRef;
  type ProtoMsg = TestMessage<Self>;
  type LocalServiceCommand = TestCommand<Self>;
  type LocalServiceReply = TestReply;
  localproxyglobal!();
  type GlobalService = TestService<Self>;
  type GlobalServiceSpawn = ThreadParkRef;
  type GlobalServiceChannelIn = MpscChannelRef;
  type ApiReturn = OneResult<(Vec<ApiResultSend<Self>>,usize,usize)>;
  type ApiService = Api<Self,HashMap<ApiQueryId,(OneResult<(Vec<ApiResultSend<Self>>,usize,usize)>,Instant)>>;
  type ApiServiceSpawn = RestartOrError;
  type ApiServiceChannelIn = LocalRcChannel;
  type PeerStoreQueryCache = SimpleCacheQuery<Self::Peer,Self::PeerRef,Self::PeerRef,HashMapQuery<Self::Peer,Self::PeerRef,Self::PeerRef>>;
  type PeerStoreServiceSpawn = ThreadParkRef;
  type PeerStoreServiceChannelIn = MpscChannelRef;
  type PeerKVStore = SimpleCache<Self::Peer,HashMap<<Self::Peer as KeyVal>::Key,Self::Peer>>;
  type SynchListenerSpawn = NoSpawn;
  const NB_SYNCH_CONNECT : usize = 0;
  type SynchConnectChannelIn = NoChannel;
  type SynchConnectSpawn = NoSpawn;


  fn init_peer_kvstore(&mut self) -> Result<Box<Fn() -> Result<Self::PeerKVStore> + Send>> {
    Ok(Box::new(
      ||{
        Ok(SimpleCache::new(None))
      }
    ))
  }
  fn do_peer_query_forward_with_discover(&self) -> bool {
    false
  }
  fn init_peer_kvstore_query_cache(&mut self) -> Result<Box<Fn() -> Result<Self::PeerStoreQueryCache> + Send>> {
    Ok(Box::new(
      ||{
        // non random id
        Ok(SimpleCacheQuery::new(false))
      }
    ))
  }
  fn init_peerstore_channel_in(&mut self) -> Result<Self::PeerStoreServiceChannelIn> {
    Ok(MpscChannelRef)
  }
  fn init_peerstore_spawner(&mut self) -> Result<Self::PeerStoreServiceSpawn> {
    Ok(ThreadParkRef)
  }
  fn init_ref_peer(&mut self) -> Result<Self::PeerRef> {
     let addr = utils::sa4(Ipv4Addr::new(127,0,0,1), self.1 as u16);
     let val = Node {nodeid: self.0.clone(), address : SerSocketAddr(addr)};
     Ok(RcRef::new(val))
    // Ok(RcRef::new(val))
  }
  fn get_main_spawner(&mut self) -> Result<Self::MainloopSpawn> {
    //Ok(Blocker)
    Ok(ThreadParkRef)
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
    Ok(MpscChannelRef)
    //Ok(MpscChannelRef)
  }
  fn init_main_loop_channel_out(&mut self) -> Result<Self::MainLoopChannelOut> {
    Ok(MpscChannelRef)
  }
  fn init_read_spawner(&mut self) -> Result<Self::ReadSpawn> {
    Ok(ThreadParkRef)
    //Ok(Blocker)
  }
  fn init_write_spawner(&mut self) -> Result<Self::WriteSpawn> {
    Ok(ThreadParkRef)
    //Ok(Blocker)
  }
  fn init_global_spawner(&mut self) -> Result<Self::GlobalServiceSpawn> {
    Ok(ThreadParkRef)
    //Ok(Blocker)
  }
  fn init_write_spawner_out() -> Result<Self::WriteDest> {
    Ok(NoSend)
  }
  fn init_read_channel_in(&mut self) -> Result<Self::ReadChannelIn> {
    Ok(MpscChannelRef)
  }
  fn init_write_channel_in(&mut self) -> Result<Self::WriteChannelIn> {
    Ok(MpscChannelRef)
  }
  fn init_peermgmt_channel_in(&mut self) -> Result<Self::PeerMgmtChannelIn> {
    Ok(MpscChannelRef)
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
    Ok(MpscChannelRef)
  }

  fn init_route(&mut self) -> Result<Self::Route> {
    Ok(TestRoute(PhantomData))
  }

  fn init_api_service(&mut self) -> Result<Self::ApiService> {
    Ok(Api(HashMap::new(),Duration::from_millis(3000),0,PhantomData))
  }

  fn init_api_channel_in(&mut self) -> Result<Self::ApiServiceChannelIn> {
    Ok(LocalRcChannel)
  }
  fn init_api_spawner(&mut self) -> Result<Self::ApiServiceSpawn> {
    Ok(RestartOrError)
  }
  fn init_synch_listener_spawn(&mut self) -> Result<Self::SynchListenerSpawn> {
    Ok(NoSpawn)
  }
   fn init_synch_connect_spawn(&mut self) -> Result<Self::SynchConnectSpawn> {
    Ok(NoSpawn)
  }
  fn init_synch_connect_channel_in(&mut self) -> Result<Self::SynchConnectChannelIn> {
    Ok(NoChannel)
  }
}


/*mod test_dummy_all_block_thread {
  use super::*;
  use std::time::Duration;
  pub struct TestAllThConf1(pub TestAllThConf,pub Option<AsynchTransportTest>);
  impl MyDHTConf for TestAllThConf1 {

    const LOOP_NAME : &'static str = "Conf test spawner";
    const EVENTS_SIZE : usize = 1024;
    const SEND_NB_ITER : usize = 1;
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
    let conf1 = TestAllThConf1(TestAllThConf("peer1".to_string(), 0, true),tr.pop());
    let conf2 = TestAllThConf1(TestAllThConf("peer2".to_string(), 1, true),tr.pop());

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
