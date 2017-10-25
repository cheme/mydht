//! Tests, unitary tests are more likely to be found in their related package, here is global
//! testing.
extern crate mydht_inefficientmap;
extern crate mydht_slab;

use std::borrow::Borrow;
use mydhtresult::Result;
use rules::{
  DHTRules,
};
use transport::Transport;
use procs::{
  PeerCacheEntry,
  AddressCacheEntry,
  ChallengeEntry,
};



use procs::deflocal::{
  LocalReply,
};
use utils::{
  OneResult,
  new_oneresult,
  clone_wait_one_result,
  Ref,
  ArcRef,
//  RcRef,
//  CloneRef,

};
use std::time::Instant;
use std::time::Duration;
use std::mem::replace;
use std::marker::PhantomData;
use std::collections::HashMap;
use procs::storeprop::{
  KVStoreService,
  KVStoreCommand,
  KVStoreReply,
  KVStoreProtoMsgWithPeer,
};
use query::simplecache::{
  SimpleCacheQuery,
  HashMapQuery,
};

use procs::{
  MCReply,
  PeerCacheRouteBase,
  RWSlabEntry,
};
use procs::api::{
  Api,
  ApiResult,
  ApiQueryId,
};

use procs::{
  ApiCommand,
};
use peer::{
  Peer,
};
use procs::api::{
  DHTIn,
};
use self::mydht_slab::slab::{
  Slab,
};

use std::thread;
use std::hash::Hash;
use simplecache::SimpleCache;
use self::mydht_inefficientmap::inefficientmap::InefficientmapBase2;
use rules::simplerules::{
  DHTRULES_DEFAULT,
  SimpleRules,
  DhtRules,
};
use peer::test::{
  TestingRules,
  PeerTest,
  ShadowModeTest,
};
use keyval::{
  KeyVal,
};
use rand::{thread_rng,Rng};
use query::{QueryConf,QueryMode};
use query::{
  //Query,
  QueryPriority,
};
use procs::noservice::{
  NoCommandReply,
};
use service::{
  NoService,
  NoSpawn,
  //Service,
  //MioChannel,
  //SpawnChannel,
  MpscChannel,
//  MpscChannelRef,
  NoChannel,
  //NoRecv,
  //LocalRcChannel,
  //SpawnerYield,
  SpawnSend,
 // LocalRc,
 // MpscSender,
  NoSend,

  //Spawner,
  //Blocker,
  //RestartOrError,
  //Coroutine,
  //RestartSameThread,
 // ThreadBlock,
  ThreadPark,
 // ThreadParkRef,

  //CpuPool,
  //CpuPoolFuture,
};


use kvstore::{
  KVStore,
};
use msgenc::json::Json;
use peer::PeerMgmtMeths;
use msgenc::MsgEnc;
use num::traits::ToPrimitive;
use procs::ClientMode;
use procs::{
  MyDHTConf,
};
#[cfg(feature="with-extra-test")]
mod diag_tcp;
#[cfg(feature="with-extra-test")]
mod diag_udp;

mod mainloop;
pub use mydht_basetest::local_transport::*;
pub use mydht_basetest::transport::*;

#[cfg(feature="with-extra-test")]
#[test]
fn simu_aproxy_peer_discovery () {
  let nbpeer = 5;
  let mut rules = DHTRULES_DEFAULT.clone();
  rules.nbhopfact = nbpeer - 1;
  let rcs = runningcontext1(nbpeer.to_usize().unwrap(),rules);
  let qconf = QueryConf {
    mode : QueryMode::AProxy,
//    chunk : QueryChunk::None,
    hop_hist : Some((4,false)),
  };
  peerconnect_scenario2 (&qconf, 2, rcs)
}

#[cfg(feature="with-extra-test")]
#[test]
fn simu_amix_proxy_peer_discovery () {
  let nbpeer = 5;
  let mut rules = DHTRULES_DEFAULT.clone();
  rules.nbhopfact = nbpeer - 1;
  let rcs = runningcontext1(nbpeer.to_usize().unwrap(),rules);
  let qconf = QueryConf {
    mode : QueryMode::AMix(9),
//    chunk : QueryChunk::None,
    hop_hist : None,
  };
  peerconnect_scenario2(&qconf, 2, rcs)
}

#[cfg(feature="with-extra-test")]
#[test]
fn simu_asynch_peer_discovery () {
  let nbpeer = 5;
  let mut rules = DHTRULES_DEFAULT.clone();
  rules.nbhopfact = nbpeer - 1;
  let rcs = runningcontext1(nbpeer.to_usize().unwrap(),rules);
  let qconf = QueryConf {
    mode : QueryMode::Asynch,
//    chunk : QueryChunk::None,
    hop_hist : Some((3,true)),
  };
  peerconnect_scenario2(&qconf, 2, rcs)
}


/*struct RunningTypesImpl<
  A : Address,
  P : Peer<Address = A>,
  V : KeyVal,
  M : PeerMgmtMeths<P, V>, 
  R : DHTRules,
  E : MsgEnc, 
  T : Transport<Address = A>>
*/ 

