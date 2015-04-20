extern crate dht as odht;
extern crate num;

use std::num::ToPrimitive;
use self::odht::KNodeTable;
use self::odht::Peer as DhtPeer;
use std::io::Result as IoResult;

use std::collections::{HashMap,BTreeSet,VecDeque};
use peer::{Peer,PeerPriority};
use procs::{ClientChanel};
use std::sync::{Arc};
use std::sync::mpsc::{Sender,Receiver};
use route::Route;
use std::iter::Iterator;
use std::rc::Rc;
use kvstore::KeyVal;
use kvstore::Attachment;

#[derive(Clone,Debug)]
struct ArcP<P : DhtPeer>(Arc<P>);

// No priority mgmt only offline or not (only one kad) // TODO minus hashmap : pb is get
/// Routing with a bitorrent kademlia choice of closest peer
/// In fact it need DhtKey for peer and value (converting key to bigint to be able to xor
/// keys).
/// This route need Peer implementing DhtPeer trait for interface with underlying library fork.
pub struct BTKad<P : Peer + DhtPeer, V : KeyVal> {
  kad : KNodeTable<ArcP<P>>,
  peers : HashMap<P::Key, (Arc<P>, PeerPriority, Option<ClientChanel<P,V>>)>,
}


impl<P : DhtPeer> DhtPeer for ArcP<P> {
  type Id = P::Id;
  fn id<'a> (&'a self) -> &'a P::Id {
    self.0.id()
  }
  #[inline]
  fn key_as_buint<'a>(k : &'a P::Id) -> &'a num::BigUint {
    <P as DhtPeer>::key_as_buint(k)
  }
  #[inline]
  fn random_id(hash_size : usize) -> P::Id {
    <P as DhtPeer>::random_id(hash_size)
  }
}

// trait use for kad like routing where the key of value is compared with user dhtkey
pub trait DhtKey<K : DhtPeer> {
  fn to_peer_key(&self) -> K::Id;
}

// implementation of bt kademlia from rust-dht project
impl<P : Peer + DhtPeer, V : KeyVal> Route<P,V> for BTKad<P,V> where P::Key : DhtKey<P>,  V::Key : DhtKey<P>  {
  fn query_count_inc(& mut self, pnid : &P::Key){
    // Not used
  }

  fn query_count_dec(& mut self, pnid : &P::Key){
    // Not used
  }

  fn add_node(& mut self, node : Arc<P>, chan : Option<ClientChanel<P,V>>) {
    self.peers.insert(node.get_key(), (node,PeerPriority::Offline, chan));
  }

  fn remchan(& mut self, nodeid : &P::Key) {
    let mut peer = match self.peers.get(nodeid) {
      Some(&(_,_, None)) => {None}, // TODO rewrite with in place write of hashmap (currently some issue with arc).
      Some(&(ref ap,prio, ref s)) => {Some ((ap.clone(), prio, None))}, // TODO rewrite with in place write of hashmap (currently some issue with arc).
      None => {None},
    };
    match peer {
      Some(v) => {
        self.peers.insert(nodeid.clone(),v);
      },
        None => (),
    };
  }


  fn update_priority(& mut self, nodeid : &P::Key, prio : PeerPriority) {
    let putinkad = prio != PeerPriority::Offline && prio != PeerPriority::Blocked;
    debug!("update prio of {:?} to {:?}",nodeid,prio);
    let mut peer = match self.peers.get(nodeid) {
      Some(&(ref ap,_, ref s)) => {Some ((ap.clone(), prio, s.clone()))}, // TODO rewrite with in place write of hashmap (currently some issue with arc).
      None => {None},
    };

/*    match peer {
      Some((_,_,None)) => println!("######## updating on no channel"),
      None => println!("######### updating on no value"),
       _ => println!("#### updating with xisting channel"),
     };*/
    match peer {
      Some(v) => {
        let tmp = v.0.clone();
        self.peers.insert(nodeid.clone(),v);
        if putinkad {
          // Note that update is done even if same status (it invole a kad position
          // update)
          let updok = self.kad.update(&ArcP(tmp)); // TODO return possibly not added node
          if !updok {
            debug!("Viable node not in possible closest due to full bucket");
          }
        } else {
          //remove
          self.kad.remove(&nodeid.to_peer_key());
        }
      },
      None => (),
    };
  }

  fn get_node(& self, nid : &P::Key) -> Option<&(Arc<P>, PeerPriority, Option<ClientChanel<P,V>>)>  {
    self.peers.get(nid)
  }

  fn get_closest_for_node(& self, nnid : &P::Key, nbnode : u8, filter : &VecDeque<P::Key>) -> Vec<Arc<P>> {
    let id : &P::Id = &nnid.to_peer_key();
    self.kad.find(id, nbnode.to_uint().unwrap()) // TODO filter offline!! if no remove??
      .into_iter().map (|n|n.0).collect()
  }

  fn get_closest_for_query(& self, nnid : &V::Key, nbnode : u8, filter : &VecDeque<P::Key>) -> Vec<Arc<P>> {
    self.kad.find(&nnid.to_peer_key(), nbnode.to_uint().unwrap()) // TODO filter offline!!
      .into_iter().map (|n|n.0).collect()
  }
 
