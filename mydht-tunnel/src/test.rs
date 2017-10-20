
extern crate mydht_tcp_loop;
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

/// if last bool is true it is api (should be better design as enum)
pub struct TestReply<PR>(pub Option<PR>,pub TestMessage,pub bool);
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


impl OptInto<TestMessage> for TestMessage {
  fn can_into(&self) -> bool {
    true
  }
  fn opt_into(self) -> Option<TestMessage> {
    Some(self)
  }
}

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
impl<PR> ApiRepliable for TestReply<PR> {
  #[inline]
  fn get_api_reply(&self) -> Option<ApiQueryId> {
    match self.1 {
      TestMessage::TouchQ(ref qid) =>  Some(ApiQueryId(*qid)),
      _ => None,
    }
  }
}
/*
/// TODO use a inner to global dest for inner service
impl<P : Peer,PR,GSC,GSR> OptInto<GlobalReply<P,PR,GSC,GSR>> for TestReply<PR> {
  fn can_into(&self) -> bool {
    true
  }
  fn opt_into(self) -> Option<GlobalReply<P,PR,GSC,GSR>> {
    let (dest, m) = match self {
      (Some(dest),ms,false) => (Some(vec![dest]),ms),
      (None,ms,false) => (None,ms),
      // warn need mapping to tunnile message
      (_,ms,true) => return Some(GlobalReply::Api(ms)),
    };
    Some(GlobalReply::Forward(dest,None,FWConf {
          nb_for : 1,
          discover : false,
    }, m))
 
  }

}*/
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
  type CommandOut = TestReply<<TunnelConf as MyDHTTunnelConf>::PeerRef>;
  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, _async_yield : &mut S) -> Result<Self::CommandOut> {
    match req {
      GlobalCommand(Some(_), TestMessage::TouchQ(qid)) => {
        Ok(TestReply(None,TestMessage::TouchR(qid),false))
      },
      GlobalCommand(Some(_), TestMessage::TouchR(qid)) => {
        // api reply (last true)
        Ok(TestReply(None,TestMessage::TouchR(qid),true))
      },
      GlobalCommand(None, TestMessage::TouchQ(qid)) => {
        self.0 = qid;
        // proxy message
        Ok(TestReply(Some(self.1.clone()),TestMessage::TouchQ(qid),false))

      },
      GlobalCommand(None, TestMessage::TouchR(qid)) => {
        unreachable!()
      },
    }
  }
}

impl MyDHTTunnelConf for TunnelConf {
  type PeerKey = <Self::Peer as KeyVal>::Key;
  type Peer = Node;
  type PeerRef = ArcRef<Node>;
  type InnerCommand = TestMessage;
  type InnerReply = TestReply<Self::PeerRef>;
  type InnerService = TestService<Self>;
  type Transport = Tcp;
  type MsgEnc = Json;
  type PeerMgmtMeths = TestingRules;
  type DHTRules = Arc<SimpleRules>;
  type ProtoMsg = TestMessage;
  type PeerCache = HashMap<<Self::Peer as KeyVal>::Key,PeerCacheEntry<Self::PeerRef>>;
  type ChallengeCache = HashMap<Vec<u8>,ChallengeEntry<MyDHTTunnelConfType<Self>>>;
  type Route = TestRoute<MyDHTTunnelConfType<Self>>;
  type PeerKVStore = SimpleCache<Self::Peer,HashMap<<Self::Peer as KeyVal>::Key,Self::Peer>>;

// peer name, listener port, is_multiplexed, node in kvstore, and dest for query
//pub struct TunnelConf(pub String, pub SerSocketAddr, pub bool, pub Vec<Node>, pub Node);
  fn init_ref_peer(&mut self) -> Result<Self::PeerRef> {
    Ok(ArcRef::new(Node {
      nodeid: self.0.clone(),
      address : self.1.clone(),
    }))
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
  fn init_main_loop_challenge_cache(&mut self) -> Result<Self::ChallengeCache> {
    Ok(HashMap::new())
  }

}



#[test]
fn test_ping_pong() {
  let start_port = 45330;
  let nb_hop = 2;
  let mut peers : Vec<Node> = Vec::with_capacity(nb_hop);
  for i in 0..nb_hop {
    let addr = sa4(Ipv4Addr::new(127,0,0,1), (start_port + i) as u16);
    let peer = Node {nodeid: format!("peer {}",i), address : SerSocketAddr(addr)};
    peers.push(peer);
  }
  let mut sends : Vec<_> = peers.iter().enumerate().map(|(i,p)| {
    let mut stpeers = peers.clone();
    stpeers.remove(i);
    let conf = MyDHTTunnelConfType(TunnelConf(p.nodeid.clone(), p.address.clone(), true,stpeers,peers[peers.len()-1].clone())); 
    let send = conf.start_loop().unwrap().0;
    send
  }).collect();
  let o_res = new_oneresult((Vec::with_capacity(1),1,1));
  let touchq = ApiCommand::call_service_reply(GlobalTunnelCommand::Inner(
    TestMessage::TouchQ(0)
  ),o_res.clone());
  sends[0].send(touchq).unwrap();
  let o_res = replace_wait_one_result(&o_res,(Vec::new(),0,999)).unwrap();
  assert!(o_res.0.len() == 1);
  for v in o_res.0.iter() {
    assert!(if let &ApiResult::ServiceReply(MCReply::Global(GlobalTunnelReply::Inner(TestReply(_,TestMessage::TouchR(_),_)))) = v {true} else {false});
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