/// init with all peers as others : filtering is done in scenario (random removal and remove self)
fn runningcontext1 (nbpeer : usize, dhtrules : DhtRules) -> Vec<TestConf<PeerTest, TransportTest, Json, TestingRules,SimpleRules>> {
  // non duplex
  let mut transports = TransportTest::create_transport(nbpeer,true,true);
  transports.reverse();
  let mut rcs = Vec::with_capacity(nbpeer);
  let mut nodes = Vec::with_capacity(nbpeer);
  for i in 0 .. nbpeer {
    let peer = PeerTest {
         nodeid: "dummyID".to_string() + (&i.to_string()[..]), 
         address : LocalAdd(i),
         keyshift : i as u8 + 1,
         modeshauth : ShadowModeTest::NoShadow,
         modeshmsg : ShadowModeTest::SimpleShift,
    };

    nodes.push(peer.clone());
    let context = confinitpeers1(
       peer,
       Vec::with_capacity(nbpeer),
       transports.pop().unwrap(),
       TestingRules::new_no_delay(),
       dhtrules.clone(),
    );
    rcs.push(context);
  };
  for r in rcs.iter_mut() {
    r.others = Some(nodes.clone());
  }
  rcs
}

 
/// ration is 1 for all 2 for half and so on.
fn peerconnect_scenario2 (queryconf : &QueryConf, knownratio : usize, contexts : Vec<TestConf<PeerTest, TransportTest, Json, TestingRules,SimpleRules>>) {
  let nodes : Vec<(PeerTest,SimpleRules)> = contexts.iter().map(|c|(c.me.clone(),c.rules.clone())).collect();
  let mut rng = thread_rng();
  let mut ix_context = 0;
  let mut me = None;
  let mut procs : Vec<DHTIn<TestConf<PeerTest,TransportTest,Json,TestingRules,SimpleRules>>> = contexts.into_iter().map(|mut context|{
    info!("node : {:?}", context.me);

    if me == None {
      // to be use in queries asynch
      me = Some(context.me.clone());
    }
//        rng.shuffle(noderng);
    let others = replace(&mut context.others, None).unwrap();
    let mut bpeers : Vec<PeerTest>;
    // default route is used for querying : next peer from kvstore use a 2/3 ratio cf kvcache
    // next_random_values which involve that first peer need at leet 2 peers so we filter until at
    // least 2 : done only for first peer (proxy of query make discovery likely for others
    // This implementation does not make a lot of sense (was seemingly to make nb of proxy peer
    // more random??) and should change
    if ix_context == 0 {
      while {
        let ot2 = others.clone();
        bpeers = ot2.into_iter().filter(|i| (*i != context.me && rng.gen_range(0,knownratio) == 0) ).collect();
        bpeers.len() < 1
      } {}
    } else {
        bpeers = others.into_iter().filter(|i| (*i != context.me && rng.gen_range(0,knownratio) == 0) ).collect();
    }
    ix_context += 1;
    context.others = Some(bpeers);

//        let mut noderng : &mut [PeerTest] = bpeers.as_slice();
    let (sendcommand,_) = context.start_loop().unwrap();
    sendcommand
  }).collect();
  // find all node from the first node first node
  let ref mut fprocs = procs[0];

  let mut itern = nodes.iter();
  itern.next();
  for &(ref n,ref rule) in itern {
    // local find 
    let nb_res = 1;
    let o_res = new_oneresult((Vec::with_capacity(nb_res),nb_res,nb_res));
    // prio is same as nbhop with simple rules
    let prio = 1;
    let nb_hop = rule.nbhop(prio);
    let nb_for = rule.nbquery(prio);
    let qm = queryconf.query_message(me.as_ref().unwrap(), nb_res, nb_hop, nb_for, prio);
    let peer_q = ApiCommand::call_peer_reply(KVStoreCommand::Find(qm,n.get_key(),None),o_res.clone());
    fprocs.send(peer_q).unwrap();
    let o_res = clone_wait_one_result(&o_res,None).unwrap();
    assert!(o_res.0.len() == 1, "Peer not found {:?}", n);
  }

  /* TODO uncomment when call_shutdown reply is implemented
  for p in procs.iter_mut(){
    let o_res = new_oneresult((Vec::with_capacity(1),1,1));
    let shutc = ApiCommand::call_shutdown_reply(o_res);
    let o_res = clone_wait_one_result(&o_res,None).unwrap();
    assert!(o_res.0.len() == 1, "Fail shutdown");
  }
  */


}

#[cfg(test)]
static FEWTESTMODE : [QueryMode; 2] = [
                     QueryMode::Asynch,
                     QueryMode::AProxy,
                   ];


#[cfg(test)]
static ALLTESTMODE : [QueryMode; 6] = [
                     QueryMode::Asynch,
                     QueryMode::AProxy,
                     QueryMode::AMix(0),
                     QueryMode::AMix(1),
                     QueryMode::AMix(2),
                     QueryMode::AMix(3),
                   ];

