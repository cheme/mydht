use procs::{ClientChanel};
use peer::{Peer, PeerPriority};
use keyval::KeyVal;
use procs::RunningProcesses;
use std::sync::Arc;
use std::rc::Rc;
use std::collections::VecDeque;
use mydhtresult::Result as MydhtResult;
use std::thread;
pub mod inefficientmap;

#[cfg(feature="dht-route")]
pub mod btkad;

// TODO refactor to got explicit add and rem chan plus prio
// eg : update with chan plus prio!!
// TODO refactor get closest to return connected closest plus a number of better to have non
// connected and then do some discovery on the better one (just query them).
/// Trait for storing peer information and implementing strategie to choose closest nodes for query
/// (either for querying a peer or a value).
/// Trait contains serializable content (Peer), but also trensiant content like channel to peer
/// client process.
/// TODO route to store enum over client chanel or direct writestream see clientinfo (will require
/// get_mut for info (as write stream need to be mut(or refcell usage))
/// TODO split route into composition of transient cache of ClientInfo and routing algo?? 
pub trait Route<P:Peer,V:KeyVal> : Send + 'static {
  /// count of running query (currently only updated in proxy mode)
  fn query_count_inc(& mut self, &P::Key);
  /// count of running query (currently only updated in proxy mode)
  fn query_count_dec(& mut self, &P::Key);
  /// add a peer
  fn add_node(& mut self, Arc<P>, Option<ClientChanel<P,V>>);
  /// change a peer prio (eg setting offline or normal...)
  fn update_priority(& mut self, &P::Key, PeerPriority);
  // TODO change
  /// get a peer info (peer, priority (eg offline), and existing channel to client process) 
  fn get_node(& self, &P::Key) -> Option<&(Arc<P>, PeerPriority, Option<ClientChanel<P,V>>)>;
  // remove chan for node TODO refactor to two kind of status and auto rem when offline or blocked
  /// remove channel to process (use when a client process broke or normal shutdown).
  fn remchan(&mut self, &P::Key);

  // TODO maybe return sender instead
  /// routing method to choose peer for a peer query (no offline or blocked peer)
  fn get_closest_for_node(& self, &P::Key, u8, &VecDeque<P::Key>) -> Vec<Arc<P>>;
  /// routing method to choose peer for a KeyVal query(no offline or blocked peer)
  fn get_closest_for_query(& self, &V::Key, u8, &VecDeque<P::Key>) -> Vec<Arc<P>>;
  // will be way better with an iterator so that for instance we could try to connect until 
  // no more or connection pool is fine
  // TODO refactor to using this box iterator (trait returned in box for cast)
  /// Get n peer even if offline
  fn get_pool_nodes(& self, usize) -> Vec<Arc<P>>;

  // TODO lot of missing params(queryconf, msg...) : change it when implementing (first good code
  // for light client separating concerns in fn).
  /// Interface allowing complex route implementation to run slow lookup then do the stuff in a
  /// continuation passing way.
  /// Typically a route like this should have a main thread for cache lookup (fast access to node),
  /// and thread(s) running slow closest node calculation, the main thread interface to them for
  /// get_closest (waiting for result), but for heavy_get_closest it do not have to wait for result
  /// since it is continuation passing design.
  ///
  /// Default implementation should simply panic, here instead it do a slow get_closest (same as
  /// slow one).
  fn heavy_get_closest_for_node<C,D>(& self, node : &P::Key, nb : u8, filter : &VecDeque<P::Key>, rc : &RunningProcesses<P,V>, each : C, adjustnb : D) 
    where C : Fn(&Arc<P>, &RunningProcesses<P, V>), 
          D : Fn(usize) {
       let vclo = self.get_closest_for_node(node, nb, filter);
       let s = vclo.len();
       adjustnb(s);
       for n in vclo.iter() {
         each(n, rc)
       }
  }
  
  // TODO lot of missing params(queryconf, msg...) : change it when implementing (first good code
  // for light client separating concerns in fn).
  fn heavy_get_closest_for_query<C,D>(& self, k : &V::Key, nb : u8, filter : &VecDeque<P::Key>, rc : &RunningProcesses<P,V>, each : C, adjustnb : D)
    where C : Fn(&Arc<P>, &RunningProcesses<P, V>), 
          D : Fn(usize) {
       let vclo = self.get_closest_for_query(k, nb, filter);
       let s = vclo.len();
       adjustnb(s);
       for n in vclo.iter() {
         each(n, rc)
       }
  }
  
  // TODO lot of missing params(queryconf, msg...) : change it when implementing (first good code
  // for light client separating concerns in fn).
  fn heavy_get_pool_nodes<C>(&self, nb : usize, rc : &RunningProcesses<P,V>, each : C) 
    where C : Fn(&Arc<P>, &RunningProcesses<P, V>) {
     let vclo = self.get_pool_nodes(nb);
     for n in vclo.iter() {
       each(n, rc)
     }
  }
 


  /// Possible Serialize on quit
  fn commit_store(& mut self) -> bool;
}

// offlines, get_pool_nodes returningoffline if needed.
#[cfg(test)]
mod test {
  use super::Route;
  use keyval::KeyVal;
  use std::sync::{Arc};
  use std::collections::VecDeque;
use peer::{Peer, PeerPriority};
  pub fn test_route<P:Peer,V:KeyVal,R:Route<P,V>> (peers : &[Arc<P>; 5], route : & mut R, valkey : V::Key) {
    let fpeer = peers[0].clone();
    let fkey = fpeer.get_key();
    assert!(route.get_node(&fkey).is_none());
    route.add_node(fpeer, None);
    assert!(route.get_node(&fkey).unwrap().0.get_key() == fkey);
    for p in peers.iter(){
      route.add_node(p.clone(), None);
    }
    assert!(route.get_node(&fkey).is_some());
    // all node are still off line
    assert!(route.get_closest_for_node(&fkey,1,&VecDeque::new()).len() == 0);
    assert!(route.get_closest_for_query(&valkey,1,&VecDeque::new()).len() == 0);
    for p in peers.iter(){
      route.update_priority(&p.get_key(), PeerPriority::Normal);
    }
    assert!(route.get_closest_for_node(&fkey,1,&VecDeque::new()).len() == 1);
    assert!(route.get_closest_for_query(&valkey,1,&VecDeque::new()).len() == 1);
    for p in peers.iter(){
      route.update_priority(&p.get_key(), PeerPriority::Priority(1));
    }
    let nb_fnode = route.get_closest_for_node(&fkey,10,&VecDeque::new()).len();
    assert!(nb_fnode > 0);
    assert!(nb_fnode < 6);
    for p in peers.iter(){
      route.update_priority(&p.get_key(), PeerPriority::Blocked);
    }
    assert!(route.get_closest_for_node(&fkey,1,&VecDeque::new()).len() == 0);
    assert!(route.get_closest_for_query(&valkey,1,&VecDeque::new()).len() == 0);
    assert!(route.get_node(&fkey).is_some());
    // blocked or offline should remove channel (no more process) TODO test it
    // assert!(route.get_node(&fkey).unwrap().2.is_none());
 
  }
} 
