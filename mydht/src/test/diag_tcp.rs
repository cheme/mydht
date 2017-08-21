//! Run existing mydht Tests over a tcp transport (with true sockets)
//!
//!

extern crate mydht_tcp;
use msgenc::json::Json;
use msgenc::MsgEnc;
use DHT;
use self::mydht_tcp::Tcp;
use utils;
use transport::{
  Transport,
  SerSocketAddr,
};
use std::net::{SocketAddr,Ipv4Addr};
#[cfg(test)]
use mydht_basetest::transport::connect_rw_with_optional;
use time::Duration;
use super::{
  initpeers,
  DHTRULES_DEFAULT,
  ALLTESTMODE,
  finddistantpeer,
};
use rules::simplerules::{DhtRules};
#[cfg(test)]
use node::Node;

use peer::test::TestingRules;
use query::{QueryConf,QueryMode,QueryChunk,QueryPriority};
use peer::PeerMgmtMeths;
use procs::RunningTypes;
use std::marker::PhantomData;
use rules::simplerules::SimpleRules;
use num::traits::ToPrimitive;
use keyval::KeyVal;

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



fn initpeers_tcp<M : PeerMgmtMeths<Node, Node> + Clone> (start_port : u16, nbpeer : usize, map : &[&[usize]], meths : M, rules : DhtRules, sim : Option<u32>) -> Vec<(Node, DHT<RunningTypesImpl<M,Tcp,Json>>)> {
  let mut nodes = Vec::new();
  let mut transports = Vec::new();

  for i in 0 .. nbpeer {
    let addr = utils::sa4(Ipv4Addr::new(127,0,0,1), start_port + i.to_u16().unwrap());
    let tcp_transport = Tcp::new(
      &addr,
      Duration::seconds(5), // timeout
    //  Duration::seconds(5), // conn timeout
      true,//mult
    ).unwrap();
    transports.push(tcp_transport);
    nodes.push(Node {nodeid: "NodeID".to_string() + &(i + 1).to_string()[..], address : SerSocketAddr(addr)});
  };
  initpeers(nodes, transports, map, meths, rules,Json,sim)
}

#[test]
fn connect_rw () {
  let start_port = 50000;

  let a1 = SerSocketAddr(utils::sa4(Ipv4Addr::new(127,0,0,1), start_port));
  let a2 = SerSocketAddr(utils::sa4(Ipv4Addr::new(127,0,0,1), start_port+1));
  let tcp_transport_1 : Tcp = Tcp::new (&a1, Duration::seconds(5), true).unwrap();
  let tcp_transport_2 : Tcp = Tcp::new (&a2, Duration::seconds(5), true).unwrap();

  connect_rw_with_optional(tcp_transport_1,tcp_transport_2,&a1,&a2,true,false);
}

#[test]
fn connect_rw_dup () {
  let start_port = 50100;

  let a1 = SerSocketAddr(utils::sa4(Ipv4Addr::new(127,0,0,1), start_port));
  let a2 = SerSocketAddr(utils::sa4(Ipv4Addr::new(127,0,0,1), start_port+1));
  let tcp_transport_1 : Tcp = Tcp::new (&a1, Duration::seconds(5), false).unwrap();
  let tcp_transport_2 : Tcp = Tcp::new (&a2, Duration::seconds(5), false).unwrap();

  connect_rw_with_optional(tcp_transport_1,tcp_transport_2,&a1,&a2,false,false);
}

#[test]
fn testpeer4hopget () {
    let n = 6;
    let nbport = n.to_u16().unwrap();
    let map : &[&[usize]] = &[&[2],&[3],&[4],&[5],&[6],&[]];
    let mut startport = 46450;
    // prio 2 with rules multiplying by 3 give 6 hops
    for m in ALLTESTMODE.iter() {
      let mut rules = DHTRULES_DEFAULT.clone();
      rules.nbhopfact = 3;
      let peers = initpeers_tcp(startport,n, map, TestingRules::new_no_delay(), rules, None);
      finddistantpeer(peers,n,(*m).clone(),2,map,true);
      startport = startport + 4 * nbport; // 4 * is useless but due to big nb of open port (both way) it may help some still open port.
    };
   
    // prio 1 max nb hop is 3
    for m in ALLTESTMODE.iter() {
      let mut rules = DHTRULES_DEFAULT.clone();
      rules.nbhopfact = 3;
      // for asynch we need only one nbquer (expected to many none otherwhise)
      if let &QueryMode::Asynch = m {
        rules.nbqueryfact = 0.0; // nb query is 1 + prio * nbqfact
      };
      let peers = initpeers_tcp(startport,n, map, TestingRules::new_no_delay(), rules, None);
      finddistantpeer(peers,n,(*m).clone(),1,map,false);
      startport = startport + nbport;
    };
}


