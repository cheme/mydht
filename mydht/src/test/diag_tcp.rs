//! Run existing mydht Tests over a tcp transport (with true sockets)
//!
//!

extern crate mydht_tcp;
extern crate igd;

#[cfg(not(windows))]
extern crate libc;

#[cfg(not(windows))]
extern crate ipnetwork;
#[cfg(not(windows))]
use self::libc::{
  getifaddrs,
  freeifaddrs,
  ifaddrs,
  sockaddr,
  sockaddr_in,
  AF_INET,
};

#[cfg(not(windows))]
use self::ipnetwork::{
  ip_mask_to_prefix,
  Ipv4Network,
};

#[cfg(not(windows))]
use std::net::{IpAddr};

use procs::api::{
  DHTIn,
};
use std::mem;
use std::ptr;
use msgenc::json::Json;
use self::mydht_tcp::Tcp;
use utils;
use transport::{
  SerSocketAddr,
};
use std::net::{SocketAddrV4,Ipv4Addr};
#[cfg(test)]
use mydht_basetest::transport::connect_rw_with_optional;
use std::time::Duration;
use super::{
  initpeers2,
  DHTRULES_DEFAULT,
  ALLTESTMODE,
  finddistantpeer2,
  TestConf,
};
use rules::simplerules::{DhtRules};
#[cfg(test)]
use node::Node;

use peer::test::TestingRules;
use query::{QueryMode};
use rules::simplerules::SimpleRules;




fn initpeers_tcp2 (start_port : u16, nbpeer : usize, map : &[&[usize]], meths : TestingRules, rules : DhtRules, sim : Option<u64>)
  -> Vec<(Node, DHTIn<TestConf<Node,Tcp,Json,TestingRules,SimpleRules>>)> {
  let mut nodes = Vec::new();
  let mut transports = Vec::new();

  for i in 0 .. nbpeer {
    let addr = utils::sa4(Ipv4Addr::new(127,0,0,1), start_port + i as u16);
    let tcp_transport = Tcp::new(
      &addr,
      Duration::from_secs(5), // timeout
    //  Duration::seconds(5), // conn timeout
      true,//mult
    ).unwrap();
    transports.push(tcp_transport);
    nodes.push(Node {nodeid: "NodeID".to_string() + &(i + 1).to_string()[..], address : SerSocketAddr(addr)});
  };
  initpeers2(nodes, transports, map, meths, rules,Json,sim)
}



#[test]
fn connect_rw () {
  let start_port = 50000;

  let a1 = SerSocketAddr(utils::sa4(Ipv4Addr::new(127,0,0,1), start_port));
  let a2 = SerSocketAddr(utils::sa4(Ipv4Addr::new(127,0,0,1), start_port+1));
  let tcp_transport_1 : Tcp = Tcp::new (&a1, Duration::from_secs(5), true).unwrap();
  let tcp_transport_2 : Tcp = Tcp::new (&a2, Duration::from_secs(5), true).unwrap();

  connect_rw_with_optional(tcp_transport_1,tcp_transport_2,&a1,&a2,true,false);
}


