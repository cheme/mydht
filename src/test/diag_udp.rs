//! Run existing mydht Tests over a udp transport (with true sockets)
//!

use transport::udp::Udp;
use utils::SocketAddrExt;
use utils;
use std::net::{Ipv4Addr,SocketAddr};
#[cfg(test)]
use transport::test::connect_rw_with_optional_non_managed;
use time::Duration;


#[test]
fn connect_rw () {
  let start_port = 60000;

  let a1 = SocketAddrExt(utils::sa4(Ipv4Addr::new(127,0,0,1), start_port));
  let a2 = SocketAddrExt(utils::sa4(Ipv4Addr::new(127,0,0,1), start_port+1));
  let tcp_transport_1 : Udp = Udp::new (&a1, 500, true).unwrap();
  let tcp_transport_2 : Udp = Udp::new (&a2, 500, true).unwrap();

  connect_rw_with_optional_non_managed(tcp_transport_1,tcp_transport_2,&a1,&a2,false);
}

#[test]
fn connect_rw_nospawn () {
  let start_port = 60100;

  let a1 = SocketAddrExt(utils::sa4(Ipv4Addr::new(127,0,0,1), start_port));
  let a2 = SocketAddrExt(utils::sa4(Ipv4Addr::new(127,0,0,1), start_port+1));
  let tcp_transport_1 : Udp = Udp::new (&a1, 500, false).unwrap();
  let tcp_transport_2 : Udp = Udp::new (&a2, 500, false).unwrap();

  connect_rw_with_optional_non_managed(tcp_transport_1,tcp_transport_2,&a1,&a2,false);
}