#[cfg(feature="with-extra-test")]
#[test]
fn simpeer2hopget () {
    let n = 4;

    let map : &[&[usize]] = &[&[2],&[3],&[],&[3]];
    for m in ALLTESTMODE.iter() {
      let mut rules = DHTRULES_DEFAULT.clone();
      rules.nbhopfact = 3;
      let peers = initpeers_test2(n, map, TestingRules::new_no_delay(), rules.clone(), None);
      //let peers = initpeers_test2(n, map, TestingRules::new_no_delay(), rules.clone(), DEF_SIM);
      finddistantpeer2(peers,n,(*m).clone(),1,map,true, SimpleRules::new(rules));
    }
}

fn finddistantpeer2<P : Peer, T : Transport<Address = <P as Peer>::Address>,E : MsgEnc<P,KVStoreProtoMsgWithPeer<P,ArcRef<P>,P,ArcRef<P>>> + Clone>
   (mut peers : Vec<(P, DHTIn<TestConf<P,T,E,TestingRules,SimpleRules>>)>, nbpeer : usize, qm : QueryMode, prio : QueryPriority, _map : &[&[usize]], find : bool, rules : SimpleRules)
  where  <P as KeyVal>::Key : Hash,
         <P as Peer>::Address : Hash,
{
    let queryconf = QueryConf {
      mode : qm.clone(), 
//      chunk : QueryChunk::None, 
      hop_hist : Some((3,true))
    }; // note that we only unloop to 3 hop 

    let dest = peers.get(nbpeer -1).unwrap().0.clone();
    let nb_res = 1;
    let o_res = new_oneresult((Vec::with_capacity(nb_res),nb_res,nb_res));
    let nb_hop = rules.nbhop(prio);
    let nb_for = rules.nbquery(prio);
    let qm = queryconf.query_message(&peers.get(0).unwrap().0, nb_res, nb_hop, nb_for, prio);
    let peer_q = ApiCommand::call_peer_reply(KVStoreCommand::Find(qm.clone(),dest.get_key(),None),o_res.clone());
    peers.get_mut(0).unwrap().1.send(peer_q).unwrap();
    let mut o_res = clone_wait_one_result(&o_res,None).unwrap();
    assert!(o_res.0.len() == 1, "Peer not found {:?}", dest.get_key_ref());
    let v = o_res.0.pop().unwrap();
    let result : Option<ArcRef<P>> = if let ApiResult::ServiceReply(MCReply::PeerStore(KVStoreReply::FoundApi(ores,_))) = v {
      ores
    } else if let ApiResult::ServiceReply(MCReply::PeerStore(KVStoreReply::FoundApiMult(mut vres,_))) = v {
      vres.pop()
    } else {
      None
    };
    let matched = result.map(|p|{
      let pt : &P = p.borrow();
      *pt == dest
    }).unwrap_or(false);

    if find {
      assert!(matched, "Peer not found {:?} , {:?}", dest, qm);
    } else {
      assert!(!matched, "Peer found {:?} , {:?}", dest, qm);
    }
}


#[test]
fn testpeer2hopget (){
  let n = 4;
  let map : &[&[usize]] = &[&[2],&[3],&[4],&[]];
  for m in ALLTESTMODE.iter() {
    let mut rules = DHTRULES_DEFAULT.clone();
    rules.nbhopfact = 3;
    let peers = initpeers_test2(n, map, TestingRules::new_no_delay(), rules.clone(), None);
    finddistantpeer2(peers,n,(*m).clone(),1,map,true, SimpleRules::new(rules)); 
  }
}

/* TODO adapt when heavy accept willbe  implemented back
#[test]
fn testpeer2hopgetheavy () {
  let n = 4;
  let map : &[&[usize]] = &[&[2],&[3],&[4],&[]];
  for m in FEWTESTMODE.iter() {
    let mut rules = DHTRULES_DEFAULT.clone();
    rules.nbhopfact = 3;
    rules.heavyaccept = true;
    let peers = initpeers_test(n, map, TestingRules::new_small_delay_heavy_accept(Some(100)), rules, None);
    finddistantpeer(peers,n,(*m).clone(),1,map,true); 
  }
}
#[test]
fn testpeer2hopgetheavylocal () {
  let n = 4;
  let map : &[&[usize]] = &[&[2],&[3],&[4],&[]];
  for m in FEWTESTMODE.iter() {
    let mut rules = DHTRULES_DEFAULT.clone();
    rules.clientmode = ClientMode::Local(false);
    rules.nbhopfact = 3;
    rules.heavyaccept = true;
    let peers = initpeers_test(n, map, TestingRules::new_small_delay_heavy_accept(Some(100)), rules, None);
    finddistantpeer(peers,n,(*m).clone(),1,map,true);
  }
}
#[test]
fn testpeer2hopgetheavylocalspawn () {
  let n = 4;
  let map : &[&[usize]] = &[&[2],&[3],&[4],&[]];
  for m in FEWTESTMODE.iter() {
    let mut rules = DHTRULES_DEFAULT.clone();
    rules.clientmode = ClientMode::Local(true);
    rules.nbhopfact = 3;
    rules.heavyaccept = true;
    let peers = initpeers_test(n, map, TestingRules::new_small_delay_heavy_accept(Some(100)), rules, None);
    finddistantpeer(peers,n,(*m).clone(),1,map,true);
  }
}*/

