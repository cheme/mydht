
//use bit_vec::BitVec;
//use std::io::Result as IoResult;
//use mydht_base::mydhtresult::{ErrorKind,Error};

use std::collections::{HashMap,VecDeque,BTreeSet};
use mydht_base::peer::{Peer,PeerState,PeerStateChange};
//use mydht_base::procs::{ClientChanel};
use std::sync::{Arc};
//use std::sync::mpsc::{Sender,Receiver};
use mydht_base::route::RouteBase;
use mydht_base::route::is_peerinfo_online;
use std::iter::Iterator;
//use std::rc::Rc;
use mydht_base::keyval::KeyVal;
//use mydht_base::keyval::{Attachment,SettableAttachment};
use std::hash::Hash;
use mydht_base::transport::{Transport,Address};
use mydht_base::route::PeerInfoBase;
use mydht_base::kvcache::{
  KVCache,
  Cache,
};
use std::marker::PhantomData;
use mydht_base::route::{pi_upprio_base};
use mydht_base::mydhtresult::Result as MydhtResult;

use std::borrow::Borrow;
use mydht_base::route::RouteFromBase;
use mydht_base::route2::{
  GetPeerRef,
  RouteBaseMessage,
  RouteBase as RouteBase2,
  RouteMsgType,
};


/// Routing structure based on map plus count of query for proxy mode
/// May also be used for testing
pub struct InefficientmapBase<P : Peer, V : KeyVal, T : Transport, C : KVCache<P::Key, PeerInfoBase<P,SI,CI>>,CI,SI> where P::Key : Ord + Hash {
    peers_nbquery : BTreeSet<(u8,P::Key)>, //TODO switch to collection ordered by nb query : highly ineficient when plus and minus on query
//    peers : HashMap<P::Key, PeerInfo<P,V>>, //TODO maybe get node priority out of this container or do in place update
    peers : C, //TODO maybe get node priority out of this container or do in place update
    _phdat : PhantomData<(V,T,CI,SI)>,
}

/// simply iterate on cache
pub struct InefficientmapBase2<P : Peer,RP : Borrow<P>, GP : GetPeerRef<P,RP>, C : Cache<<P as KeyVal>::Key,GP>> {
   peers_nbquery : Vec<(P::Key,Option<usize>)>,
   next : usize,
   peers : C, //TODO maybe get node priority out of this container or do in place update
   _phdat : PhantomData<(RP,GP)>,
}
impl<P : Peer,RP : Borrow<P>, GP : GetPeerRef<P,RP>, C : Cache<<P as KeyVal>::Key,GP>> 
 InefficientmapBase2<P,RP,GP,C>
{
  pub fn new(c : C) -> Self {
    InefficientmapBase2 {
      peers_nbquery : Vec::new(),
      next : 0,
      peers : c,
      _phdat : PhantomData,
    }
  }
}
impl<P : Peer,RP : Borrow<P>, GP : GetPeerRef<P,RP>, C : Cache<<P as KeyVal>::Key,GP>>
InefficientmapBase2<P,RP,GP,C> {

  fn get_closest(&mut self, nbnode : usize, filter : Option<&VecDeque<P::Key>>) -> Vec<usize> {
    let mut r = Vec::new();
    if self.peers_nbquery.len() == 0 {
      return r
    }
    let st_ix = self.next;
    let mut i = 0;
    // with the nb of query , priority not use
    loop {
      let nid = self.peers_nbquery.get(self.next).unwrap();
      debug!("!!!in closest node {:?}", nid);
      let mut filtered = false;
      if let Some(ref filter) = filter {
        if filter.iter().find(|r|**r == nid.0) != None {
          filtered = true
        }
      }
      if !filtered {
        if let Some(ws) = nid.1 {
          r.push(ws);
          i += 1;
        }
      }
      if self.next == 0 {
        self.next = self.peers_nbquery.len() - 1;
      } else {
        self.next -= 1;
      }
      if self.next == st_ix {
        break
      }
      if i == nbnode {
        break
      }
    };
    debug!("Closest found {:?}", r);
    r
  }

}


impl<P : Peer,RP : Borrow<P>,GP : GetPeerRef<P,RP>, C : Cache<<P as KeyVal>::Key,GP>>
  RouteBase2<P,RP,GP> for InefficientmapBase2<P,RP,GP,C> {
  fn route_base<MSG : RouteBaseMessage<P>>(&mut self, nb : usize, m : MSG, _ : RouteMsgType) -> MydhtResult<(MSG,Vec<usize>)> {
    let closest = {
      let filter = m.get_filter();
      self.get_closest(nb,filter)
    };
    Ok((m,closest))
  }
}
 
