//! Run existing mydht Tests over a tcp transport (with true sockets)
//!
use msgenc::json::Json;
use msgenc::MsgEnc;
use DHT;
use transport::tcp::Tcp;
use utils::SocketAddrExt;
use utils;
use transport::Transport;
use std::net::{SocketAddr,Ipv4Addr};
#[cfg(test)]
use transport::test::connect_rw_with_optional;
use time::Duration;
use super::{
  initpeers,
  DHTRULES_DEFAULT,
  ALLTESTMODE,
};
use peer::node::Node;

use peer::test::TestingRules;
use query::{QueryConf,QueryMode,QueryChunk,QueryPriority};
use peer::PeerMgmtMeths;
use procs::RunningTypes;
use std::marker::PhantomData;
use rules::simplerules::SimpleRules;
use num::traits::ToPrimitive;
use keyval::KeyVal;

struct RunningTypesImpl<M : PeerMgmtMeths<Node, Node>, T : Transport, E : MsgEnc> (PhantomData<(M,T,E)>);

impl<M : PeerMgmtMeths<Node, Node>, T : Transport<Address=SocketAddr>, E : MsgEnc> RunningTypes for RunningTypesImpl<M, T, E> {
  type A = SocketAddr;
  type P = Node;
  type V = Node;
  type M = M;
  type R = SimpleRules;
  type E = E;
  type T = T;
}

fn finddistantpeer<M : PeerMgmtMeths<Node, Node> + Clone>  (startport : u16,nbpeer : usize, qm : QueryMode, meths : M, prio : QueryPriority, map : &[&[usize]], find : bool) {
    let peers = initpeers_tcp(startport,nbpeer, map, meths);
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


fn initpeers_tcp<M : PeerMgmtMeths<Node, Node> + Clone> (start_port : u16, nbpeer : usize, map : &[&[usize]], meths : M) -> Vec<(Node, DHT<RunningTypesImpl<M,Tcp,Json>>)> {
  let mut nodes = Vec::new();
  let mut transports = Vec::new();

  for i in 0 .. nbpeer {
    let addr = utils::sa4(Ipv4Addr::new(127,0,0,1), start_port + i.to_u16().unwrap());
    let tcp_transport = Tcp::new(
      &addr,
      Duration::seconds(5), // timeout
      Duration::seconds(5), // conn timeout
      true,//mult
    ).unwrap();
    transports.push(tcp_transport);
    nodes.push(Node {nodeid: "NodeID".to_string() + &(i + 1).to_string()[..], address : SocketAddrExt(addr)});
  };
  let mut rules = DHTRULES_DEFAULT.clone();
  // 9 hop
  rules.nbhopfact = 9;
  initpeers(nodes, transports, map, meths, rules,Json,false)
}

#[test]
fn connect_rw () {
  let start_port = 50000;

  let a1 = SocketAddrExt(utils::sa4(Ipv4Addr::new(127,0,0,1), start_port));
  let a2 = SocketAddrExt(utils::sa4(Ipv4Addr::new(127,0,0,1), start_port+1));
  let tcp_transport_1 : Tcp = Tcp::new (&a1, Duration::seconds(5), Duration::seconds(5), true).unwrap();
  let tcp_transport_2 : Tcp = Tcp::new (&a2, Duration::seconds(5), Duration::seconds(5), true).unwrap();

  connect_rw_with_optional(tcp_transport_1,tcp_transport_2,&a1,&a2,true);
}

#[test]
fn connect_rw_dup () {
  let start_port = 50100;

  let a1 = SocketAddrExt(utils::sa4(Ipv4Addr::new(127,0,0,1), start_port));
  let a2 = SocketAddrExt(utils::sa4(Ipv4Addr::new(127,0,0,1), start_port+1));
  let tcp_transport_1 : Tcp = Tcp::new (&a1, Duration::seconds(5), Duration::seconds(5), false).unwrap();
  let tcp_transport_2 : Tcp = Tcp::new (&a2, Duration::seconds(5), Duration::seconds(5), false).unwrap();

  connect_rw_with_optional(tcp_transport_1,tcp_transport_2,&a1,&a2,false);
}

#[test]
fn testPeer4hopget (){
    let n = 6;
    let map : &[&[usize]] = &[&[2],&[3],&[4],&[5],&[6],&[]];
    let mut startport = 46450;
    for m in ALLTESTMODE.iter() {
      finddistantpeer(startport,n,(*m).clone(),TestingRules::new_no_delay(),1,map,true);
      startport += 10;
    }
    // prio 2 max nb hop is 3  TODO now it is 2 * 9
    for m in ALLTESTMODE.iter() {
      finddistantpeer(startport,n,(*m).clone(),TestingRules::new_no_delay(),2,map,false);
      startport += 10;
    }
}