/// TODO rename no more pool TODO alternate conf for ThreadPool usage??
#[test]
fn testpeer2hopgetmultpool () {
  let n = 8;
  let map : &[&[usize]] = &[&[5,6,2,7,8],&[3,5,6,7],&[5,6,7,8,4],&[],&[],&[],&[],&[]];
  for m in FEWTESTMODE.iter() {
    let mut rules = DHTRULES_DEFAULT.clone();
    rules.clientmode = ClientMode::ThreadPool(2);
    rules.nbhopfact = 3;
    rules.nbqueryfact = 5.0;
    let peers = initpeers_test2(n, map, TestingRules::new_no_delay(), rules.clone(), None);
    finddistantpeer2(peers,n,(*m).clone(),1,map,true,SimpleRules::new(rules));
  }
}
/* TODO restore with alternate service config
#[test]
fn testpeer2hopgetmultmax () {
  let n = 8;
  let map : &[&[usize]] = &[&[5,6,2,7,8],&[3,5,6,7],&[5,6,7,8,4],&[],&[],&[],&[],&[]];
  for m in FEWTESTMODE.iter() {
    let mut rules = DHTRULES_DEFAULT.clone();
    rules.clientmode = ClientMode::ThreadedMax(2);
    rules.nbhopfact = 3;
    rules.nbqueryfact = 5.0;
    let peers = initpeers_test2(n, map, TestingRules::new_no_delay(), rules, None);
    finddistantpeer2(peers,n,(*m).clone(),1,map,true);
  }
}*/




/* TODO uncomment when heavy reimplemented
#[cfg(feature="with-extra-test")]
#[test]
fn simpeer2hopgetheavy (){
  let n = 4;
  let map : &[&[usize]] = &[&[2],&[3],&[],&[3]];
  for m in FEWTESTMODE.iter() {
    let mut rules = DHTRULES_DEFAULT.clone();
    rules.nbhopfact = 3;
    rules.heavyaccept = true;
    let peers = initpeers_test(n, map, TestingRules::new_small_delay_heavy_accept(None), rules, DEF_SIM);
    // delay of one sec per accept : we need lot of time
    thread::sleep_ms(10000); // local get easily stuck
    finddistantpeer(peers,n,(*m).clone(),1,map,true); 
  }
}*/



#[cfg(feature="with-extra-test")]
#[test]
fn simpeermultipeersnoresult (){
  let n = 6;
  let map : &[&[usize]] = &[&[2,3,4],&[3,5],&[1],&[4],&[1],&[]];
  for m in ALLTESTMODE.iter() {
    let mut rules = DHTRULES_DEFAULT.clone();
    rules.nbhopfact = 3;
    // for asynch we need only one nbquer (expected to many none otherwhise)
    if let &QueryMode::Asynch = m {
      rules.nbqueryfact = 0.0; // nb query is 1 + prio * nbqfact
    };
    let peers = initpeers_test2(n, map, TestingRules::new_no_delay(), rules.clone(), DEF_SIM);
    finddistantpeer2(peers,n,(*m).clone(),1,map,false,SimpleRules::new(rules));
  }
}


#[test]
fn testpeer4hopget (){
    let n = 6;
    let map : &[&[usize]] = &[&[2],&[3],&[4],&[5],&[6],&[]];
    // prio 2 with rules multiplying by 3 give 6 hops
    for m in ALLTESTMODE.iter() {
      let mut rules = DHTRULES_DEFAULT.clone();
      rules.nbhopfact = 3;
      let peers = initpeers_test2(n, map, TestingRules::new_no_delay(), rules.clone(), None);
      finddistantpeer2(peers,n,(*m).clone(),2,map,true,SimpleRules::new(rules.clone()));
    };
   
    // prio 1 max nb hop is 3
    for m in ALLTESTMODE.iter() {
      let mut rules = DHTRULES_DEFAULT.clone();
      rules.nbhopfact = 3;
      // for asynch we need only one nbquer (expected to many none otherwhise)
      if let &QueryMode::Asynch = m {
        rules.nbqueryfact = 0.0; // nb query is 1 + prio * nbqfact
      };
      let peers = initpeers_test2(n, map, TestingRules::new_no_delay(), rules.clone(), None);
      finddistantpeer2(peers,n,(*m).clone(),1,map,false,SimpleRules::new(rules.clone()));
    };
}
/* TODO when heavy or not : very redundant
#[test]
fn testpeer4hopgetheavy (){
    let n = 6;
    let map : &[&[usize]] = &[&[2],&[3],&[4],&[5],&[6],&[]];
    // prio 2 with rules multiplying by 3 give 6 hops
    for m in ALLTESTMODE.iter() {
      let mut rules = DHTRULES_DEFAULT.clone();
      rules.nbhopfact = 3;
      rules.heavyaccept = true;
      let peers = initpeers_test(n, map, TestingRules::new_no_delay(), rules, None);
      finddistantpeer(peers,n,(*m).clone(),2,map,true);
    };
   
    // prio 1 max nb hop is 3
    for m in ALLTESTMODE.iter() {
      let mut rules = DHTRULES_DEFAULT.clone();
      rules.nbhopfact = 3;
      rules.heavyaccept = true;
      // for asynch we need only one nbquer (expected to many none otherwhise)
      if let &QueryMode::Asynch = m {
        rules.nbqueryfact = 0.0; // nb query is 1 + prio * nbqfact
      };
      let peers = initpeers_test(n, map, TestingRules::new_no_delay(), rules, None);
      finddistantpeer(peers,n,(*m).clone(),1,map,false);
    };
}*/



