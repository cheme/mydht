extern crate bit_vec;
use self::bit_vec::BitVec;
use mydht_base::route::byte_rep::{
  DHTElem,
};
use std::collections::VecDeque;
use std::sync::Arc;
use mydht_base::peer::{
  Peer,
  PeerState,
  PeerStateChange,
  PeerPriority,
};
use mydht_base::route::RouteBase;
use mydht_base::transport::{
  Transport,
  Address,
};
use mydht_base::keyval::KeyVal;


#[derive(Clone,PartialEq,Eq)]
pub struct KeyConflict {
  pub key : BitVec,
  pub content : BitVec,
}

impl DHTElem for KeyConflict {
  #[inline]
  fn bits_ref (& self) -> & BitVec {
    &self.key
  }
  #[inline]
  fn kelem_eq(&self, other : &Self) -> bool {
    self.content == other.content
  }
}

/// warning same as test route from mydht 
pub fn test_routebase<A:Address,P:Peer<Address = A>,V:KeyVal,T:Transport<Address = A>,R:RouteBase<A,P,V,T,CI,SI>,CI,SI> (peers : &[Arc<P>; 5], route : & mut R, valkey : V::Key) {
    let fpeer = peers[0].clone();
    let fkey = fpeer.get_key();
    assert!(route.get_node(&fkey).is_none());
    route.add_node((fpeer, PeerState::Offline(PeerPriority::Normal), (None,None)));
    assert!(route.get_node(&fkey).unwrap().0.get_key() == fkey);
    for p in peers.iter(){
      route.add_node((p.clone(), PeerState::Offline(PeerPriority::Normal), (None,None)));
    }
    assert!(route.get_node(&fkey).is_some());
    // all node are still off line
    assert!(route.get_closest_for_node(&fkey,1,&VecDeque::new()).len() == 0);
    assert!(route.get_closest_for_query(&valkey,1,&VecDeque::new()).len() == 0);
    for p in peers.iter(){
      route.update_priority(&p.get_key(), None, Some(PeerStateChange::Online));
    }
    assert!(route.get_closest_for_node(&fkey,1,&VecDeque::new()).len() == 1);
    assert!(route.get_closest_for_query(&valkey,1,&VecDeque::new()).len() == 1);
    for p in peers.iter(){
      route.update_priority(&p.get_key(), Some(PeerState::Online(PeerPriority::Priority(1))),None);
    }
    let nb_fnode = route.get_closest_for_node(&fkey,10,&VecDeque::new()).len();
    assert!(nb_fnode > 0);
    assert!(nb_fnode < 6);
    for p in peers.iter(){
      route.update_priority(&p.get_key(), Some(PeerState::Blocked(PeerPriority::Normal)),None);
    }
    assert!(route.get_closest_for_node(&fkey,1,&VecDeque::new()).len() == 0);
    assert!(route.get_closest_for_query(&valkey,1,&VecDeque::new()).len() == 0);
    assert!(route.get_node(&fkey).is_some());
    // blocked or offline should remove channel (no more process) TODO test it
    // assert!(route.get_node(&fkey).unwrap().2.is_none());
 
  }