impl<P : Peer,RP : Borrow<P>,GP : GetPeerRef<P,RP>, C : Cache<<P as KeyVal>::Key,GP>>
  Cache<<P as KeyVal>::Key,GP> for InefficientmapBase2<P,RP,GP,C> {
  fn add_val_c(& mut self, k : <P as KeyVal>::Key, v : GP) {
    // warn common mistake to forget that add val can also remove
    if self.peers.has_val_c(&k) {
      if let Some(ix) = self.peers_nbquery.iter().position(|nid|nid.0 == k) {
        self.peers_nbquery.remove(ix);
      }
    }
    self.peers_nbquery.push({
      let (p,ows) = v.get_peer_ref();
      (p.get_key(),ows)
    });
    self.peers.add_val_c(k,v)
  }
  fn get_val_c<'a>(&'a self, k : &<P as KeyVal>::Key) -> Option<&'a GP> {
    self.peers.get_val_c(k)
  }
  fn get_val_mut_c<'a>(&'a mut self, k : &<P as KeyVal>::Key) -> Option<&'a mut GP> {
    self.peers.get_val_mut_c(k)
  }
  fn has_val_c<'a>(&'a self, k : &<P as KeyVal>::Key) -> bool {
    self.peers.has_val_c(k)
  }
  fn remove_val_c(& mut self, k : &<P as KeyVal>::Key) -> Option<GP> {
    if let Some(ix) = self.peers_nbquery.iter().position(|nid|nid.0 == *k) {
      self.peers_nbquery.remove(ix);
    }
    self.peers.remove_val_c(k)
  }
}



impl<A : Address, P : Peer<Address = A>, V : KeyVal, T : Transport<Address = A>, C : KVCache<P::Key, PeerInfoBase<P,SI,CI>>,CI,SI> RouteBase<A,P,V,T,CI,SI> for InefficientmapBase<P,V,T,C,CI,SI> where P::Key : Ord + Hash {
  fn query_count_inc(& mut self, pnid : &P::Key) {
    let val = match self.peers_nbquery.iter().filter(|&&(_,ref nid)| (*pnid) == (*nid) ).next() {
      Some(inid) => Some(inid.clone()),
      None => None,
    };
    match val {
      Some(inid) => {
        self.peers_nbquery.remove(&inid);
        self.peers_nbquery.insert((inid.0+1,inid.1));
      },
      None => {self.peers_nbquery.insert((1,(*pnid).clone()));}
    };
  }

  fn query_count_dec(& mut self, pnid : &P::Key) {
    let val = match self.peers_nbquery.iter().filter(|&&(_,ref nid)| (*pnid) == (*nid) ).next() {
      Some(inid) => Some(inid.clone()),
      None => None,
    };
    match val {
      Some(inid) => {
        self.peers_nbquery.remove(&inid);
        self.peers_nbquery.insert((inid.0-1,inid.1));
      },
      None => {self.peers_nbquery.insert((0,(*pnid).clone()));}
    };
  }

  fn add_node(& mut self, pi : PeerInfoBase<P,SI,CI>) {
    self.peers_nbquery.insert((0,pi.0.get_key()));
    self.peers.add_val_c(pi.0.get_key(), pi);
  }



  fn update_priority(& mut self, nodeid : &P::Key, opri : Option<PeerState>, och : Option<PeerStateChange>) where P::Key : Send {
    debug!("update prio of {:?} to {:?} , {:?}",nodeid,opri,och);
    self.peers.update_val_c(nodeid,|v|pi_upprio_base(v,opri,och)).unwrap();
  }

  fn update_infos<F>(&mut self, nodeid : &P::Key, f : F) -> MydhtResult<bool> where F : FnOnce(&mut (Option<SI>, Option<CI>)) -> MydhtResult<()> {
    self.peers.update_val_c(nodeid,|&mut (_,_,ref mut sici)|{
      f(sici)
    })
  }

  fn get_node(& self, nid : &P::Key) -> Option<&PeerInfoBase<P,SI,CI>> {
    self.peers.get_val_c(nid)
  }
  fn has_node(& self, nid : &P::Key) -> bool {
    self.peers.has_val_c(nid)
  }


  fn get_closest_for_query(& self, _ : &V::Key, nbnode : u8, filter : &VecDeque<P::Key>) -> Vec<Arc<P>> {
    self.get_closest(nbnode, filter)
  }

  fn get_closest_for_node(& self, _ : &P::Key, nbnode : u8, filter : &VecDeque<P::Key>) -> Vec<Arc<P>> {
    self.get_closest(nbnode, filter)
  }

  fn get_pool_nodes(& self, nbnode : usize) -> Vec<Arc<P>> {
    let mut r = Vec::new();
    let mut i = 0;
    // here by closest but not necessary could be random (ping may define closest)
    for nid in self.peers_nbquery.iter() {
      match self.peers.get_val_c(&nid.1) {
        // no status check this method is usefull for refreshing or initiating
        // connections
        Some(&(ref ap,ref prio, _)) => {
          println!("found {:?}", prio);
          r.push(ap.clone());
          i = i + 1;
        },
          _ => (),
      };
      if i == nbnode {break;}
    };
    r
  }


  fn commit_store(& mut self) -> bool{
    // TODO implement with a serialize and a path
    true
  }