#[test]
fn simloopget (){ // TODO this only test loop over our node TODO circuit loop test
    let n = 4;
    // closest used are two first nodes (first being ourselves
    let map : &[&[usize]] = &[&[1,2],&[2,3],&[3,4],&[4]];
    //let map : &[&[usize]] = &[&[1,2],&[2,1,3],&[3,4],&[4]];
    for m in ALLTESTMODE.iter() {
      let mut rules = DHTRULES_DEFAULT.clone();
      rules.nbhopfact = 3;
      let peers = initpeers_test2(n, map, TestingRules::new_no_delay(), rules.clone(), DEF_SIM);
      finddistantpeer2(peers,n,(*m).clone(),1,map,true,SimpleRules::new(rules));
    };
}

struct TestConf<P,T,ENC,PM,DR> {
  pub me : P,
  pub others : Option<Vec<P>>,
  // transport in conf is bad, but flexible (otherwhise we could not be generic as we would need
  // transport initialisation parameter in struct : not only address for transport test).
  // Furthermore it makes the conf usable only once.
  pub transport : Option<T>,
  pub msg_enc : ENC,
  pub peer_mgmt : PM,
  pub rules : DR,
  pub do_peer_query_forward_with_discover : bool,
}

impl<
  P : Peer,
  T : Transport<Address = <P as Peer>::Address>,
  ENC : MsgEnc<P, KVStoreProtoMsgWithPeer<P,ArcRef<P>,P,ArcRef<P>>>,
  PM : PeerMgmtMeths<P>,
  DR : DHTRules + Clone,
  > MyDHTConf for TestConf<P,T,ENC,PM,DR> 
where <P as KeyVal>::Key : Hash,
      <P as Peer>::Address : Hash,

