//! Tests, unitary tests are more likely to be found in their related package, here is global
//! testing.

use std::sync::{
  Arc,
  Mutex,
};
use std::thread;
use DHT;
use std::hash::Hash;
use query::simplecache::SimpleCache;
use query::simplecache::SimpleCacheQuery;
use route::inefficientmap::Inefficientmap;
use transport::local_transport::{TransportTest};
use transport::{LocalAdd};
use time::Duration;
use rules::simplerules::{SimpleRules,DhtRules};
use procs::{
  RunningContext, 
  RunningProcesses, 
  ArcRunningContext, 
  RunningTypes,
  RunningTypesImpl,
};
use peer::test::{
  TestingRules,
  PeerTest,
};
use keyval::KeyVal;
use rand::{thread_rng,Rng};
use query::{QueryConf,QueryMode,QueryChunk};
use msgenc::json::Json;


#[cfg(feature="with-extra-test")]
mod diag_tcp;
#[cfg(feature="with-extra-test")]
mod diag_udp;


const dhtrules_default : DhtRules = DhtRules {
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



//#[test]
fn aproxyPeerDiscovery () {
  let rcs = runningcontext1(5,dhtrules_default.clone());
  let qconf = QueryConf {
    mode : QueryMode::AProxy,
    chunk : QueryChunk::None,
    hop_hist : Some((4,false)),
  };
  peerConnectScenario(&qconf, 2, rcs)
}

//#[test]
fn amixProxyPeerDiscovery () {
  let rcs = runningcontext1(5,dhtrules_default.clone());
  let qconf = QueryConf {
    mode : QueryMode::AMix(9),
    chunk : QueryChunk::None,
    hop_hist : None,
  };
  peerConnectScenario(&qconf, 2, rcs)
}

//#[test]
fn asynchPeerDiscovery () {
  let rcs = runningcontext1(5,dhtrules_default.clone());
  let qconf = QueryConf {
    mode : QueryMode::Asynch,
    chunk : QueryChunk::None,
    hop_hist : Some((3,true)),
  };
  peerConnectScenario(&qconf, 2, rcs)
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
fn peerConnectScenario<RT : RunningTypes> (queryconf : &QueryConf, knownratio : usize, contexts : Vec<RunningContext<RT>>) 
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
    for n in itern{
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

//    for mut p in procs.iter(){p.block()}
//    assert_eq!(1i,1i);
}