  fn next_random_peers(&mut self, nb : usize) -> Vec<Arc<P>> {
    self.peers.next_random_values(nb,Some(&is_peerinfo_online))
      .into_iter().map(|p|(p.0).clone()).collect()
  }


} 

pub type Inefficientmap<A,P,V,T,HM,CI,SI> = RouteFromBase<A,P,V,T,CI,SI,InefficientmapBase<P,V,T,HM,CI,SI>>;

  #[inline]
  pub fn new
  <A:Address,P:Peer<Address = A>,V:KeyVal,T:Transport<Address = A>,CI,SI> 
  () -> Inefficientmap<A,P,V,T,HashMap<P::Key, PeerInfoBase<P,SI,CI>>,CI,SI> 
   where P::Key : Ord + Hash {
    new_with_cache(HashMap::new())
  }


  pub fn new_with_cache
  <A:Address,P:Peer<Address = A>,V:KeyVal,T:Transport<Address = A>, C : KVCache<P::Key, PeerInfoBase<P,SI,CI>>,CI,SI> 
  (c : C) -> Inefficientmap<A,P,V,T,C,CI,SI> 
   where P::Key : Ord + Hash {
    RouteFromBase(InefficientmapBase{ peers_nbquery : BTreeSet::new(), peers : c, _phdat : PhantomData}, PhantomData)
  }


impl
  <A:Address,P:Peer<Address = A>,V:KeyVal,T:Transport<Address = A>, C : KVCache<P::Key, PeerInfoBase<P,SI,CI>>,CI,SI> 
 InefficientmapBase<P,V,T,C,CI,SI> where P::Key : Ord + Hash {

  fn get_closest(& self, nbnode : u8, filter : &VecDeque<P::Key>) -> Vec<Arc<P>> {
    let mut r = Vec::new();
    let mut i = 0;
    // with the nb of query , priority not use
    for nid in self.peers_nbquery.iter() {
      debug!("!!!in closest node {:?}", nid);
      match self.peers.get_val_c(&nid.1) {
        Some(&(ref ap,PeerState::Online(_),_)) => {
          debug!("found");
          if filter.iter().find(|r|**r == ap.get_key()) == None {
            r.push(ap.clone());
            i = i + 1;
          }
        },
        Some(&(ref ap,PeerState::Offline(_),_)) => {
          warn!("high priority node (get_closest) not used (offline) : {:?}", ap);
        },
        _ => (),
      };
      if i == nbnode {break;}
    };
    debug!("Closest found {:?}", r);
    r
  }

}


#[cfg(test)]
mod test {
  extern crate rustc_serialize;
  extern crate mydht_basetest;
  extern crate num;
  extern crate rand;


  //use mydht_base::route::RouteBase;
  use super::Inefficientmap;
  use self::mydht_basetest::route::test_routebase;
  //use mydht_base::keyval::KeyVal;
  use std::sync::{Arc};
  //use std::collections::VecDeque;
  use self::mydht_basetest::node::{Node,NodeID};
  //use self::num::{BigUint};
  //use self::num::bigint::RandBigInt;
  //use std::net::{SocketAddr};
  //use mydht_base::utils::{SocketAddrExt};
  //use mydht_base::utils;
  //use std::io::Result as IoResult;
  //use mydht_base::peer::Peer;
  use self::rand::Rng;
  use self::mydht_basetest::local_transport::TransportTest;
  use self::mydht_basetest::transport::LocalAdd;
  use self::mydht_basetest::peer::PeerTest;
  use self::mydht_basetest::shadow::ShadowModeTest;

// TODO a clean nodeK, with better serialize (use as_vec) , but here good for testing as key is not
// same type as id
//#[derive(RustcDecodable,RustcEncodable,Debug,PartialEq,Eq,Clone)]
//pub struct NodeK(Node,BigUint);
type NodeK = Node;

const HASH_SIZE : usize = 160;

fn random_id(hash_size : usize) -> NodeID {
  let mut rng = rand::thread_rng();
  let mut res : Vec<u8> = vec!(0;hash_size / 8);
  rng.fill_bytes(&mut res[..]);
  unsafe{
    String::from_utf8_unchecked(res)
  }
}

fn initpeer() -> Arc<PeerTest> {
  let id = random_id(HASH_SIZE);
//    let sid = to_str_radix(id,10);
  Arc::new(PeerTest{nodeid:id, address: LocalAdd(0), keyshift: 1,
   modeshauth : ShadowModeTest::NoShadow,
   modeshmsg : ShadowModeTest::SimpleShift,
  })
}


#[test]
fn test(){
  let mut route : Inefficientmap<LocalAdd,PeerTest,PeerTest,TransportTest,_,u8,u8> = super::new();
    let nodes  = [
    initpeer(),
    initpeer(),
    initpeer(),
    initpeer(),
    initpeer(),
    ];
  let kv = random_id(HASH_SIZE).to_string();

  test_routebase(&nodes, & mut route.0, kv);
}

} 