{
  const SEND_NB_ITER : usize = 10;

  type MainloopSpawn = ThreadPark;
  type MainLoopChannelIn = MpscChannel;
  type MainLoopChannelOut = MpscChannel;

  type Transport = T;
  type MsgEnc = ENC;
  type Peer = P;
  type PeerRef = ArcRef<P>;
  type PeerMgmtMeths = PM;
  type DHTRules = DR;
  type Slab = Slab<RWSlabEntry<Self>>;

  type PeerCache = InefficientmapBase2<Self::Peer, Self::PeerRef, PeerCacheEntry<Self::PeerRef>,
    HashMap<<Self::Peer as KeyVal>::Key,PeerCacheEntry<Self::PeerRef>>>;
  type AddressCache = HashMap<<Self::Transport as Transport>::Address,AddressCacheEntry>;
  type ChallengeCache = HashMap<Vec<u8>,ChallengeEntry<Self>>;
  type PeerMgmtChannelIn = MpscChannel;
  type ReadChannelIn = MpscChannel;
  type ReadSpawn = ThreadPark;
  // Placeholder
  type WriteDest = NoSend;
  type WriteChannelIn = MpscChannel;
  type WriteSpawn = ThreadPark;
  type Route = PeerCacheRouteBase;

  // keep val of global service to peer
  type ProtoMsg = KVStoreProtoMsgWithPeer<Self::Peer,Self::PeerRef,Self::Peer,Self::PeerRef>;
  //PMES : Into<MCCommand<TestConf<P,T,ENC,PM,DR>>> + SettableAttachments + GettableAttachments + OptFrom<MCCommand<TestConf<P,T,ENC,PM,DR>>>,


  nolocal!();

  type GlobalServiceCommand = KVStoreCommand<Self::Peer,Self::PeerRef,Self::Peer,Self::PeerRef>;
  type GlobalServiceReply = KVStoreReply<Self::PeerRef>;
  /// Same as internal peerstore
  type GlobalService = KVStoreService<Self::Peer,Self::PeerRef,Self::Peer,Self::PeerRef,Self::PeerKVStore,Self::DHTRules,Self::PeerStoreQueryCache>;
  type GlobalServiceSpawn = ThreadPark;
  type GlobalServiceChannelIn = MpscChannel;

  type ApiReturn = OneResult<(Vec<ApiResult<Self>>,usize,usize)>;
  type ApiService = Api<Self,HashMap<ApiQueryId,(OneResult<(Vec<ApiResult<Self>>,usize,usize)>,Instant)>>;
  type ApiServiceSpawn = ThreadPark;
  type ApiServiceChannelIn = MpscChannel;

  type PeerStoreQueryCache = SimpleCacheQuery<Self::Peer,Self::PeerRef,Self::PeerRef,HashMapQuery<Self::Peer,Self::PeerRef,Self::PeerRef>>;
  type PeerKVStore = SimpleCache<Self::Peer,HashMap<<Self::Peer as KeyVal>::Key,Self::Peer>>;
  type PeerStoreServiceSpawn = ThreadPark;
  type PeerStoreServiceChannelIn = MpscChannel;
 
  type SynchListenerSpawn = ThreadPark;

  const NB_SYNCH_CONNECT : usize = 3;
  type SynchConnectChannelIn = MpscChannel;
  type SynchConnectSpawn = ThreadPark;


  fn init_peer_kvstore(&mut self) -> Result<Box<Fn() -> Result<Self::PeerKVStore> + Send>> {
    let others = self.others.clone().unwrap();
    Ok(Box::new(
      move ||{
        let others = others.clone();
        let mut sc = SimpleCache::new(None);
        debug!("init kvstore with nb val {}",others.len());
        for o in others.into_iter() {
          sc.add_val(o,None);
        }

        Ok(sc)
      }
    ))
  }
  fn do_peer_query_forward_with_discover(&self) -> bool {
    self.do_peer_query_forward_with_discover
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
    Ok(ArcRef::new(self.me.clone()))
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
    Ok(InefficientmapBase2::new(HashMap::new()))
  }
  fn init_main_loop_address_cache(&mut self) -> Result<Self::AddressCache> {
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
    Ok(self.msg_enc.clone())
  }

  fn init_transport(&mut self) -> Result<Self::Transport> {
    Ok(replace(&mut self.transport,None).unwrap())
  }
  fn init_peermgmt_proto(&mut self) -> Result<Self::PeerMgmtMeths> {
    Ok(self.peer_mgmt.clone())
  }
  fn init_dhtrules_proto(&mut self) -> Result<Self::DHTRules> {
    Ok(self.rules.clone())
  }

  fn init_global_service(&mut self) -> Result<Self::GlobalService> {
    Ok(KVStoreService {
      // second ref create here due to P genericity (P is in conf : RefPeer should be in conf : but
      // for testing purpose we do it this way)
      me : self.init_ref_peer()?,
      init_store : self.init_peer_kvstore()?,
      init_cache : self.init_peer_kvstore_query_cache()?,
      store : None,
      dht_rules : self.init_dhtrules_proto()?,
      query_cache : None,
      discover : self.do_peer_query_forward_with_discover(),
      _ph : PhantomData,
    })
  }

  fn init_global_channel_in(&mut self) -> Result<Self::GlobalServiceChannelIn> {
    Ok(MpscChannel)
  }

  fn init_route(&mut self) -> Result<Self::Route> {
    Ok(PeerCacheRouteBase)
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
    Ok(ThreadPark)
  }
  fn init_synch_connect_channel_in(&mut self) -> Result<Self::SynchConnectChannelIn> {
    Ok(MpscChannel)
  }


}
/*     
 *     Arc::new(peer),
       TestingRules::new_no_delay(),
       SimpleRules::new(dhtrules.clone()),
       Json,
       transports.pop().unwrap(),
*/
// SimpleRules missing only for RunningType1
fn confinitpeers1(me : PeerTest, others : Vec<PeerTest>, transport : TransportTest, meths : TestingRules, dhtrules : DhtRules) -> TestConf<PeerTest, TransportTest, Json, TestingRules,SimpleRules> {
  TestConf {
    me : me,
    others : Some(others),
    transport : Some(transport),
    msg_enc : Json,
    peer_mgmt : meths,
    rules : SimpleRules::new(dhtrules),
    do_peer_query_forward_with_discover : true,
  }
}
//struct TestConf<P,T,ENC,PM,DR> {
//
/// Sim is not to be used in similar case as before : there is no guaranties all peers will be
/// queried : in fact with  hashmap kvstore default implementation only a ratio of 2/3 peer will be
/// queried by call to subset on peer connect discovery thus it is bad for test with single route.
fn initpeers2<P : Peer, T : Transport<Address = <P as Peer>::Address>,E : MsgEnc<P,KVStoreProtoMsgWithPeer<P,ArcRef<P>,P,ArcRef<P>>> + Clone> (nodes : Vec<P>, transports : Vec<T>, map : &[&[usize]], meths : TestingRules, rules : DhtRules, enc : E, sim : Option<u64>) 
  -> Vec<(P, DHTIn< TestConf<P,T,E,TestingRules,SimpleRules>  >)> 
  where  <P as KeyVal>::Key : Hash,
         <P as Peer>::Address : Hash,
  {
  let mut i = 0;// TODO redesign with zip of map and nodes iter
  let mut result : Vec<(P, DHTIn<TestConf<P,T,E,TestingRules,SimpleRules>>, Vec<P>)> = transports.into_iter().map(|t|{
    let n = nodes.get(i).unwrap();
    info!("node : {:?}", n);
    println!("{:?}",map[i]);
    let bpeers : Vec<P> = map[i].iter().map(|j| nodes.get(*j-1).unwrap().clone()).collect();
    i += 1;
    let test_conf = TestConf {
      me : n.clone(),
      others : Some(bpeers.clone()),
      transport : Some(t), 
      msg_enc : enc.clone(),
      peer_mgmt : meths.clone(),
      rules : SimpleRules::new(rules.clone()),
      do_peer_query_forward_with_discover : false,
    };

    let (sendcommand,_) = test_conf.start_loop().unwrap();
    (n.clone(),sendcommand,bpeers)
   }).collect();
   if sim.is_some() {
     // all has started
     for n in result.iter_mut(){
       thread::sleep(Duration::from_millis(100)); // local get easily stuck
       let refresh_command = ApiCommand::refresh_peer(10000); // Warn hard coded value.
       n.1.send(refresh_command).unwrap();
     };
     // ping established
     thread::sleep(Duration::from_millis(sim.unwrap()));
   } else {
     //establish connection by peerping of bpeers and wait result : no need to sleep
     for n in result.iter_mut(){
       for p in n.2.iter(){
         let o_res = new_oneresult((Vec::with_capacity(1),1,1));
         // TODO wait for reply
         let connect_command = ApiCommand::try_connect_reply(p.get_address().clone(),o_res.clone());
         n.1.send(connect_command).unwrap();
         let o_res = clone_wait_one_result(&o_res,None).unwrap();
         assert!(o_res.0.len() == 1);
         for v in o_res.0.iter() {
           assert!(if let &ApiResult::ServiceReply(MCReply::Done(_)) = v {true} else {false});
         }
       }
     };
   
   };
   result.into_iter().map(|n|(n.0,n.1)).collect()
}

