extern crate mydht_tcp_loop;
extern crate sized_windows_lim;
extern crate mydht_basetest;
use tunnel::info::error::{
  MultipleErrorInfo,
  MultipleErrorMode,
};
use tunnel::info::multi::{
  MultipleReplyMode,
};

use tunnel::{
  SymProvider,
};
use readwrite_comp::{
  ExtWrite,
  ExtRead,
  MultiRExt,
};
use std::io::{
  Write,
  Read,
  Result as IoResult,
};
use self::mydht_basetest::shadow::{
  ShadowTest,
  ShadowModeTest,
};
use mydht::transportif::{
  Transport,
};

use super::{
  SSWCache,
};
use self::sized_windows_lim::{
  SizedWindowsParams,
  SizedWindows,
};
use tunnel::full::{
  Full,
  FullW,
  GenTunnelTraits,
  TunnelCachedWriterExt,
  ErrorWriter,
};




use mydht::kvstoreif::{
  KVCache,
  KVStore,
};
use mydht::kvcache::{
  Cache,
};

use std::marker::PhantomData;
use self::mydht_tcp_loop::{
  Tcp,
};
use std::collections::HashMap;
use std::sync::Arc;
use mydht_basetest::node::Node;
use mydht::peer::Peer;
use mydht::keyval::{
  GettableAttachments,
  SettableAttachments,
  Attachment,
};
use std::thread;
use std::time::Duration;
use super::{
  MyDHTTunnelConf,
  MyDHTTunnelConfType,
  TunnelWriterReader,
  GlobalTunnelCommand,
  GlobalTunnelReply,
  LocalTunnelReply,
};
use mydht::MyDHTConf;
use mydht::utils::{
  Ref,
  SRef,
  SToRef,
  OptInto,
  OptFrom,
  ArcRef,
  new_oneresult,
  replace_wait_one_result,
  sa4,
  SerSocketAddr,
};
use mydht::dhtimpl::{
  SimpleRules,
  SimpleCache,
  DhtRules,
  DHTRULES_DEFAULT,
};
use mydht::dhtif::{
  Result,
  PeerMgmtMeths,
  KeyVal,
};
use mydht::service::{
  Service,
  SpawnerYield,
  SpawnSend,
};
use mydht::{
  PeerStatusListener,
  PeerStatusCommand,
  MCCommand,
  MCReply,
  Json,
  Route,
};

use mydht::api::{
  Api,
  ApiResult,
  ApiResultSend,
  ApiCommand,
  ApiQueriable,
  ApiQueryId,
  ApiRepliable,
};

use mydht::{
  GlobalCommand,
  GlobalReply,
  LocalReply,
  FWConf,
  PeerCacheEntry,
  AddressCacheEntry,
  ChallengeEntry,
  ReadReply,
  MainLoopCommand,
  PeerPriority,
};

use std::net::{
  Ipv4Addr,
};
#[derive(Serialize,Deserialize,Debug,Clone)]
#[serde(bound(deserialize = ""))]
/// test message is both testmessage, testcommand and testreply (very simple test
pub enum TestMessage {
  TouchQ(usize),
  TouchR(usize),
}

/// TODO check usage and use a simple touchr for api
pub struct TestReply(pub TestMessage);
impl GettableAttachments for TestMessage {
  fn get_nb_attachments(&self) -> usize {
    0
  }
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

impl SRef for TestMessage {
  type Send = Self;
  fn get_sendable(self) -> Self::Send {
    self
  }
}
impl SToRef<TestMessage> for TestMessage {
  fn to_ref(self) -> TestMessage {
    self
  }
}

/*
impl OptInto<TestMessage> for TestMessage {
  fn can_into(&self) -> bool {
    true
  }
  fn opt_into(self) -> Option<TestMessage> {
    Some(self)
  }
}
*/
impl ApiQueriable for TestMessage {
  #[inline]
  fn is_api_reply(&self) -> bool {
    if let &TestMessage::TouchQ(..) = self {
      true
    } else {
      false
    }
  }
  #[inline]
  fn set_api_reply(&mut self, aid : ApiQueryId) {
    match *self {
      TestMessage::TouchQ(ref mut qid) => *qid = aid.0,
      _ => (),
    }
  }
  #[inline]
  fn get_api_reply(&self) -> Option<ApiQueryId> {
    match *self {
      TestMessage::TouchQ(ref qid) =>  Some(ApiQueryId(*qid)),
      _ => None,
    }
  }
}
impl ApiQueriable for TestReply {
  #[inline]
  fn is_api_reply(&self) -> bool {
    match self.0 {
      TestMessage::TouchR(..) => true,
      _ => false,
    }
  }
  #[inline]
  fn get_api_reply(&self) -> Option<ApiQueryId> {
    match self.0 {
      TestMessage::TouchR(ref qid) =>  Some(ApiQueryId(*qid)),
      _ => None,
    }
  }
  #[inline]
  fn set_api_reply(&mut self, aid : ApiQueryId) {
    match self.0 {
      TestMessage::TouchR(ref mut qid) => *qid = aid.0,
      _ => (),
    }
  }

}

