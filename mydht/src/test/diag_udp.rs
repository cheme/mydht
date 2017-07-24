//! Run existing mydht Tests over a udp transport (with true sockets)
//!

#[cfg(test)]
extern crate mydht_bincode;
extern crate mydht_udp;


use self::mydht_bincode::Bincode;
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
use query::{QueryConf,QueryMode,QueryChunk,QueryPriority};
use super::{
  initpeers,
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

struct RunningTypesImpl<M : PeerMgmtMeths<Node, Node>, T : Transport, E : MsgEnc> (PhantomData<(M,T,E)>);

impl<M : PeerMgmtMeths<Node, Node>, T : Transport<Address=SerSocketAddr>, E : MsgEnc> RunningTypes for RunningTypesImpl<M, T, E> {
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

  connect_rw_with_optional_non_managed(tcp_transport_1,tcp_transport_2,&a1,&a2,false,false,false);
}

#[test]
fn connect_rw_nospawn () {
  let start_port = 60100;

  let a1 = SerSocketAddr(utils::sa4(Ipv4Addr::new(127,0,0,1), start_port));
  let a2 = SerSocketAddr(utils::sa4(Ipv4Addr::new(127,0,0,1), start_port+1));
  let tcp_transport_1 : Udp = Udp::new (&a1, 500, false).unwrap();
  let tcp_transport_2 : Udp = Udp::new (&a2, 500, false).unwrap();

  connect_rw_with_optional_non_managed(tcp_transport_1,tcp_transport_2,&a1,&a2,false,false,true);
}

fn initpeers_udp<M : PeerMgmtMeths<Node, Node> + Clone> (start_port : u16, nbpeer : usize, map : &[&[usize]], meths : M, rules : DhtRules, sim : Option<u32>) -> Vec<(Node, DHT<RunningTypesImpl<M,Udp,Bincode>>)>{
  let mut nodes = Vec::new();
  let mut transports = Vec::new();

  for i in 0 .. nbpeer {
    let addr = utils::sa4(Ipv4Addr::new(127,0,0,1), start_port + i.to_u16().unwrap());
    let udp_transport = Udp::new(&addr,2048,true).unwrap(); // here udp with a json encoding with last sed over a few hop : we need a big buffer
    transports.push(udp_transport);
    nodes.push(Node {nodeid: "NodeID".to_string() + &(i + 1).to_string()[..], address : SerSocketAddr(addr)});
  };
  initpeers(nodes, transports, map, meths, rules,Bincode,sim)
}
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
        chunk : QueryChunk::None, 
        hop_hist : Some((7,false))
      };
      assert!(dest.store_val(val.clone(), &queryconf, prio, StoragePriority::Local));
      let res = peers.get(0).unwrap().1.find_val(val.get_key().clone(), &queryconf, prio,StoragePriority::NoStore, 1).pop().unwrap_or(None);
      assert_eq!(res, Some(val.clone()));
    }
}


#[test]
fn simpeer2hopfindval_udp () {
    let nbpeer = 4;
    // addr dummy (used as keyval so no ping validation)
    let addr = utils::sa4(Ipv4Addr::new(127,0,0,1), 999);
    let val = Node {nodeid: "to_find".to_string(), address : SerSocketAddr(addr)};



    let map : &[&[usize]] = &[&[],&[1,3],&[],&[3]];

    let mut startport = 73440;
    let prio = 1;

    let mut rules = DHTRULES_DEFAULT.clone();
    rules.clientmode = ClientMode::Local(true);
    rules.nbhopfact = 3;
    let peers = initpeers_udp(startport,nbpeer, map, TestingRules::new_no_delay(), rules, Some(2000));
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
        chunk : QueryChunk::None, 
        hop_hist : Some((7,false))
      };
      assert!(dest.store_val(val.clone(), &queryconf, prio, StoragePriority::Local));
      let res = peers.get(0).unwrap().1.find_val(val.get_key().clone(), &queryconf, prio,StoragePriority::NoStore, 1).pop().unwrap_or(None);
      assert_eq!(res, Some(val.clone()));
    }
}