// local transport usage is faster than actual transports
// Yet default to one second
static DEF_SIM : Option<u64> = Some(2000);

fn initpeers_test2 (nbpeer : usize, map : &[&[usize]], meths : TestingRules, rules : DhtRules, sim : Option<u64>) -> Vec<(PeerTest, DHTIn<TestConf<PeerTest,TransportTest,Json,TestingRules,SimpleRules>>)> {
  let transports = TransportTest::create_transport(nbpeer,true,true);
  let mut nodes = Vec::new();
  for i in 0 .. nbpeer {
    let peer = PeerTest {
         nodeid: "dummyID".to_string() + (&i.to_string()[..]), 
         address : LocalAdd(i),
         keyshift : i as u8 + 1,

         modeshauth : ShadowModeTest::NoShadow,
         modeshmsg : ShadowModeTest::SimpleShift,
         //modeshmsg : ShadowModeTest::SimpleShift,
    };
    nodes.push(peer);
  };
  initpeers2(nodes, transports, map, meths, rules, Json, sim)
}



#[cfg(feature="with-extra-test")]
#[test]
fn simpeer2hopfindval () {
    let nbpeer = 4;
    let val = PeerTest {
         nodeid: "to_find".to_string(),
         address : LocalAdd(999),
         keyshift : 60,

         modeshauth : ShadowModeTest::NoShadow,
         modeshmsg : ShadowModeTest::SimpleShift,
    };

    let mut rules = DHTRULES_DEFAULT.clone();
    rules.nbhopfact = 3;
    let srules = SimpleRules::new(rules.clone());
     
    let map : &[&[usize]] = &[&[],&[1,3],&[],&[3]];

    let prio = 1;
    let mut peers = initpeers_test2(nbpeer, map, TestingRules::new_no_delay(), rules, None);
    let dest = nbpeer - 1;
    for conf in ALLTESTMODE.iter(){
      let queryconf = QueryConf {
        mode : conf.clone(), 
  //      chunk : QueryChunk::None, 
        hop_hist : Some((7,false))
      };
      let nb_res = 1;
      let o_res = new_oneresult((Vec::with_capacity(nb_res),nb_res,nb_res));
      let store_q = ApiCommand::call_service_reply(KVStoreCommand::StoreLocally(ArcRef::new(val.clone()),1,None),o_res.clone());
      peers.get_mut(dest).unwrap().1.send(store_q).unwrap();
      let mut o_res = clone_wait_one_result(&o_res,None).unwrap();
      assert!(o_res.0.len() == 1, "No store rep ");
      let v = o_res.0.pop().unwrap();
      assert!(if let ApiResult::ServiceReply(MCReply::Global(KVStoreReply::Done(..))) = v { true } else { false });
      //assert!(dest.store_val(val.clone(), &queryconf, prio, StoragePriority::Local));

      let o_res = new_oneresult((Vec::with_capacity(nb_res),nb_res,nb_res));
      let nb_hop = srules.nbhop(prio);
      let nb_for = srules.nbquery(prio);
      let qm = queryconf.query_message(&peers.get(0).unwrap().0, nb_res, nb_hop, nb_for, prio);
      let peer_q = ApiCommand::call_service_reply(KVStoreCommand::Find(qm,val.get_key(),None),o_res.clone());
      peers.get_mut(0).unwrap().1.send(peer_q).unwrap();
      let mut o_res = clone_wait_one_result(&o_res,None).unwrap();
      assert!(o_res.0.len() == 1, "Peer not found {:?}", val.get_key_ref());
      let v = o_res.0.pop().unwrap();
      let result : Option<ArcRef<PeerTest>> = if let ApiResult::ServiceReply(MCReply::Global(KVStoreReply::FoundApi(ores,_))) = v {
        ores
      } else if let ApiResult::ServiceReply(MCReply::Global(KVStoreReply::FoundApiMult(mut vres,_))) = v {
        vres.pop()
      } else {
        None
      };
      assert!(result.map(|p|{
        let pt : &PeerTest = p.borrow();
        *pt == val
      }).unwrap_or(false));

//    let res = peers.get(0).unwrap().1.find_val(val.get_key().clone(), &queryconf, prio,StoragePriority::NoStore, 1).pop().unwrap_or(None);
//    assert_eq!(res, Some(val.clone()));
    }
}

