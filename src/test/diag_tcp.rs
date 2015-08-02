//! Run existing mydht Tests over a tcp transport (with true sockets)
//!

use transport::tcp::Tcp;
use utils::SocketAddrExt;
use utils;
use std::net::{Ipv4Addr,SocketAddr};
#[cfg(test)]
use transport::test::connect_rw_with_optional;
use time::Duration;


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

