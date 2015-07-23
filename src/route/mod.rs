use procs::{ClientChanel};
use std::sync::Mutex;
use utils::send_msg;
use procs::mesgs::{ClientMessage,ClientMessageIx};
use std::sync::mpsc::{Sender};
use peer::{Peer, PeerPriority,PeerState,PeerStateChange};
use keyval::KeyVal;
use procs::RunningProcesses;
use std::sync::Arc;
use std::rc::Rc;
use std::collections::VecDeque;
use mydhtresult::Result as MydhtResult;
use std::thread;
use procs::RunningTypes;
use transport::{Address,Transport,WriteTransportStream};
use std::marker::PhantomData;
use std::ops::Drop;

pub mod inefficientmap;

#[cfg(feature="dht-route")]
pub mod btkad;

//pub type PeerInfo<P,V,T> = (Arc<P>, PeerPriority, Option<ClientChanel<P,V>>,PhantomData<T>);
/// Stored info about client in peer management (in route transient cache).
pub enum ClientInfo<P : Peer, V : KeyVal, T : Transport> {
  /// Stream is used locally
  Local(T::WriteStream),
  /// usize is only useful for client thread shared
  Threaded(Sender<ClientMessageIx<P,V>>,usize),

}
/// Drop implementation is really needed as it may close thread : part of the work of ending client
/// info is done by fn close_client from peermanager or a client thread and part is done at drop:
/// clientinfo may be shared in other process (when threaded not local of course) : in this case
/// the client shutdown message must be send at drop and not at peermanager removal.
impl<P : Peer, V : KeyVal, T : Transport>  Drop for ClientInfo<P,V,T> {
    fn drop(&mut self) {
        debug!("Drop of client info");
        match *self {
          ClientInfo::Local(_) => (),
          ClientInfo::Threaded(ref s,ref ix) => {s.send((ClientMessage::ShutDown,*ix));()},
        }
    }
}

/// TODO change to JoinHandle?? (no way to drop thread could try to park and drop Thread handle : a
/// park thread with no in scope handle should be swiped) TODO drop over unsafe mut cast of thread
/// pb is stop thread will fail since blocked on transport receive -> TODO post how to on stack!!
/// and for now just Arcmutex an exit bool
pub enum ServerInfo {
  /// transport manage the server, disconnection with transport primitive (calling remove on
  /// transport with peer address (alwayse called).
  TransportManaged,
  /// a thread exists (instanciated from transport reception or from peermgmt on connect)
  /// We keep a reference to end it on client side failure or simply peer removal.
  Threaded(Arc<Mutex<bool>>),
  // TODO add server pool messages
}

//pub type PeerInfo<P : Peer, V : KeyVal, T : Transport> = (Arc<P>,PeerState,Option<ServerInfo>, Option<ClientInfo<P,V,T>>);
pub type PeerInfo<P, V, T> = (Arc<P>,PeerState,Option<ServerInfo>, Option<ClientInfo<P,V,T>>);

impl<P : Peer, V : KeyVal, T : Transport> ClientInfo<P,V,T> {
   pub fn send_msg_local(&mut self,  msg : ClientMessage<P,V>) -> MydhtResult<()> {
    match self {
      &mut ClientInfo::Local(ref mut writer) => {
        // TODO all needed to start client fn exec
        Ok(())

      },
      &mut ClientInfo::Threaded(ref s,ref ix) => {
        try!(s.send((msg,ix.clone())));
        Ok(())
      },

    }
  }
 
  pub fn send_msg(&self,  msg : ClientMessage<P,V>) -> MydhtResult<()> {
    match self {
      &ClientInfo::Local(_) => {
        panic!("Trying to send local message under a non local config");

      },
      &ClientInfo::Threaded(ref s,ref ix) => {
        try!(s.send((msg,ix.clone())));
        Ok(())
      },

    }
  }
}

impl ServerInfo {

  /// could be call on ended shutdown
  fn shutdown<A : Address, T : Transport<Address = A>, P : Peer<Address = A>>(&self, t : & T, p : & P) -> MydhtResult<()> {
    match self {
      &ServerInfo::TransportManaged => {
         try!(t.disconnect(&p.to_address()));
  
      },
      &ServerInfo::Threaded(ref mutstop) => {
        // TODO move that to drop implementation of serverInfo!!!
        match mutstop.lock() {
          Ok(mut res) => *res = true,
          Err(m) => error!("poisoned mutex on server shutdown"),
        };
      },
    };
    Ok(())

  }
}

