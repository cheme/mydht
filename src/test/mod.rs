//! Tests, unitary tests are more likely to be found in their related package, here is global
//! testing.

use std::sync::{
  Arc,
};
use std::thread;
use DHT;
use std::hash::Hash;
use query::simplecache::SimpleCache;
use query::simplecache::SimpleCacheQuery;
use route::inefficientmap::Inefficientmap;
use transport::local_transport::{TransportTest};
use transport::{LocalAdd};
use rules::simplerules::{SimpleRules,DhtRules};
use procs::{
  RunningContext, 
  RunningTypes,
  RunningTypesImpl,
};
use peer::test::{
  TestingRules,
  PeerTest,
};
use keyval::KeyVal;
use rand::{thread_rng,Rng};
use query::{QueryConf,QueryMode,QueryChunk,QueryPriority};
use kvstore::StoragePriority;
use msgenc::json::Json;
use utils::ArcKV;
use peer::PeerMgmtMeths;
use msgenc::MsgEnc;

#[cfg(feature="with-extra-test")]
mod diag_tcp;
#[cfg(feature="with-extra-test")]
mod diag_udp;


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
};


fn simu_aproxy_peer_discovery () {
  let rcs = runningcontext1(5,DHTRULES_DEFAULT.clone());
  let qconf = QueryConf {
    mode : QueryMode::AProxy,
    chunk : QueryChunk::None,
    hop_hist : Some((4,false)),
  };
  peerconnect_scenario(&qconf, 2, rcs)
}

fn simu_amix_proxy_peer_discovery () {
  let rcs = runningcontext1(5,DHTRULES_DEFAULT.clone());
  let qconf = QueryConf {
    mode : QueryMode::AMix(9),
    chunk : QueryChunk::None,
    hop_hist : None,
  };
  peerconnect_scenario(&qconf, 2, rcs)
}

fn simu_asynch_peer_discovery () {
  let rcs = runningcontext1(5,DHTRULES_DEFAULT.clone());
  let qconf = QueryConf {
    mode : QueryMode::Asynch,
    chunk : QueryChunk::None,
    hop_hist : Some((3,true)),
  };
  peerconnect_scenario(&qconf, 2, rcs)
}


#[test]
fn aproxy_peer_discovery () {
  let nbpeer = 5;
  let rcs = runningcontext1(nbpeer,DHTRULES_DEFAULT.clone());
  let qconf = QueryConf {
    mode : QueryMode::AProxy,
    chunk : QueryChunk::None,
    hop_hist : Some((4,false)),
  };
  peerconnect_test(&qconf, rcs)
}

#[test]
fn amix_proxy_peer_discovery () {
  let nbpeer = 6;
  let rcs = runningcontext1(nbpeer,DHTRULES_DEFAULT.clone());
  let qconf = QueryConf {
    mode : QueryMode::AMix(9),
    chunk : QueryChunk::None,
    hop_hist : None,
  };
  peerconnect_test(&qconf, rcs)
}