  /*  
/// TODO use a inner to local dest for inner service
impl<MC : MyDHTTunnelConf> OptInto<LocalReply<MyDHTTunnelConfType<MC>>> for TestReply<<MC as MyDHTConf>::PeerRef> {
  fn can_into(&self) -> bool {
    true
  }
  fn opt_into(self) -> Option<LocalReply<MyDHTTunnelConfType<MC>>> {
    let (dest, m) = match self {
      (Some(dest),ms,false) => (Some(vec![dest]),ms),
      (None,ms,false) => (None,ms),
      // warn need mapping to tunnile message
      (_,ms,true) => return Some(LocalReply::Api(ms)),
    };
    Some(LocalReply::Read(ReadReply::MainLoopCommand(MainLoopCommand::ForwardService(dest,None,FWConf {
          nb_for : 1,
          discover : false,
    }, m))))
  }
}
*/

impl OptFrom<TestMessage> for TestMessage {
  fn can_from(_ : &TestMessage) -> bool {
    true
  }
  fn opt_from(m : TestMessage) -> Option<Self> {
    Some(m)
  }
}
// TODO delete?
impl<MC : MyDHTTunnelConf> OptFrom<MCCommand<MyDHTTunnelConfType<MC>>> for TestMessage {
  fn can_from(m : &MCCommand<MyDHTTunnelConfType<MC>>) -> bool {
    match *m {
      MCCommand::Local(..) | MCCommand::Global(..) => true,
      MCCommand::PeerStore(..) | MCCommand::TryConnect(..) => false,
    }
  }
  fn opt_from(m : MCCommand<MyDHTTunnelConfType<MC>>) -> Option<Self> {
    match m {
      MCCommand::Local(..) => unimplemented!(), 
      MCCommand::Global(..) =>  unimplemented!(),
      MCCommand::PeerStore(..) | MCCommand::TryConnect(..) => None,
    }
  }
}
// TODO delete?
impl<MC : MyDHTTunnelConf> Into<MCCommand<MyDHTTunnelConfType<MC>>> for TestMessage {
  fn into(self) -> MCCommand<MyDHTTunnelConfType<MC>> {
    match self {
      TestMessage::TouchQ(aid) => unimplemented!(),
      TestMessage::TouchR(aid) => unimplemented!(),
    }
  }
}


impl<P> PeerStatusListener<P> for TestMessage {
  const DO_LISTEN : bool = false;
  #[inline]
  fn build_command(_ : PeerStatusCommand<P>) -> Self {
    unreachable!()
  }
}

/// inner mydht tunnel service store when called it will send to its dest
pub struct TestService<MC : MyDHTTunnelConf>(usize, <MC as MyDHTTunnelConf>::PeerRef);
/// peer name, listener port, is_multiplexed, node in kvstore, and dest for query
pub struct TunnelConf(pub String, pub SerSocketAddr, pub bool, pub Vec<Node>, pub Node);

impl Service for TestService<TunnelConf> {
  type CommandIn = GlobalCommand<<TunnelConf as MyDHTTunnelConf>::PeerRef,<TunnelConf as MyDHTTunnelConf>::InnerCommand>;
  type CommandOut = GlobalTunnelReply<TunnelConf>;
  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, _async_yield : &mut S) -> Result<Self::CommandOut> {
 
    match req {
      GlobalCommand::Distant(_, TestMessage::TouchQ(qid)) => {
        Ok(GlobalTunnelReply::TryReplyFromReader(
            TestMessage::TouchR(qid)))
      },
      GlobalCommand::Distant(_, TestMessage::TouchR(qid)) => {
        // api reply (last true)
        Ok(GlobalTunnelReply::Api(
          TestReply(TestMessage::TouchR(qid))))
      },
      GlobalCommand::Local(TestMessage::TouchQ(qid)) => {
        self.0 = qid;
        // proxy message
        Ok(GlobalTunnelReply::SendCommandTo(
          self.1.clone(),TestMessage::TouchQ(qid)))

      },
      GlobalCommand::Local(TestMessage::TouchR(qid)) => {
        unreachable!()
      },
    }
  }
}