/// fn for updates of cache
/// remove both client and server info (including server shutdown), leading to drop (and associated
/// clean operation (multiplexing management, thead shutdown...) of both
pub fn pi_remchan<A : Address, T : Transport<Address = A>, P : Peer<Address = A>, V : KeyVal> (pi : &mut PeerInfo<P,V,T>, t : & T) -> MydhtResult<()> {
  pi.3 = None;
  match &pi.2 {
    &Some(ref si) => {
      try!(si.shutdown(t,&(*pi.0)));
    },
    &None => (),
  };

  // drop may not be call at this point (possibly in query or in server (ended just before))
  pi.2 = None;

  Ok(())
}
/// fn for updates of cache
pub fn pi_upprio<P : Peer,V : KeyVal,T : Transport> (pi : &mut PeerInfo<P,V,T>,ostate : Option<PeerState>,och : Option<PeerStateChange>) -> MydhtResult<()> {
  match ostate {
    Some(newstate) => pi.1 = newstate,
    None => {
      match och {
        Some(ch) => {
          pi.1 = pi.1.new_state(ch);
        },
        None => (),
      };
    },
  }
  Ok(())
}


// TODO refactor to got explicit add and rem chan plus prio
// eg : update with chan plus prio!!
// TODO refactor get closest to return connected closest plus a number of better to have non
// connected and then do some discovery on the better one (just query them).
/// Trait for storing peer information and implementing strategie to choose closest nodes for query
/// (either for querying a peer or a value).
/// Trait contains serializable content (Peer), but also trensiant content like channel to peer
/// client process.
///
/// Route design may separate storage of Blocked peers and Offline peers from others (online),
/// those one must not have handles (both closed) in their cli info so their cli info is useless
/// and can be dropped. Therefore state (PeerPriority) update is a separate operation from peer consultation
/// (might be doable to distinguish those case to do single operation in some cases).
///
/// TODO consider parameterize type with RT : RunningType
///
/// TODO Drop implemetation for Route : shutdown all info (only needed for receive threads) (no override so just an utility fn)
///
pub trait Route<A:Address,P:Peer<Address = A>,V:KeyVal,T:Transport<Address = A>> 

  {
  /// count of running query (currently only updated in proxy mode)
  fn query_count_inc(& mut self, &P::Key);
  /// count of running query (currently only updated in proxy mode)
  fn query_count_dec(& mut self, &P::Key);
  /// add or update a peer
  fn add_node(& mut self, PeerInfo<P,V,T>);
  /// change a peer prio (eg setting offline or normal...), when peerprio is set to none, old
  /// existing priority is used (or unknow) 
  fn update_priority(& mut self, &P::Key, Option<PeerState>, Option<PeerStateChange>);
  // TODO change
  /// get a peer info (peer, priority (eg offline), and existing channel to client process) 
  fn get_node(& self, &P::Key) -> Option<&PeerInfo<P,V,T>>;
  /// has node tells if there is node in route
  fn has_node(& self, k : &P::Key) -> bool {
    self.get_node(k).is_some()
  }
 
  // remove chan for node TODO refactor to two kind of status and auto rem when offline or blocked
  /// remove channel to process (use when a client process broke or normal shutdown).
  fn remchan(&mut self, &P::Key, &T);

  /// function to send message, allowing local send (if there is client thread please prefer
  /// sending from clientinfo).
  /// This function use 
  /// Some route implementation may panic on this (need a route cache where you can write mutable
  /// transport stream)
  /// return false if no peer
  fn local_send(&mut self, &P::Key, ClientMessage<P,V>) -> MydhtResult<bool>;
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
  fn heavy_get_closest_for_node<RT : RunningTypes<P = P, V = V>,C,D>(& self, node : &P::Key, nb : u8, filter : &VecDeque<P::Key>, rc : &RunningProcesses<RT>, each : C, adjustnb : D) 
    where C : Fn(&Arc<P>, &RunningProcesses<RT>), 
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
  fn heavy_get_closest_for_query<RT : RunningTypes<P = P, V = V>,C,D>(& self, k : &V::Key, nb : u8, filter : &VecDeque<P::Key>, rc : &RunningProcesses<RT>, each : C, adjustnb : D)
    where C : Fn(&Arc<P>, &RunningProcesses<RT>), 
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
  fn heavy_get_pool_nodes<RT : RunningTypes<P = P, V = V>,C>(&self, nb : usize, rc : &RunningProcesses<RT>, each : C) 
    where C : Fn(&Arc<P>, &RunningProcesses<RT>) {
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
  use transport::{Transport,Address};
  use std::collections::VecDeque;
use peer::{Peer, PeerPriority,PeerState,PeerStateChange};
  pub fn test_route<A:Address,P:Peer<Address = A>,V:KeyVal,T:Transport<Address = A>,R:Route<A,P,V,T>> (peers : &[Arc<P>; 5], route : & mut R, valkey : V::Key) {
    let fpeer = peers[0].clone();
    let fkey = fpeer.get_key();
    assert!(route.get_node(&fkey).is_none());
    route.add_node((fpeer, PeerState::Offline(PeerPriority::Normal), None,None));
    assert!(route.get_node(&fkey).unwrap().0.get_key() == fkey);
    for p in peers.iter(){
      route.add_node((p.clone(), PeerState::Offline(PeerPriority::Normal), None,None));
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
} 
