//! Tests, unitary tests are more likely to be found in their related package, here is global
//! testing.
extern crate mydht_inefficientmap;
use std::sync::{
  Arc,
};
use std::thread;
use DHT;
use std::hash::Hash;
use simplecache::SimpleCache;
use query::simplecache::SimpleCacheQuery;
use self::mydht_inefficientmap::inefficientmap::Inefficientmap;
use self::mydht_inefficientmap::inefficientmap::new as new_inmap;
use rules::simplerules::{SimpleRules,DhtRules};
use procs::{
  RunningContext, 
  RunningTypes,
  RunningTypesImpl,
};
use peer::test::{
  TestingRules,
  PeerTest,
  ShadowModeTest,
};
use keyval::KeyVal;
use rand::{thread_rng,Rng};
use query::{QueryConf,QueryMode,QueryPriority};
use kvstore::StoragePriority;
use msgenc::json::Json;
use utils::ArcKV;
use peer::PeerMgmtMeths;
use msgenc::MsgEnc;
use num::traits::ToPrimitive;
use procs::ClientMode;
#[cfg(feature="with-extra-test")]
mod diag_tcp;
#[cfg(feature="with-extra-test")]
mod diag_udp;

mod mainloop;
pub use mydht_basetest::local_transport::*;
pub use mydht_basetest::transport::*;
const DHTRULES_DEFAULT : DhtRules = DhtRules {
  randqueryid : false,
  // nbhop = prio * fact
  nbhopfact : 1,
  // nbquery is 1 + query * fact
  nbqueryfact : 1.0, 
  //query lifetime second
  lifetime : 15,
  // increment of lifetime per priority inc
  lifetimeinc : 2,
  cleaninterval : None, // in seconds if needed
  cacheduration : None, // cache in seconds
  cacheproxied : false, // do you cache proxied result
  storelocal : true, // is result stored locally
  storeproxied : None, // store only if less than nbhop
  heavyaccept : false,
  clientmode : ClientMode::ThreadedOne,
  // TODO client mode param + testing for local tcp and mult tcp in max 2 thread and in pool 2
  // thread
  tunnellength : 3,
};


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
  peerconnect_scenario(&qconf, 2, rcs)
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
  peerconnect_scenario(&qconf, 2, rcs)
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
  peerconnect_scenario(&qconf, 2, rcs)
}


#[test]
fn aproxy_peer_discovery () {
  let nbpeer = 5;
  let mut rules = DHTRULES_DEFAULT.clone();
  rules.nbhopfact = nbpeer - 1;
  let rcs = runningcontext1(nbpeer.to_usize().unwrap(),rules);
  let qconf = QueryConf {
    mode : QueryMode::AProxy,
//    chunk : QueryChunk::None,
    hop_hist : Some((4,false)),
  };
  peerconnect_test(&qconf, rcs)
}

#[test]
fn amix_proxy_peer_discovery () {
  let nbpeer = 6;
  let mut rules = DHTRULES_DEFAULT.clone();
  rules.nbhopfact = nbpeer - 1;
  let rcs = runningcontext1(nbpeer.to_usize().unwrap(),rules);
  let qconf = QueryConf {
    mode : QueryMode::AMix(9),
//    chunk : QueryChunk::None,
    hop_hist : None,
  };
  peerconnect_test(&qconf, rcs)
}