impl MyDHTTunnelConf for TunnelConf {

  /// when testing value should be change as this is direct connect
  const INIT_ROUTE_LENGTH : usize = 0;
  /// when testing value should be change
  const INIT_ROUTE_BIAS : usize = 0;
  type PeerKey = <Self::Peer as KeyVal>::Key;
  type Peer = Node;
  type PeerRef = ArcRef<Node>;
  type InnerCommand = TestMessage;
  type InnerReply = TestReply;
  type InnerService = TestService<Self>;
  type InnerServiceProto = Self::PeerRef;
  type Transport = Tcp;
  type TransportAddress = SerSocketAddr;
  type MsgEnc = Json;
  type PeerMgmtMeths = TestingRules;
  type DHTRules = Arc<SimpleRules>;
  type ProtoMsg = TestMessage;
  type PeerCache = HashMap<<Self::Peer as KeyVal>::Key,PeerCacheEntry<Self::PeerRef>>;
  type AddressCache = HashMap<<Self::Peer as Peer>::Address,AddressCacheEntry>;
  type ChallengeCache = HashMap<Vec<u8>,ChallengeEntry<MyDHTTunnelConfType<Self>>>;
  type Route = TestRoute<MyDHTTunnelConfType<Self>>;
  type PeerKVStore = SimpleCache<Self::Peer,HashMap<<Self::Peer as KeyVal>::Key,Self::Peer>>;

  type LimiterW = SizedWindows<TestSizedWindows>;
  type LimiterR = SizedWindows<TestSizedWindows>;

  type SSW = SWrite;
  type SSR = SRead;
  type SP = SProv;

  type CacheSSW = HashMap<Vec<u8>,SSWCache<Self>>;
  type CacheSSR = HashMap<Vec<u8>,MultiRExt<Self::SSR>>;
  type CacheErW = HashMap<Vec<u8>,(ErrorWriter,<Self::Transport as Transport>::Address)>;
  type CacheErR = HashMap<Vec<u8>,Vec<MultipleErrorInfo>>;
// peer name, listener port, is_multiplexed, node in kvstore, and dest for query
//pub struct TunnelConf(pub String, pub SerSocketAddr, pub bool, pub Vec<Node>, pub Node);
  fn init_ref_peer(&mut self) -> Result<Self::PeerRef> {
    Ok(ArcRef::new(Node {
      nodeid: self.0.clone(),
      address : self.1.clone(),
    }))
  }

  fn init_inner_service_proto(&mut self) -> Result<Self::InnerServiceProto> {
    Ok(ArcRef::new(self.4.clone()))
  }
  fn init_inner_service(dest : Self::InnerServiceProto, _me : Self::PeerRef) -> Result<Self::InnerService> {
    Ok(TestService(0, dest.clone()))
  }

  fn init_peer_kvstore(&mut self) -> Result<Box<Fn() -> Result<Self::PeerKVStore> + Send>> {
    let dest = self.3.clone();
    Ok(Box::new(
      move || {
        let mut dest = dest.clone();
        let mut cache = SimpleCache::new(None);
        for n in dest.drain(..) {
          cache.add_val(n,None)
        }
        Ok(cache)
      }
    ))
  }

  fn init_transport(&mut self) -> Result<Self::Transport> {
    Ok(Tcp::new(&(self.1).0, None, self.2)?)
  }