/* This two variants  will check local store on proxy : TODO currently not implemented!!! : rules will
 * define it not message content
#[cfg(feature="with-extra-test")]
#[test]
fn simpeer2hopstoreval () {
    let nbpeer = 4;
    let val = PeerTest {
         nodeid: "to_find".to_string(),
         address : LocalAdd(999),
         keyshift : 1000,
         modeshauth : ShadowModeTest::NoShadow,
         modeshmsg : ShadowModeTest::SimpleShift,
    };
    let map : &[&[usize]] = &[&[],&[1,3],&[],&[3]];

    let mut rules = DHTRULES_DEFAULT.clone();
    rules.nbhopfact = 1;
    let prio = 3;
    let peers = initpeers_test2(nbpeer, map, TestingRules::new_no_delay(), rules, None);
    let dest = nbpeer -1;
    let conf = ALLTESTMODE.get(0).unwrap();
    let queryconf = QueryConf {
      mode : conf.clone(), 
//      chunk : QueryChunk::None, 
      hop_hist : Some((4,true))
    };
    assert!(dest.store_val(val.clone(), &queryconf, prio, StoragePriority::Local));
    // prio 3 so 3 * 1 = 0
    let res = peers.get(0).unwrap().1.find_val(val.get_key().clone(), &queryconf, prio,StoragePriority::Local, 1).pop().unwrap_or(None);
    assert_eq!(res, Some(val.clone()));
    // prio 0 is nohop (we see if localy store)
    let res = peers.get(0).unwrap().1.find_val(val.get_key().clone(), &queryconf, 0,StoragePriority::NoStore, 1).pop().unwrap_or(None);
    assert_eq!(res, Some(val.clone()));
    let res = peers.get(1).unwrap().1.find_val(val.get_key().clone(), &queryconf, 0,StoragePriority::NoStore, 1).pop().unwrap_or(None);
    assert!(!(res == Some(val.clone())));
}

#[test]
fn testpeer2hopstoreval () {
    let nbpeer = 4;
    let val = PeerTest {
         nodeid: "to_find".to_string(),
         address : LocalAdd(999),
         keyshift : 1000, 
         modeshauth : ShadowModeTest::NoShadow,
         modeshmsg : ShadowModeTest::SimpleShift,
    };
    let map : &[&[usize]] = &[&[2],&[3],&[4],&[]];

    let mut rules = DHTRULES_DEFAULT.clone();
    rules.nbhopfact = 1;
    let prio = 3;
    let peers = initpeers_test2(nbpeer, map, TestingRules::new_no_delay(), rules, None);
    let ref dest = nbpeer -1;
    let conf = ALLTESTMODE.get(0).unwrap();
    let queryconf = QueryConf {
      mode : conf.clone(), 
//      chunk : QueryChunk::None, 
      hop_hist : Some((4,true)),
    };
    assert!(dest.store_val(val.clone(), &queryconf, prio, StoragePriority::Local));
    // prio 3 so 3 * 1 = 0
    let res = peers.get(0).unwrap().1.find_val(val.get_key().clone(), &queryconf, prio,StoragePriority::Local, 1).pop().unwrap_or(None);
    assert_eq!(res, Some(val.clone()));
    // prio 0 is nohop (we see if localy store)
    let res = peers.get(0).unwrap().1.find_val(val.get_key().clone(), &queryconf, 0,StoragePriority::NoStore, 1).pop().unwrap_or(None);
    assert_eq!(res, Some(val.clone()));
    let res = peers.get(1).unwrap().1.find_val(val.get_key().clone(), &queryconf, 0,StoragePriority::NoStore, 1).pop().unwrap_or(None);
    assert!(!(res == Some(val.clone())));
}
*/