  fn get_pool_nodes(& self, nbnode : usize) -> Vec<Arc<P>> {
    let mut r = Vec::new();
    let mut i = 0;
    // here by closest but not necessary could be random (ping may define closest)
    for p in self.peers.iter() {
      r.push((p.1).0.clone());
      i = i + 1;
      if i == nbnode {break;}
    };
    r
  }

  fn commit_store(& mut self) -> bool{
    // TODO implement serialization??? -> only for the hashmap, and load on start
    true
  }


}

impl<P:Peer + DhtPeer,V: KeyVal> BTKad<P,V> {
    // TODO spawn a cleaner of the dht for poping oldest or pop in knodetable on update over full
    // TODO use with detail for right size of key (more in knodetable)
    pub fn new(k : P::Id) -> BTKad<P,V> {
        BTKad{ peers : HashMap::new(), kad : KNodeTable::new(k)}
    }
}



#[cfg(test)]
mod test {
  extern crate dht as odht;
  extern crate num;
  extern crate rand;
  use rustc_serialize as serialize;
  use super::super::Route;
  use super::BTKad;
  use super::DhtKey;
  use super::super::test;
  use kvstore::KeyVal;
  use std::sync::{Arc};
  use std::collections::VecDeque;
  use peer::node::{Node,NodeID};
  use self::odht::Peer as DhtPeer;
  use self::num::{BigUint};
  use self::num::bigint::RandBigInt;
  use std::net::{SocketAddr};
  use utils::{SocketAddrExt};
  use utils;
  use std::io::Result as IoResult;
  use peer::Peer;
  use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};
  use std::fs::File;

  use std::net::{Ipv4Addr};
  use kvstore::Attachment;
  use std::str::FromStr;

// TODO a clean nodeK, with better serialize (use as_vec) , but here good for testing as key is not
// same type as id
#[derive(RustcDecodable,RustcEncodable,Debug,PartialEq,Eq,Clone)]
pub struct NodeK(Node,BigUint);

impl KeyVal for NodeK {
    type Key = NodeID;
    fn get_key(&self) -> NodeID {
        self.0.get_key()
    }
    nospecificencoding!(NodeK);
    noattachment!();
}

impl Peer for NodeK {
  //type Address = <Node as Peer>::Address;
  #[inline]
  //fn to_address(&self) -> <Node as Peer>::Address {
  fn to_address(&self) -> SocketAddr {
    self.0.to_address()
  }

}

impl DhtPeer for NodeK {
  type Id = BigUint;
  #[inline]
  fn id<'a> (&'a self) -> &'a BigUint {
    &self.1
//      &BigUint::from_bytes_be(self.nodeid.as_bytes())
  }
  #[inline]
  fn key_as_buint<'a>(k : &'a BigUint) -> &'a BigUint {
      k
  }
  #[inline]
  fn random_id(hash_size : usize) -> BigUint {
      let mut rng = rand::thread_rng();
      rng.gen_biguint(hash_size)
  }
}
impl DhtKey<NodeK> for NodeID {
  fn to_peer_key(&self) -> BigUint{
      //BigUint::from_bytes_be(self.as_bytes())
      FromStr::from_str(&self[..]).unwrap()
  }
}
/*
impl serialize::Encodable for NodeK {
    fn encode<S:serialize::Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
        s.emit_struct("Node", 2, |s| {
            try!(s.emit_struct_field("node", 0, |s2| {
                self.0.encode(s2)
            }));

            try!(s.emit_struct_field("id", 1, |s2| {
                self.1.to_bytes_be().encode(s2)
            }));

            Ok(())
        })
    }
}

impl serialize::Decodable for NodeK {
    fn decode<D:serialize::Decoder> (d : &mut D) -> Result<Self, D::Error> {
        d.read_struct("Node", 2, |d| {
            let addr = try!(d.read_struct_field("node", 0, |d2| {
                serialize::Decodable::decode(d2)
            }));

// TODO not sure ok between vec and &[] of encode
            let id : Vec<u8> = try!(d.read_struct_field("id", 1, |d2| {
                serialize::Decodable::decode(d2)
            }));

            Ok((addr, BigUint::from_bytes_be(id.as_slice()))) 
        })
    }
}
*/


fn initpeer() -> Arc<NodeK> {
  let id = <NodeK as DhtPeer>::random_id(160); // TODO hash size in btkad params 
//    let sid = to_str_radix(id,10);
  let sid = id.to_string();
  let rid = sid.to_peer_key();
  assert!(rid == id);
  Arc::new(NodeK(Node{nodeid:sid, address: SocketAddrExt(utils::sa4(Ipv4Addr::new(127,0,0,1), 8080))},id))
}


//#[test]
fn test(){
  let myid = <NodeK as DhtPeer>::random_id(160); // TODO hash size in btkad params 
  let mut route : BTKad<NodeK, NodeK> = BTKad::new(myid);
    let nodes  = [
    initpeer(),
    initpeer(),
    initpeer(),
    initpeer(),
    initpeer(),
    ];
  let kv = <NodeK as DhtPeer>::random_id(160).to_string();

  test::test_route(&nodes, & mut route, kv);
}

} 