  fn init_peermgmt_proto(&mut self) -> Result<Self::PeerMgmtMeths> {
    Ok(TestingRules)
  }

  fn init_dhtrules_proto(&mut self) -> Result<Self::DHTRules> {
    Ok(Arc::new(SimpleRules::new(DHTRULES_DEFAULT)))
  }
  fn init_enc_proto(&mut self) -> Result<Self::MsgEnc> {
    Ok(Json)
  }

  fn init_route(&mut self) -> Result<Self::Route> {
    Ok(TestRoute(PhantomData))
  }

  fn init_main_loop_peer_cache(&mut self) -> Result<Self::PeerCache> {
    Ok(HashMap::new())
  }
  fn init_main_loop_address_cache(&mut self) -> Result<Self::AddressCache> {
    Ok(HashMap::new())
  }
  fn init_main_loop_challenge_cache(&mut self) -> Result<Self::ChallengeCache> {
    Ok(HashMap::new())
  }
  fn init_cache_ssw(&mut self) -> Result<Self::CacheSSW> {
    Ok(HashMap::new())
  }
  fn init_cache_ssr(&mut self) -> Result<Self::CacheSSR> {
    Ok(HashMap::new())
  }
  fn init_cache_err(&mut self) -> Result<Self::CacheErR> {
    Ok(HashMap::new())
  }
  fn init_cache_erw(&mut self) -> Result<Self::CacheErW> {
    Ok(HashMap::new())
  }
  fn init_shadow_provider(&mut self) -> Result<Self::SP> {
    Ok(SProv(ShadowTest(0,0,ShadowModeTest::SimpleShift)))
  }
  fn init_limiter_w(&mut self) -> Result<Self::LimiterW> {
    Ok(SizedWindows::new(TestSizedWindows))
  }
  fn init_limiter_r(&mut self) -> Result<Self::LimiterR> {
    Ok(SizedWindows::new(TestSizedWindows))
  }
}


#[test]
fn test_ping_pong_no_hop() {
  test_ping_pong(2,45330)
}

#[test]
fn test_ping_pong_one_hop() {
  test_ping_pong(3,45333)
}
#[test]
fn test_ping_pong_mult_hop() {
//  test_ping_pong(8,45337)
  test_ping_pong(25,45337)
  // warning for testing with 128 or higher we may reach file limit on somelinux systems (1024 by default),
  // testing with 'ulimit -n 4096' succeed.
  //test_ping_pong(128,45337)
}


fn test_ping_pong(nb_peer : usize, start_port : usize) {
  let mode = MultipleReplyMode::Route;
  let err_mode = MultipleErrorMode::NoHandling;
  let mut peers : Vec<Node> = Vec::with_capacity(nb_peer);
  for i in 0..nb_peer {
    let addr = sa4(Ipv4Addr::new(127,0,0,1), (start_port + i) as u16);
    let peer = Node {nodeid: format!("peer {}",i), address : SerSocketAddr(addr)};
    peers.push(peer);
  }
  let mut sends : Vec<_> = peers.iter().enumerate().map(|(i,p)| {
    let mut stpeers = peers.clone();
    stpeers.remove(i);
    let conf = MyDHTTunnelConfType::new(
      TunnelConf(p.nodeid.clone(), p.address.clone(), true,stpeers,peers[peers.len()-1].clone()),
      mode.clone(),
      err_mode.clone(),
      // route len
      Some(nb_peer - 2),
      // route bias
      Some(0),
    ).unwrap();
    let send = conf.start_loop().unwrap().0;
    send
  }).collect();
  let o_res = new_oneresult((Vec::with_capacity(1),1,1));
  let touchq = ApiCommand::call_service_reply(GlobalTunnelCommand::Inner(
    TestMessage::TouchQ(0)
  ),o_res.clone());
  sends[0].send(touchq).unwrap();
  let o_res = replace_wait_one_result(&o_res,(Vec::new(),0,0)).unwrap();
  assert!(o_res.0.len() == 1);
  for v in o_res.0.iter() {
//    assert!(if let &ApiResult::ServiceReply(MCReply::Local(_)) = v {true} else {false});
    assert!(if let &ApiResult::ServiceReply(MCReply::Local(LocalTunnelReply::Api(TestReply(TestMessage::TouchR(_))))) = v {true} else {false});
  }

  // no service to check connection, currently only for testing and debugging : sleep
  thread::sleep(Duration::from_millis(3000));

}

