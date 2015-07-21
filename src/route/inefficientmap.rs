use std::collections::{HashMap,BTreeSet,VecDeque};
use peer::{Peer,PeerPriority};
use procs::{ClientChanel};
use std::sync::{Arc};
use std::sync::mpsc::{Sender,Receiver};
use route::Route;
use std::iter::Iterator;
use std::rc::Rc;
use keyval::KeyVal;
use std::hash::Hash;
use kvcache::KVCache;
use std::marker::PhantomData;
use transport::Transport;
use super::PeerInfo;
use super::{pi_remchan,pi_upprio};


/// Routing structure based on map plus count of query for proxy mode
/// May also be used for testing
pub struct Inefficientmap<P : Peer, V : KeyVal, T : Transport, C : KVCache<P::Key, PeerInfo<P,V,T>>> where P::Key : Ord + Hash {
    peersNbQuery : BTreeSet<(u8,P::Key)>, //TODO switch to collection ordered by nb query : highly ineficient when plus and minus on query
//    peers : HashMap<P::Key, PeerInfo<P,V>>, //TODO maybe get node priority out of this container or do in place update
    peers : C, //TODO maybe get node priority out of this container or do in place update
    _phdat : PhantomData<(V,T)>,
}


impl<P : Peer, V : KeyVal, T : Transport, C : KVCache<P::Key, PeerInfo<P,V,T>>> Route<P,V,T> for Inefficientmap<P,V,T,C> where P::Key : Ord + Hash {
  fn query_count_inc(& mut self, pnid : &P::Key) {
    let val = match self.peersNbQuery.iter().filter(|&&(ref nb,ref nid)| (*pnid) == (*nid) ).next() {
      Some(inid) => Some(inid.clone()),
      None => None,
    };
    match val {
      Some(inid) => {
        self.peersNbQuery.remove(&inid);
        self.peersNbQuery.insert((inid.0+1,inid.1));
      },
      None => {self.peersNbQuery.insert((1,(*pnid).clone()));}
    };
  }

  fn query_count_dec(& mut self, pnid : &P::Key) {
    let val = match self.peersNbQuery.iter().filter(|&&(ref nb,ref nid)| (*pnid) == (*nid) ).next() {
      Some(inid) => Some(inid.clone()),
      None => None,
    };
    match val {
      Some(inid) => {
        self.peersNbQuery.remove(&inid);
        self.peersNbQuery.insert((inid.0-1,inid.1));
      },
      None => {self.peersNbQuery.insert((0,(*pnid).clone()));}
    };
  }

  fn add_node(& mut self, node : Arc<P>, chan : Option<ClientChanel<P,V>>) {
    self.peersNbQuery.insert((0,node.get_key()));
    self.peers.add_val_c(node.get_key(), (node,PeerPriority::Offline, chan, PhantomData));
  }

  fn remchan(& mut self, nodeid : &P::Key) where P::Key : Send {
    // TODO result management
    self.peers.update_val_c(nodeid,pi_remchan).unwrap();
  }

  fn update_priority(& mut self, nodeid : &P::Key, prio : PeerPriority) where P::Key : Send {
    debug!("update prio of {:?} to {:?}",nodeid,prio);
    self.peers.update_val_c(nodeid,|v|pi_upprio(v,prio)).unwrap();
  }

  fn get_node(& self, nid : &P::Key) -> Option<&PeerInfo<P,V,T>> {
    self.peers.get_val_c(nid)
  }

  fn get_closest_for_query(& self, nnid : &V::Key, nbnode : u8, filter : &VecDeque<P::Key>) -> Vec<Arc<P>> {
    self.get_closest(nbnode, filter)
  }

  fn get_closest_for_node(& self, nnid : &P::Key, nbnode : u8, filter : &VecDeque<P::Key>) -> Vec<Arc<P>> {
    self.get_closest(nbnode, filter)
  }

  fn get_pool_nodes(& self, nbnode : usize) -> Vec<Arc<P>> {
    let mut r = Vec::new();
    let mut i = 0;
    // here by closest but not necessary could be random (ping may define closest)
    for nid in self.peersNbQuery.iter() {
      match self.peers.get_val_c(&nid.1) {
        // no status check this method is usefull for refreshing or initiating
        // connections
        Some(&(ref ap,ref prio, ref s,_)) => {
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


}

impl<P : Peer, V : KeyVal, T : Transport> Inefficientmap<P,V,T,HashMap<P::Key, PeerInfo<P,V,T>>> where P::Key : Ord + Hash {
  #[inline]
  pub fn new() -> Inefficientmap<P,V,T,HashMap<P::Key, PeerInfo<P,V,T>>> {
    Self::new_with_cache(HashMap::new())
  }
}

impl<P : Peer, V : KeyVal, T : Transport, C : KVCache<P::Key, PeerInfo<P,V,T>>> Inefficientmap<P,V,T,C> where P::Key : Ord + Hash {
  pub fn new_with_cache(c : C) -> Inefficientmap<P,V,T,C> {
    Inefficientmap{ peersNbQuery : BTreeSet::new(), peers : c, _phdat : PhantomData}
  }

  fn get_closest(& self, nbnode : u8, filter : &VecDeque<P::Key>) -> Vec<Arc<P>> {
    let mut r = Vec::new();
    let mut i = 0;
    // with the nb of query , priority not use
    for nid in self.peersNbQuery.iter() {
      debug!("!!!in closest node {:?}", nid);
      match self.peers.get_val_c(&nid.1) {
        Some(&(ref ap,PeerPriority::Normal, ref s,_)) | Some(&(ref ap,PeerPriority::Priority(_), ref s,_)) => {
          debug!("found");
          if filter.iter().find(|r|**r == ap.get_key()) == None {
            r.push(ap.clone());
            i = i + 1;
          }
        },
        Some(&(ref ap,PeerPriority::Offline, ref s,_)) => {
          debug!("found offline");
          warn!("more prioritary node not send")
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
  use super::super::Route;
  use super::Inefficientmap;
  use super::super::test;
  use keyval::KeyVal;
  use std::sync::{Arc};
  use std::collections::VecDeque;
  use peer::node::{Node};
  use utils;
  use utils::SocketAddrExt;
  use std::net::{Ipv4Addr,SocketAddr};
  use transport::tcp::Tcp;

#[test]
fn test(){
  let mut route : Inefficientmap<Node, Node,Tcp,_> = Inefficientmap::new();

  let nodes = [
    Arc::new(Node{nodeid:"test_id_1".to_string(), address: SocketAddrExt(utils::sa4(Ipv4Addr::new(127,0,0,1), 8080))}),
    Arc::new(Node{nodeid:"test_id_2".to_string(), address: SocketAddrExt(utils::sa4(Ipv4Addr::new(127,0,0,1), 8080))}),
    Arc::new(Node{nodeid:"test_id_3".to_string(), address: SocketAddrExt(utils::sa4(Ipv4Addr::new(127,0,0,1), 8080))}),
    Arc::new(Node{nodeid:"test_id_4".to_string(), address: SocketAddrExt(utils::sa4(Ipv4Addr::new(127,0,0,1), 8080))}),
    Arc::new(Node{nodeid:"test_id_5".to_string(), address: SocketAddrExt(utils::sa4(Ipv4Addr::new(127,0,0,1), 8080))}),
  ];
  let kv = "dummy".to_string();

  test::test_route(& nodes, & mut route, kv);
}
} 