#[test]
fn asynch_peer_discovery () {
  let nbpeer = 9;
  let mut rules = DHTRULES_DEFAULT.clone();
  rules.nbhopfact = nbpeer - 1;
  let rcs = runningcontext1(nbpeer.to_usize().unwrap(),rules);
  let qconf = QueryConf {
    mode : QueryMode::Asynch,
//    chunk : QueryChunk::None,
    hop_hist : Some((5,true)),
  };
  peerconnect_test(&qconf, rcs)
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
type RunningTypes1 = RunningTypesImpl<LocalAdd, PeerTest, PeerTest, TestingRules, SimpleRules, Json, TransportTest>;

fn runningcontext1 (nbpeer : usize, dhtrules : DhtRules) -> Vec<RunningContext<RunningTypes1>> {
  // non duplex
  let mut transports = TransportTest::create_transport(nbpeer,true,true);
  transports.reverse();
  let mut rcs = Vec::with_capacity(nbpeer);
  for i in 0 .. nbpeer {
    let peer = PeerTest {
         nodeid: "dummyID".to_string() + (&i.to_string()[..]), 
         address : LocalAdd(i),
         keyshift : i as u8 + 1,
         modeshauth : ShadowModeTest::NoShadow,
         modeshmsg : ShadowModeTest::SimpleShift,
       };

    let context = RunningContext::new(
       Arc::new(peer),
       TestingRules::new_no_delay(),
       SimpleRules::new(dhtrules.clone()),
       Json,
       transports.pop().unwrap(),
    );
    rcs.push(context);
  };
  rcs
}

/// ration is 1 for all 2 for half and so on.
fn peerconnect_scenario<RT : RunningTypes> (queryconf : &QueryConf, knownratio : usize, contexts : Vec<RunningContext<RT>>) 
where <RT:: P as KeyVal>::Key : Ord + Hash,
      <RT:: V as KeyVal>::Key : Hash
{

  let nodes : Vec<RT::P> = contexts.iter().map(|c|(*c.me).clone()).collect();
  let mut rng = thread_rng();
/*  let procs : Vec<DHT<RT>> = contexts.into_iter().map(|context|{
    info!("node : {:?}", context.me);

//        rng.shuffle(noderng);
    let bpeers = nodes.clone().into_iter().filter(|i| (*i != *context.me && rng.gen_range(0,knownratio) == 0) ).map(|p|Arc::new(p)).collect();
//        let mut noderng : &mut [PeerTest] = bpeers.as_slice();
    DHT::boot_server(Arc:: new(context),
      move || Some(new_inmap()), 
      move || Some(SimpleCacheQuery::new(false)), 
      move || Some(SimpleCache::new(None)), 
      Vec::new(), 
      bpeers).unwrap()
  }).collect();

  thread::sleep_ms(2000); // TODO remove this
  // find all node from the first node first node
  let ref fprocs = procs[0];

  let mut itern = nodes.iter();
  itern.next();
  for n in itern {
    // local find 
    let fpeer = fprocs.find_peer(n.get_key(), queryconf, 1); // TODO put in future first then match result (simultaneous search)
    let matched = match fpeer {
      Some(v) => *v == *n,
      _ => false,
    };
    assert!(matched, "Peer not found {:?}", n);
  }

  for p in procs.iter(){p.shutdown()}
//    procs.iter().map(|p|{p.shutdown()}); // no cause lazy
*/
}

fn peerconnect_test<RT : RunningTypes> (queryconf : &QueryConf, contexts : Vec<RunningContext<RT>>) 
where <RT:: P as KeyVal>::Key : Ord + Hash,
      <RT:: V as KeyVal>::Key : Hash
{
  // topography will be if 1/3 size connected nodes and bridge in between
  let nb = contexts.len();
  let gnb = nb / 6;
  let mid = nb / 2;
  let nodes : Vec<RT::P> = contexts.iter().map(|c|(*c.me).clone()).collect();
  let mut rng = thread_rng();
/*  let procs : Vec<DHT<RT>> = contexts.into_iter().zip(0..nb).map(|(context,ix)|{
    info!("node : {:?}", context.me);
    

    let mut bpeers =  Vec::new();
    if ix < mid {
      bpeers.push(Arc::new((nodes.get(ix + 1).unwrap()).clone()));
    } else if ix > mid {
      bpeers.push(Arc::new((nodes.get(ix - 1).unwrap()).clone()));
    };
    if ix < gnb {
      for i in 0 .. gnb {
        if i != ix && i != ix + 1 {
          bpeers.push(Arc::new((nodes.get(i).unwrap()).clone()));
        }
      }
    } else if ix > (nb - gnb) {
      for i in (nb - gnb) .. nb {
        if i != ix && i != ix - 1 {
          bpeers.push(Arc::new((nodes.get(i).unwrap()).clone()));
        }
      }
    };

//        let mut noderng : &mut [PeerTest] = bpeers.as_slice();
    DHT::boot_server(Arc:: new(context),
        move || Some(new_inmap()), 
        move || Some(SimpleCacheQuery::new(false)), 
        move || Some(SimpleCache::new(None)), 
        Vec::new(), 
        bpeers).unwrap()
  }).collect();

  thread::sleep_ms(1000); // TODO remove this (might miss unidir in center)
  // find all node from the first node first node
  let ref fprocs = procs[0];

  let mut itern = nodes.iter();
  itern.next();
  for n in itern {
    // local find 
    let fpeer = fprocs.find_peer(n.get_key(), queryconf, 1); // TODO put in future first then match result (simultaneous search)
    let matched = match fpeer {
      Some(v) => *v == *n,
      _ => false,
     };
     assert!(matched, "Peer not found {:?}", n);
  }

  for p in procs.iter(){p.shutdown()}
//    procs.iter().map(|p|{p.shutdown()}); // no cause lazy
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
      let peers = initpeers_test(n, map, TestingRules::new_no_delay(), rules, DEF_SIM);
      finddistantpeer(peers,n,(*m).clone(),1,map,true);
    }
}

#[test]
fn testpeer2hopget (){
  let n = 4;
  let map : &[&[usize]] = &[&[2],&[3],&[4],&[]];
  for m in ALLTESTMODE.iter() {
    let mut rules = DHTRULES_DEFAULT.clone();
    rules.nbhopfact = 3;
    let peers = initpeers_test(n, map, TestingRules::new_no_delay(), rules, None);
    finddistantpeer(peers,n,(*m).clone(),1,map,true); 
  }
}

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
}

#[test]
fn testpeer2hopgetmultpool () {
  let n = 8;
  let map : &[&[usize]] = &[&[5,6,2,7,8],&[3,5,6,7],&[5,6,7,8,4],&[],&[],&[],&[],&[]];
  for m in FEWTESTMODE.iter() {
    let mut rules = DHTRULES_DEFAULT.clone();
    rules.clientmode = ClientMode::ThreadPool(2);
    rules.nbhopfact = 3;
    rules.nbqueryfact = 5.0;
    let peers = initpeers_test(n, map, TestingRules::new_no_delay(), rules, None);
    finddistantpeer(peers,n,(*m).clone(),1,map,true); 
  }
}
#[test]
fn testpeer2hopgetmultmax () {
  let n = 8;
  let map : &[&[usize]] = &[&[5,6,2,7,8],&[3,5,6,7],&[5,6,7,8,4],&[],&[],&[],&[],&[]];
  for m in FEWTESTMODE.iter() {
    let mut rules = DHTRULES_DEFAULT.clone();
    rules.clientmode = ClientMode::ThreadedMax(2);
    rules.nbhopfact = 3;
    rules.nbqueryfact = 5.0;
    let peers = initpeers_test(n, map, TestingRules::new_no_delay(), rules, None);
    finddistantpeer(peers,n,(*m).clone(),1,map,true); 
  }
}





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
}



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
    let peers = initpeers_test(n, map, TestingRules::new_no_delay(), rules, DEF_SIM);
    finddistantpeer(peers,n,(*m).clone(),1,map,false);
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
      let peers = initpeers_test(n, map, TestingRules::new_no_delay(), rules, None);
      finddistantpeer(peers,n,(*m).clone(),2,map,true);
    };
   
    // prio 1 max nb hop is 3
    for m in ALLTESTMODE.iter() {
      let mut rules = DHTRULES_DEFAULT.clone();
      rules.nbhopfact = 3;
      // for asynch we need only one nbquer (expected to many none otherwhise)
      if let &QueryMode::Asynch = m {
        rules.nbqueryfact = 0.0; // nb query is 1 + prio * nbqfact
      };
      let peers = initpeers_test(n, map, TestingRules::new_no_delay(), rules, None);
      finddistantpeer(peers,n,(*m).clone(),1,map,false);
    };
}

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
}



#[test]
fn simloopget (){ // TODO this only test loop over our node TODO circuit loop test
    let n = 4;
    // closest used are two first nodes (first being ourselves
    let map : &[&[usize]] = &[&[1,2],&[2,3],&[3,4],&[4]];
    //let map : &[&[usize]] = &[&[1,2],&[2,1,3],&[3,4],&[4]];
    for m in ALLTESTMODE.iter() {
      let mut rules = DHTRULES_DEFAULT.clone();
      rules.nbhopfact = 3;
      let peers = initpeers_test(n, map, TestingRules::new_no_delay(), rules, DEF_SIM);
      finddistantpeer(peers,n,(*m).clone(),1,map,true);
    };
}

/// sim indicate if the read map need to wait for the ping back, in this case using sim = true add
/// some delay to test (more a simulation).
fn finddistantpeer<RT : RunningTypes> (peers : Vec<(RT::P,DHT<RT>)>, nbpeer : usize, qm : QueryMode, prio : QueryPriority, map : &[&[usize]], find : bool) {
    let queryconf = QueryConf {
      mode : qm.clone(), 
//      chunk : QueryChunk::None, 
      hop_hist : Some((3,true))
    }; // note that we only unloop to 3 hop 
    let dest = peers.get(nbpeer -1).unwrap().0.clone();
    let fpeer = peers.get(0).unwrap().1.find_peer(dest.get_key(), &queryconf, prio);
    let matched = match fpeer {
       Some(ref v) => **v == dest,
       _ => false,
    };
    if find {
      assert!(matched, "Peer not found {:?} , {:?}", dest, qm);
    }else{
      assert!(!matched, "Peer found {:?} , {:?}", fpeer, qm);
    }
}

fn initpeers<E : MsgEnc<RT::P,RT::V> + Clone, RT : RunningTypes<R = SimpleRules, E = E>> (nodes : Vec<RT::P>, transports : Vec<RT::T>, map : &[&[usize]], meths : RT::M, dhtrules : DhtRules, enc : RT::E, sim : Option<u32>) -> Vec<(RT::P, DHT<RT>)> 
where <RT as RunningTypes>::M : Clone,
      <<RT as RunningTypes>::P as KeyVal>::Key : Hash + Ord,
      <<RT as RunningTypes>::V as KeyVal>::Key : Hash,
  {
  let mut i = 0;// TODO redesign with zip of map and nodes iter
/*  let result :  Vec<(RT::P, DHT<RT>, Vec<Arc<RT::P>>)> = transports.into_iter().map(|t|{
    let n = nodes.get(i).unwrap();
    info!("node : {:?}", n);
    println!("{:?}",map[i]);
    let bpeers : Vec<Arc<RT::P>> = map[i].iter().map(|j| nodes.get(*j-1).unwrap().clone()).map(|p|Arc::new(p)).collect();
    i += 1;
    let nsp = Arc::new(n.clone());
    (n.clone(), 
    DHT::boot_server(Arc:: new(
       RunningContext::new( 
         nsp,
         meths.clone(),
         SimpleRules::new(dhtrules.clone()),
         enc.clone(),
         t,
        )), 
        move || Some(new_inmap()), 
        move || Some(SimpleCacheQuery::new(false)), 
        move || Some(SimpleCache::new(None)), 
        bpeers.clone(), Vec::new(),
     ).unwrap(),
     bpeers
     )
   }).collect();
   if sim.is_some() {
     // all has started
     for n in result.iter(){
       thread::sleep_ms(100); // local get easily stuck
       n.1.refresh_closest_peers(1000); // Warn hard coded value.
     };
     // ping established
     thread::sleep_ms(sim.unwrap());
   } else {
     //establish connection by peerping of bpeers and wait result : no need to sleep
     for n in result.iter(){
       for p in n.2.iter(){
         assert!(n.1.ping_peer((*p).clone()));
       }
     };
   
   };

   
   result.into_iter().map(|n|(n.0,n.1)).collect()
   */
  Vec::new()
}

// local transport usage is faster than actual transports
// Yet default to one second
static DEF_SIM : Option<u32> = Some(1000);

fn initpeers_test (nbpeer : usize, map : &[&[usize]], meths : TestingRules, rules : DhtRules, sim : Option<u32>) -> Vec<(PeerTest, DHT<RunningTypes1>)> {
  let transports = TransportTest::create_transport(nbpeer,true,true);
  let mut nodes = Vec::new();
  for i in 0 .. nbpeer {
    let peer = PeerTest {
         nodeid: "dummyID".to_string() + (&i.to_string()[..]), 
         address : LocalAdd(i),
         keyshift : i as u8 + 1,

         modeshauth : ShadowModeTest::NoShadow,
         modeshmsg : ShadowModeTest::SimpleShift,
    };
    nodes.push(peer);
  };
  initpeers(nodes, transports, map, meths, rules, Json, sim)
}

#[cfg(feature="with-extra-test")]
#[test]
fn simpeer2hopfindval () {
    let nbpeer = 4;
    let val = PeerTest {
         nodeid: "to_find".to_string(),
         address : LocalAdd(999),
         keyshift : 1000,

         modeshauth : ShadowModeTest::NoShadow,
         modeshmsg : ShadowModeTest::SimpleShift,
    };

    let mut rules = DHTRULES_DEFAULT.clone();
    rules.nbhopfact = 3;
    let map : &[&[usize]] = &[&[],&[1,3],&[],&[3]];

    let prio = 1;
    let peers = initpeers_test(nbpeer, map, TestingRules::new_no_delay(), rules, DEF_SIM);
    let ref dest = peers.get(nbpeer -1).unwrap().1;
    for conf in ALLTESTMODE.iter(){
    let queryconf = QueryConf {
      mode : conf.clone(), 
//      chunk : QueryChunk::None, 
      hop_hist : Some((7,false))
    };
    assert!(dest.store_val(val.clone(), &queryconf, prio, StoragePriority::Local));
    let res = peers.get(0).unwrap().1.find_val(val.get_key().clone(), &queryconf, prio,StoragePriority::NoStore, 1).pop().unwrap_or(None);
    assert_eq!(res, Some(val.clone()));
    }
}


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
    let peers = initpeers_test(nbpeer, map, TestingRules::new_no_delay(), rules, DEF_SIM);
    let ref dest = peers.get(nbpeer -1).unwrap().1;
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
    let peers = initpeers_test(nbpeer, map, TestingRules::new_no_delay(), rules, None);
    let ref dest = peers.get(nbpeer -1).unwrap().1;
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

