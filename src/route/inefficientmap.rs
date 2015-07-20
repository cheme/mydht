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

/// Routing structure based on map plus count of query for proxy mode
/// May also be used for testing
pub struct Inefficientmap<P : Peer, V : KeyVal> where P::Key : Ord + Hash {
    peersNbQuery : BTreeSet<(u8,P::Key)>, //TODO switch to collection ordered by nb query : highly ineficient when plus and minus on query
    peers : HashMap<P::Key, (Arc<P>, PeerPriority, Option<ClientChanel<P,V>>)>, //TODO maybe get node priority out of this container or do in place update
}


impl<P : Peer, V : KeyVal> Route<P,V> for Inefficientmap<P,V> where P::Key : Ord + Hash {
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
    self.peers.insert(node.get_key(), (node,PeerPriority::Offline, chan));
  }

  fn remchan(& mut self, nodeid : &P::Key) where P::Key : Send {
    let toadd = match self.peers.get(nodeid) {
      Some(&(_,_, None)) => {None},
      Some(&(ref ap,ref prio, ref s)) => {Some ((ap.clone(), prio.clone(), None))}, // TODO rewrite with in place write of hashmap (currently some issue with arc).
      None => {None},
    };
    match toadd {
      Some(v) => {
        self.peers.insert(nodeid.clone(),v);
      },
      None => (),
    };
  }


  fn update_priority(& mut self, nodeid : &P::Key, prio : PeerPriority) where P::Key : Send {
    /*         match self.peers.get_mut(nodeid) {
               Some(&mut (_,mut oldpri,_)) => oldpri = prio,
               None => {},
               };*/
    debug!("update prio of {:?} to {:?}",nodeid,prio);
    let toadd = match self.peers.get(nodeid) {
      Some(&(ref ap,_, ref s)) => {Some ((ap.clone(), prio, s.clone()))}, // TODO rewrite with in place write of hashmap (currently some issue with arc).
      None => {None},
    };
/*    match toadd {
      Some((_,_,None)) => println!("######## updating on no channel"),
      None => println!("######### updating on no value"),
        _ => println!("#### updating with xisting channel"),
    };*/
    match toadd {
      Some(v) => {
        self.peers.insert(nodeid.clone(),v);
      },
      None => (),
    };
  }

  fn get_node(& self, nid : &P::Key) -> Option<&(Arc<P>, PeerPriority, Option<ClientChanel<P,V>>)>  {
    self.peers.get(nid)
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
      match self.peers.get(&nid.1) {
        // no status check this method is usefull for refreshing or initiating
        // connections
        Some(&(ref ap,ref prio, ref s)) => {
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

impl<P:Peer,V:KeyVal> Inefficientmap<P,V> where P::Key : Ord + Hash {

  pub fn new() -> Inefficientmap<P,V> {
    Inefficientmap{ peersNbQuery : BTreeSet::new(), peers : HashMap::new()}
  }

  fn get_closest(& self, nbnode : u8, filter : &VecDeque<P::Key>) -> Vec<Arc<P>> {
    let mut r = Vec::new();
    let mut i = 0;
    // with the nb of query , priority not use
    for nid in self.peersNbQuery.iter() {
      debug!("!!!in closest node {:?}", nid);
      match self.peers.get(&nid.1) {
        Some(&(ref ap,PeerPriority::Normal, ref s)) | Some(&(ref ap,PeerPriority::Priority(_), ref s)) => {
          debug!("found");
          if filter.iter().find(|r|**r == ap.get_key()) == None {
            r.push(ap.clone());
            i = i + 1;
          }
        },
        Some(&(ref ap,PeerPriority::Offline, ref s)) => {
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

#[test]
fn test(){
  let mut route : Inefficientmap<Node, Node> = Inefficientmap::new();

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