#[cfg(windows)]
fn ip_addr_for_gateway (_gateway : &Ipv4Addr) -> Option<Ipv4Addr> {
  None
}
// dirty select (should use something from a designated crate)
#[cfg(not(windows))]
fn ip_addr_for_gateway (gateway : &Ipv4Addr) -> Option<Ipv4Addr> {
  let mut matches = 0;
  let mut res = None;

  let mut addresses: *mut ifaddrs = unsafe { mem::zeroed() };
  if unsafe { getifaddrs(&mut addresses as *mut _) } != 0 {
    return None;
	}
  let mut p_address: *mut ifaddrs = addresses;
  while p_address != ptr::null_mut() {
    let add = unsafe { *(*p_address).ifa_addr };
    if add.sa_family as i32 == AF_INET {
      let mask = unsafe { *(*p_address).ifa_netmask }.sa_data;
      let prefix = ip_mask_to_prefix(IpAddr::V4(Ipv4Addr::new(
            mask[2] as u8,
            mask[3] as u8,
            mask[4] as u8,
            mask[5] as u8,
      ))).unwrap();
      let add = add.sa_data;
      // may want to cast to sock_addr_in for possible different layout
      let address = Ipv4Addr::new(
            add[2] as u8,
            add[3] as u8,
            add[4] as u8,
            add[5] as u8,
      );
      if Ipv4Network::new(gateway.clone(),prefix).unwrap().network() == 
         Ipv4Network::new(address.clone(),prefix).unwrap().network() {
           res = Some(address);
           break;
      }
    }
  	p_address = unsafe { (*p_address).ifa_next };
	}
	unsafe { freeifaddrs(addresses) };
  res
}
#[test]
fn connect_rw_upnp () {
  let start_port = 50000;

//  let local_ip = Ipv4Addr::new(127,0,0,1);
  let mut local_ip = Ipv4Addr::new(0,0,0,0);
  // local v4 ip only (gateway from igd is v4)

  let a1 = SerSocketAddr(utils::sa4(local_ip.clone(), start_port));
  let a2 = SerSocketAddr(utils::sa4(local_ip.clone(), start_port+1));
  let gateway = igd::search_gateway_timeout(Duration::from_secs(5)).unwrap();
  let pub_ip = gateway.get_external_ip().unwrap();
  if let Some(a) = ip_addr_for_gateway(gateway.addr.ip()) {
    local_ip = a;
  }
  let pub_port1 = gateway.add_any_port(igd::PortMappingProtocol::TCP, SocketAddrV4::new(local_ip.clone(), start_port), 0, "test tcp upnp add1").unwrap();
  let pub_port2 = gateway.add_any_port(igd::PortMappingProtocol::TCP, SocketAddrV4::new(local_ip.clone(), start_port+1), 0, "test tcp upnp add2").unwrap();

  let d1 = SerSocketAddr(utils::sa4(pub_ip.clone(), pub_port1));
  let d2 = SerSocketAddr(utils::sa4(pub_ip.clone(), pub_port2));
  let tcp_transport_1 : Tcp = Tcp::new (&a1, Duration::from_secs(5), true).unwrap();
  let tcp_transport_2 : Tcp = Tcp::new (&a2, Duration::from_secs(5), true).unwrap();

  connect_rw_with_optional(tcp_transport_1,tcp_transport_2,&d1,&d2,true,false);
}



#[test]
fn connect_rw_dup () {
  let start_port = 50100;

  let a1 = SerSocketAddr(utils::sa4(Ipv4Addr::new(127,0,0,1), start_port));
  let a2 = SerSocketAddr(utils::sa4(Ipv4Addr::new(127,0,0,1), start_port+1));
  let tcp_transport_1 : Tcp = Tcp::new (&a1, Duration::from_secs(5), false).unwrap();
  let tcp_transport_2 : Tcp = Tcp::new (&a2, Duration::from_secs(5), false).unwrap();

  connect_rw_with_optional(tcp_transport_1,tcp_transport_2,&a1,&a2,false,false);
}

#[test]
fn testpeer4hopget () {
    let n = 6;
    let nbport = n as u16;
    let map : &[&[usize]] = &[&[2],&[3],&[4],&[5],&[6],&[]];
    let mut startport = 46450;
    // prio 2 with rules multiplying by 3 give 6 hops
    for m in ALLTESTMODE.iter() {
      let mut rules = DHTRULES_DEFAULT.clone();
      rules.nbhopfact = 3;
      let peers = initpeers_tcp2(startport,n, map, TestingRules::new_no_delay(), rules.clone(), None);
      finddistantpeer2(peers,n,(*m).clone(),2,map,true,SimpleRules::new(rules));
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
      let peers = initpeers_tcp2(startport,n, map, TestingRules::new_no_delay(), rules.clone(), None);
      finddistantpeer2(peers,n,(*m).clone(),1,map,false,SimpleRules::new(rules));
      startport = startport + nbport;
    };
}