#[derive(Clone)]
/// no rules (no auth in test so useless)
pub struct TestingRules;

impl<P : Peer> PeerMgmtMeths<P> for TestingRules {
  fn challenge (&self, _ : &P) -> Vec<u8> {
    unreachable!()
  }
  fn signmsg (&self, n : &P, chal : &[u8]) -> Vec<u8> {
    unreachable!()
  }
  fn checkmsg (&self, n : &P, chal : &[u8], sign : &[u8]) -> bool {
    unreachable!()
  }
  fn accept
  (&self, _ : &P)
  -> Option<PeerPriority> {
    unreachable!()
    //Some (PeerPriority::Normal)
  }
}

pub struct TestRoute<MC : MyDHTConf>(PhantomData<MC>);

impl Route<MyDHTTunnelConfType<TunnelConf>> for TestRoute<MyDHTTunnelConfType<TunnelConf>> {

  /// for testing we build tunnel with this route : simply get from cache plus could contain the
  /// dest (not an issue I think (self hop should be fine)).
  fn route(&mut self, 
           targetted_nb : usize, 
           c : MCCommand<MyDHTTunnelConfType<TunnelConf>>,
           _ : &mut <MyDHTTunnelConfType<TunnelConf> as MyDHTConf>::Slab, 
           cache : &mut <MyDHTTunnelConfType<TunnelConf> as MyDHTConf>::PeerCache) 
    -> Result<(MCCommand<MyDHTTunnelConfType<TunnelConf>>,Vec<usize>)> {
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



#[derive(Clone)]
pub struct TestSizedWindows;

impl SizedWindowsParams for TestSizedWindows {
//    const INIT_SIZE : usize = 45;
    const INIT_SIZE : usize = 15;
    const MAX_SIZE : usize = 2048;
    const GROWTH_RATIO : Option<(usize,usize)> = Some((3,2));
    const WRITE_SIZE : bool = true;
    const SECURE_PAD : bool = false;
}


#[derive(Clone)]
pub struct SProv (ShadowTest);
#[derive(Clone)]
pub struct SRead (ShadowTest);
#[derive(Clone)]
pub struct SWrite (ShadowTest);
impl ExtWrite for SWrite {
  #[inline]
  fn write_header<W : Write>(&mut self, w : &mut W) -> IoResult<()> {
    self.0.write_header(w)
  }
  #[inline]
  fn write_into<W : Write>(&mut self, w : &mut W, cont : &[u8]) -> IoResult<usize> {
    self.0.write_into(w,cont)
  }
  #[inline]
  fn flush_into<W : Write>(&mut self, w : &mut W) -> IoResult<()> {
    self.0.flush_into(w)
  }
  #[inline]
  fn write_end<W : Write>(&mut self, w : &mut W) -> IoResult<()> {
    self.0.write_end(w)
  }
}
impl ExtRead for SRead {
  fn read_header<R : Read>(&mut self, r : &mut R) -> IoResult<()> {
    self.0.read_header(r)
  }
  #[inline]
  fn read_from<R : Read>(&mut self, r : &mut R, buf : &mut[u8]) -> IoResult<usize> {
    self.0.read_from(r,buf)
  }
  #[inline]
  fn read_end<R : Read>(&mut self, r : &mut R) -> IoResult<()> {
    self.0.read_end(r)
  }
}

impl SymProvider<SWrite,SRead> for SProv {
  fn new_sym_key (&mut self) -> Vec<u8> {
    ShadowTest::shadow_simkey()
  }
  // TODO peerkey at 0??
  fn new_sym_writer (&mut self, v : Vec<u8>) -> SWrite {
    let mut st = self.0.clone();
    st.0 = 0;
    st.1 = v[0];
    SWrite(st)
  }
  // TODO peerkey at 0??
  fn new_sym_reader (&mut self, v : Vec<u8>) -> SRead {
    let mut st = self.0.clone();
    st.0 = 0;
    st.1 = v[0];
    SRead(st)
  }
}