#[test]
fn asynch_peer_discovery () {
  let nbpeer = 9;
  let mut rules = DHTRULES_DEFAULT.clone();
  rules.nbhopfact = 8;
  let rcs = runningcontext1(nbpeer,rules);
  let qconf = QueryConf {
    mode : QueryMode::Asynch,
    chunk : QueryChunk::None,
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
  let procs : Vec<DHT<RT>> = contexts.into_iter().map(|context|{
    info!("node : {:?}", context.me);

//        rng.shuffle(noderng);
    let bpeers = nodes.clone().into_iter().filter(|i| (*i != *context.me && rng.gen_range(0,knownratio) == 0) ).map(|p|Arc::new(p)).collect();
//        let mut noderng : &mut [PeerTest] = bpeers.as_slice();
    DHT::boot_server(Arc:: new(context),
      move || Some(Inefficientmap::new()), 
      move || Some(SimpleCacheQuery::new(false)), 
      move || Some(SimpleCache::new(None)), 
      Vec::new(), 
      bpeers)
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
  let procs : Vec<DHT<RT>> = contexts.into_iter().zip(0..nb).map(|(context,ix)|{
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
        move || Some(Inefficientmap::new()), 
        move || Some(SimpleCacheQuery::new(false)), 
        move || Some(SimpleCache::new(None)), 
        Vec::new(), 
        bpeers)
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

}

#[cfg(test)]
static ALLTESTMODE : [QueryMode; 1] = [
                     QueryMode::Asynch,
       //            QueryMode::AProxy,
    //               QueryMode::AMix(1),
    //               QueryMode::AMix(2),
     //              QueryMode::AMix(3)
                   ];

#[test]
fn simPeer2hopget (){
    let n = 4;
    let map : &[&[usize]] = &[&[2],&[3],&[],&[3]];
    for m in ALLTESTMODE.iter() {
      finddistantpeer(n,(*m).clone(),TestingRules::new_no_delay(),1,map,true,true);
    }
}

#[test]
fn testPeer2hopget (){
    let n = 4;
    let map : &[&[usize]] = &[&[2],&[3],&[4],&[]];
    for m in ALLTESTMODE.iter() {
      finddistantpeer(n,(*m).clone(),TestingRules::new_no_delay(),1,map,true,false); 
    }
}

#[test]
fn simPeermultipeersnoresult (){
    let n = 6;
    let map : &[&[usize]] = &[&[2,3,4],&[3,5],&[1],&[4],&[1],&[]];
    for m in ALLTESTMODE.iter() {
      finddistantpeer(n,(*m).clone(),TestingRules::new_no_delay(),1,map,false,true);
    }
}


#[test]
fn testPeer4hopget (){
    let n = 6;
    let map : &[&[usize]] = &[&[2],&[3],&[4],&[5],&[6],&[]];
    for m in ALLTESTMODE.iter() {
      finddistantpeer(n,(*m).clone(),TestingRules::new_no_delay(),1,map,true,false);
    };
   
    // prio 2 max nb hop is 3 TODO : for now prio 2 max np is 2 * 9
    for m in ALLTESTMODE.iter() {
      finddistantpeer(n,(*m).clone(),TestingRules::new_no_delay(),2,map,false,false);
    };
}

#[test]
fn simloopget (){ // TODO this only test loop over our node TODO circuit loop test
    let n = 4;
    // closest used are two first nodes (first being ourselves
    let map : &[&[usize]] = &[&[1,2],&[2,3],&[3,4],&[4]];
    //let map : &[&[usize]] = &[&[1,2],&[2,1,3],&[3,4],&[4]];
    for m in ALLTESTMODE.iter() {
      finddistantpeer(n,(*m).clone(),TestingRules::new_no_delay(),1,map,true,true);
    };
}

/// sim indicate if the read map need to wait for the ping back, in this case using sim = true add
/// some delay to test (more a simulation).
fn finddistantpeer (nbpeer : usize, qm : QueryMode, meths : TestingRules, prio : QueryPriority, map : &[&[usize]], find : bool, sim : bool) {
    let peers = initpeers_test(nbpeer, map, meths, sim);
    let queryconf = QueryConf {
      mode : qm.clone(), 
      chunk : QueryChunk::None, 
      hop_hist : Some((3,true))
    }; // note that we only unloop to 3 hop 
    let dest = peers.get(nbpeer -1).unwrap().0.clone();
    let fpeer = peers.get(0).unwrap().1.find_peer(dest.nodeid.clone(), &queryconf, prio);
    let matched = match fpeer {
       Some(ref v) => **v == dest,
       _ => false,
    };
    if(find){
    assert!(matched, "Peer not found {:?} , {:?}", fpeer, qm);
    }else{
    assert!(!matched, "Peer found {:?} , {:?}", fpeer, qm);
    }
}

fn initpeers<E : MsgEnc + Clone, RT : RunningTypes<R = SimpleRules, E = E>> (nodes : Vec<RT::P>, transports : Vec<RT::T>, map : &[&[usize]], meths : RT::M, dhtrules : DhtRules, enc : RT::E, sim : bool) -> Vec<(RT::P, DHT<RT>)> 
where <RT as RunningTypes>::M : Clone,
      <<RT as RunningTypes>::P as KeyVal>::Key : Hash + Ord,
      <<RT as RunningTypes>::V as KeyVal>::Key : Hash,
  {
  let mut i = 0;// TODO redesign with zip of map and nodes iter
  let result :  Vec<(RT::P, DHT<RT>, Vec<Arc<RT::P>>)> = transports.into_iter().map(|t|{
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
        move || Some(Inefficientmap::new()), 
        move || Some(SimpleCacheQuery::new(false)), 
        move || Some(SimpleCache::new(None)), 
        bpeers.clone(), Vec::new(),
     ),
     bpeers
     )
   }).collect();
   if sim {
     // all has started
     for n in result.iter(){
       thread::sleep_ms(100); // local get easily stuck
       n.1.refresh_closest_peers(1000); // Warn hard coded value.
     };
     // ping established
     thread::sleep_ms(2000);
   } else {
     //establish connection by peerping of bpeers and wait result : no need to sleep
     for n in result.iter(){
       for p in n.2.iter(){
         assert!(n.1.ping_peer((*p).clone()));
       }
     };
   
   };

   
   result.into_iter().map(|n|(n.0,n.1)).collect()
}

fn initpeers_test (nbpeer : usize, map : &[&[usize]], meths : TestingRules, sim : bool) -> Vec<(PeerTest, DHT<RunningTypes1>)> {
  let transports = TransportTest::create_transport(nbpeer,true,true);
  let mut nodes = Vec::new();
  for i in 0 .. nbpeer {
    let peer = PeerTest {
         nodeid: "dummyID".to_string() + (&i.to_string()[..]), 
         address : LocalAdd(i),
    };
    nodes.push(peer);
  };
  let mut rules = DHTRULES_DEFAULT.clone();
  // 9 hop
  rules.nbhopfact = 9;
  initpeers(nodes, transports, map, meths, rules,Json,sim)
}

#[test]
fn testPeer2hopfindval () {
    let nbpeer = 4;
    let val = PeerTest {
         nodeid: "to_find".to_string(),
         address : LocalAdd(999),
    };

    let map : &[&[usize]] = &[&[],&[1,3],&[],&[3]];

    let prio = 1;
    let peers = initpeers_test(nbpeer, map, TestingRules::new_no_delay(), true);
    let ref dest = peers.get(nbpeer -1).unwrap().1;
    for conf in ALLTESTMODE.iter(){
    let queryconf = QueryConf {
      mode : conf.clone(), 
      chunk : QueryChunk::None, 
      hop_hist : Some((7,false))
    };
    assert!(dest.store_val(val.clone(), &queryconf, prio, StoragePriority::Local));
    let res = peers.get(0).unwrap().1.find_val(val.get_key().clone(), &queryconf, prio,StoragePriority::NoStore, 1).pop().unwrap_or(None);
    assert_eq!(res, Some(val.clone()));
    }
}

#[test]
fn testPeer2hopstoreval () {
    let nbpeer = 4;
    let val = PeerTest {
         nodeid: "to_find".to_string(),
         address : LocalAdd(999),
    };
    let map : &[&[usize]] = &[&[],&[1,3],&[],&[3]];
    let prio = 1;
    let peers = initpeers_test(nbpeer, map, TestingRules::new_no_delay(), true);
    let ref dest = peers.get(nbpeer -1).unwrap().1;
    let conf = ALLTESTMODE.get(0).unwrap();
    let queryconf = QueryConf {
      mode : conf.clone(), 
      chunk : QueryChunk::None, 
      hop_hist : Some((4,true))
    };
    assert!(dest.store_val(val.clone(), &queryconf, prio, StoragePriority::Local));
    let res = peers.get(0).unwrap().1.find_val(val.get_key().clone(), &queryconf, prio,StoragePriority::Local, 1).pop().unwrap_or(None);
    assert_eq!(res, Some(val.clone()));
    // prio 10 is nohop (we see if localy store)
    let res = peers.get(0).unwrap().1.find_val(val.get_key().clone(), &queryconf, 10,StoragePriority::NoStore, 1).pop().unwrap_or(None);
    assert_eq!(res, Some(val.clone()));
    let res = peers.get(1).unwrap().1.find_val(val.get_key().clone(), &queryconf, 10,StoragePriority::NoStore, 1).pop().unwrap_or(None);
    assert!(!(res == Some(val.clone())));
}

