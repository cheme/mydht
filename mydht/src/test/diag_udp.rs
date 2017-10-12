//! Run existing mydht Tests over a udp transport (with true sockets)
//!

#[cfg(test)]
extern crate mydht_bincode;
extern crate mydht_udp;
use rules::DHTRules;
use std::borrow::Borrow;
use procs::storeprop::{
  KVStoreService,
  KVStoreCommand,
  KVStoreReply,
  KVStoreProtoMsgWithPeer,
};
use procs::api::{
  Api,
  ApiReply,
  ApiResult,
  ApiQueriable,
  ApiQueryId,
  ApiRepliable,
};
use procs::{
  MCCommand,
  MCReply,
  PeerCacheRouteBase,
  RWSlabEntry,
  OptInto,
  OptFrom,
};


use service::{
  NoService,
  NoSpawn,
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


use utils::{
  OneResult,
  new_oneresult,
  clone_wait_one_result,
  Ref,
  ArcRef,
  RcRef,
  CloneRef,

};
use procs::{
  ApiCommand,
};

use self::mydht_bincode::Bincode;
use procs::api::{
  ApiSendIn,
};


use msgenc::MsgEnc;
use transport::{
  Transport,
  SerSocketAddr,
};
use self::mydht_udp::Udp;
use utils;
use keyval::KeyVal;
use std::net::{SocketAddr,Ipv4Addr};
use kvstore::StoragePriority;
use query::{QueryConf,QueryMode,QueryPriority};
use super::{
  initpeers2,
  TestConf,
  ALLTESTMODE,
  DHTRULES_DEFAULT,
};
use rules::simplerules::{DhtRules};
use peer::test::TestingRules;
#[cfg(test)]
use node::Node;
use DHT;
use peer::PeerMgmtMeths;
use procs::RunningTypes;
use std::marker::PhantomData;
use rules::simplerules::SimpleRules;
use num::traits::ToPrimitive;
use procs::ClientMode;

#[cfg(test)]
use mydht_basetest::transport::connect_rw_with_optional_non_managed;

struct RunningTypesImpl<M : PeerMgmtMeths<Node>, T : Transport, E : MsgEnc<Node,Node>> (PhantomData<(M,T,E)>);

impl<M : PeerMgmtMeths<Node>, T : Transport<Address=SerSocketAddr>, E : MsgEnc<Node,Node>> RunningTypes for RunningTypesImpl<M, T, E> {
  type A = SerSocketAddr;
  type P = Node;
  type V = Node;
  type M = M;
  type R = SimpleRules;
  type E = E;
  type T = T;
}

#[test]
fn connect_rw () {
  let start_port = 60000;

  let a1 = SerSocketAddr(utils::sa4(Ipv4Addr::new(127,0,0,1), start_port));
  let a2 = SerSocketAddr(utils::sa4(Ipv4Addr::new(127,0,0,1), start_port+1));
  let tcp_transport_1 : Udp = Udp::new (&a1, 500, true).unwrap();
  let tcp_transport_2 : Udp = Udp::new (&a2, 500, true).unwrap();

  connect_rw_with_optional_non_managed(tcp_transport_1,tcp_transport_2,&a1,&a2,false,false,false,false);
}

#[test]
fn connect_rw_nospawn () {
  let start_port = 60100;

  let a1 = SerSocketAddr(utils::sa4(Ipv4Addr::new(127,0,0,1), start_port));
  let a2 = SerSocketAddr(utils::sa4(Ipv4Addr::new(127,0,0,1), start_port+1));
  let tcp_transport_1 : Udp = Udp::new (&a1, 500, false).unwrap();
  let tcp_transport_2 : Udp = Udp::new (&a2, 500, false).unwrap();

  connect_rw_with_optional_non_managed(tcp_transport_1,tcp_transport_2,&a1,&a2,false,false,true,false);
}

fn initpeers_udp2 (start_port : u16, nbpeer : usize, map : &[&[usize]], meths : TestingRules, rules : DhtRules, sim : Option<u32>)
  -> Vec<(Node, ApiSendIn<TestConf<Node,Udp,Bincode,TestingRules,SimpleRules>>)> {
  let mut nodes = Vec::new();
  let mut transports = Vec::new();

  for i in 0 .. nbpeer {
    let addr = utils::sa4(Ipv4Addr::new(127,0,0,1), start_port + i.to_u16().unwrap());
    let udp_transport = Udp::new(&addr,2048,true).unwrap(); // here udp with a json encoding with last sed over a few hop : we need a big buffer
    transports.push(udp_transport);
    nodes.push(Node {nodeid: "NodeID".to_string() + &(i + 1).to_string()[..], address : SerSocketAddr(addr)});
  };
  initpeers2(nodes, transports, map, meths, rules,Bincode,sim)
}

/*

#[test]
fn simpeer1hopfindval_udp () {
    let nbpeer = 2;
    // addr dummy (used as keyval so no ping validation)
    let addr = utils::sa4(Ipv4Addr::new(127,0,0,1), 999);
    let val = Node {nodeid: "to_find".to_string(), address : SerSocketAddr(addr)};



    let map : &[&[usize]] = &[&[],&[1]];

    let mut startport = 75440;
    let prio = 1;

    let mut rules = DHTRULES_DEFAULT.clone();
    rules.clientmode = ClientMode::Local(true);
    rules.nbhopfact = 3;
    let peers = initpeers_udp(startport,nbpeer, map, TestingRules::new_no_delay(), rules, Some(2000));
    let ref dest = peers.get(nbpeer -1).unwrap().1;
    for conf in ALLTESTMODE.iter(){
      let queryconf = QueryConf {
        mode : conf.clone(), 
//        chunk : QueryChunk::None, 
        hop_hist : Some((7,false))
      };
      assert!(dest.store_val(val.clone(), &queryconf, prio, StoragePriority::Local));
      let res = peers.get(0).unwrap().1.find_val(val.get_key().clone(), &queryconf, prio,StoragePriority::NoStore, 1).pop().unwrap_or(None);
      assert_eq!(res, Some(val.clone()));
    }
}
*/


// TODO udp transport impl is totally broken now (before also i guess but protocol was simplier to
// allow one exchange : the following test block after first message (first ping/pong fine then new
// read stream containing unattached content is ko) : Test is disabled
//#[test]
fn simpeer2hopfindval_udp () {
    //let nbpeer = 4;
    let nbpeer = 2;
    // addr dummy (used as keyval so no ping validation)
    let addr = utils::sa4(Ipv4Addr::new(127,0,0,1), 999);
    let val = Node {nodeid: "to_find".to_string(), address : SerSocketAddr(addr)};



    //let map : &[&[usize]] = &[&[],&[1,3],&[],&[3]];
    let map : &[&[usize]] = &[&[],&[1]];

    let mut startport = 73440;
    let prio = 1;

    let mut rules = DHTRULES_DEFAULT.clone();
    rules.clientmode = ClientMode::Local(true);
    rules.nbhopfact = 3;
    //let peers = initpeers_udp2(startport,nbpeer, map, TestingRules::new_no_delay(), rules, Some(2000));
    let mut peers = initpeers_udp2(startport,nbpeer, map, TestingRules::new_no_delay(), rules.clone(), None);
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

      let srules = SimpleRules::new(rules.clone());
      let o_res = new_oneresult((Vec::with_capacity(nb_res),nb_res,nb_res));
      let nb_hop = srules.nbhop(prio);
      let nb_for = srules.nbquery(prio);
      let qm = queryconf.query_message(&peers.get(0).unwrap().0, nb_res, nb_hop, nb_for, prio);
      let peer_q = ApiCommand::call_service_reply(KVStoreCommand::Find(qm,val.get_key(),None),o_res.clone());
      peers.get_mut(0).unwrap().1.send(peer_q).unwrap();
      let mut o_res = clone_wait_one_result(&o_res,None).unwrap();
      assert!(o_res.0.len() == 1, "Peer not found {:?}", val.get_key_ref());
      let v = o_res.0.pop().unwrap();
      let result : Option<ArcRef<Node>> = if let ApiResult::ServiceReply(MCReply::Global(KVStoreReply::FoundApi(ores,_))) = v {
        ores
      } else if let ApiResult::ServiceReply(MCReply::Global(KVStoreReply::FoundApiMult(mut vres,_))) = v {
        vres.pop()
      } else {
        None
      };
      assert!(result.map(|p|{
        let pt : &Node = p.borrow();
        *pt == val
      }).unwrap_or(false));
    }
}
/* Similar to previous as sim is not runing
#[test]
fn testpeer2hopfindval_udp () {
    let nbpeer = 4;
    // addr dummy (used as keyval so no ping validation)
    let addr = utils::sa4(Ipv4Addr::new(127,0,0,1), 999);
    let val = Node {nodeid: "to_find".to_string(), address : SerSocketAddr(addr)};



    let map : &[&[usize]] = &[&[2],&[3],&[4],&[]];

    let mut startport = 74440;
    let prio = 1;

    let mut rules = DHTRULES_DEFAULT.clone();
    rules.clientmode = ClientMode::Local(false);
    rules.nbhopfact = 3;
    let peers = initpeers_udp(startport,nbpeer, map, TestingRules::new_no_delay(),rules,None);
    let ref dest = peers.get(nbpeer -1).unwrap().1;
    for conf in ALLTESTMODE.iter(){
      let queryconf = QueryConf {
        mode : conf.clone(), 
//        chunk : QueryChunk::None, 
        hop_hist : Some((7,false))
      };
      assert!(dest.store_val(val.clone(), &queryconf, prio, StoragePriority::Local));
      let res = peers.get(0).unwrap().1.find_val(val.get_key().clone(), &queryconf, prio,StoragePriority::NoStore, 1).pop().unwrap_or(None);
      assert_eq!(res, Some(val.clone()));
    }
}*/

