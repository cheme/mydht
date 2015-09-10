
use bit_vec::BitVec;
use std::io::Result as IoResult;
use mydhtresult::{ErrorKind,Error};

use std::collections::{HashMap,BTreeSet,VecDeque};
use peer::{Peer,PeerPriority,PeerState,PeerStateChange};
use procs::{ClientChanel};
use std::sync::{Arc};
use std::sync::mpsc::{Sender,Receiver};
use route::Route;
use std::iter::Iterator;
use std::rc::Rc;
use keyval::KeyVal;
use keyval::{Attachment,SettableAttachment};
use num::traits::ToPrimitive;
use std::hash::Hash;
use transport::{Transport,Address};
use super::PeerInfo;
use kvcache::KVCache;
use std::marker::PhantomData;
use super::{pi_remchan,pi_upprio};
use procs::mesgs::{ClientMessage};
use mydhtresult::Result as MydhtResult;
use super::{ServerInfo,ClientInfo};

use std::borrow::Borrow;

use super::byte_rep::{
  DHTElem,
  DHTElemBytes,
  SimpleDHTElemBytes
};

use route::knodetable::KNodeTable;


// No priority mgmt only offline or not (only one kad) // TODO minus hashmap : pb is get
/// Routing with a bitorrent kademlia choice of closest peer
/// In fact it need DhtKey for peer and value (converting key to bigint to be able to xor
/// keys).
/// This route need Peer implementing DHTPeer trait for interface with underlying library fork.
/// TODO make it generic over any routing table
/// Note that for element which store a bitvec as a key this is not efficient (for element storing
/// or calculing Vec<u8> its fine).
pub struct BTKad<P : Peer, V : KeyVal, T : Transport, C : KVCache<P::Key, PeerInfo<P,V,T>>> 
  where for<'a> P::Key : DHTElemBytes<'a>,
        for<'a> V::Key : DHTElemBytes<'a>,
        for<'a> P : DHTElemBytes<'a>,
{
  // TODO switch to a table of key only : here P is already in cache!!!!
  kad : KNodeTable<SimpleDHTElemBytes<Arc<P>>>,
//  peers : HashMap<P::Key, (Arc<P>, PeerPriority, Option<ClientChanel<P,V>>)>,
  peers : C, //TODO maybe get node priority out of this container or do in place update
  _phdat : PhantomData<(V,T)>,
}



// implementation of bt kademlia from rust-dht project
impl<A : Address, P : Peer<Address = A>, V : KeyVal, T : Transport<Address = A>, C : KVCache<P::Key, PeerInfo<P,V,T>>> Route<A,P,V,T> for BTKad<P,V,T,C> 
  where for<'a> P::Key : DHTElemBytes<'a>,
        for<'a> V::Key : DHTElemBytes<'a>,
        for<'a> P : DHTElemBytes<'a>,
{
  fn query_count_inc(& mut self, pnid : &P::Key){
    // Not used
  }

  fn query_count_dec(& mut self, pnid : &P::Key){
    // Not used
  }
  fn add_node(& mut self, pi : PeerInfo<P,V,T>) {
    self.peers.add_val_c(pi.0.get_key(), pi);
    // TODO if online update val in kad too
  }

  fn remchan(& mut self, nodeid : &P::Key, t : & T) where P::Key : Send {
    // TODO result management
    self.peers.update_val_c(nodeid,|pi|pi_remchan(pi,t)).unwrap();
  }

  fn update_infos<F>(&mut self, nodeid : &P::Key, f : F) -> MydhtResult<bool> where F : FnOnce(&mut (Option<ServerInfo>, Option<ClientInfo<P,V,T>>)) -> MydhtResult<()> {
    self.peers.update_val_c(nodeid,|& mut (_,_,ref mut sici)|{
      f(sici)
    })
  }

  fn update_priority(& mut self, nodeid : &P::Key, opri : Option<PeerState>, och : Option<PeerStateChange>) where P::Key : Send {
    debug!("update prio of {:?} to {:?} , {:?}",nodeid,opri,och);
    let putinkad = match opri {
      Some (ref pri) => {
        match pri {
          &PeerState::Offline(_) => false,
          &PeerState::Blocked(_) => false,
          _ => true,
        }
      },
      None => match och {
        Some(ref ch) => *ch != PeerStateChange::Offline && *ch != PeerStateChange::Blocked,
        None => false,
      }
    };
    if let Ok(true) = self.peers.update_val_c(nodeid,|v|{
      pi_upprio(v,opri,och)
    }) {
      if putinkad {
        // Note that update is done even if same status (it invole a kad position
        // update)
        let tmp = self.peers.get_val_c(nodeid);
        if tmp.is_some(){
          self.kad.add(SimpleDHTElemBytes::new(tmp.unwrap().0.clone())) // TODO return possibly not added node
        };
      } else {
        let tmp = self.peers.get_val_c(nodeid);
        if let Some(elem) = tmp {
          //remove
          self.kad.remove(&SimpleDHTElemBytes::new(elem.0.clone())); // TODO
        }
      }

    };
  }

  fn get_node(& self, nid : &P::Key) -> Option<&PeerInfo<P,V,T>> {
    self.peers.get_val_c(nid)
  }
  fn has_node(& self, nid : &P::Key) -> bool {
    self.peers.has_val_c(nid)
  }

  fn get_closest_for_node(& self, nnid : &P::Key, nbnode : u8, filter : &VecDeque<P::Key>) -> Vec<Arc<P>> {
    let id = BitVec::from_bytes(nnid.bytes_ref_keb().borrow());
    self.kad.get_closest(&id, nbnode.to_usize().unwrap())
      .into_iter().map (|n|n.0.clone()).collect()
  }

  fn get_closest_for_query(& self, nnid : &V::Key, nbnode : u8, filter : &VecDeque<P::Key>) -> Vec<Arc<P>> {
    let id = BitVec::from_bytes(nnid.bytes_ref_keb().borrow());
    self.kad.get_closest(&id, nbnode.to_usize().unwrap())
      .into_iter().map (|n|n.0.clone()).collect()
  }
 
  fn get_pool_nodes(& self, nbnode : usize) -> Vec<Arc<P>> {
    let mut r = Vec::new();
    let mut i = 0;
    // here by closest but not necessary could be random (ping may define closest)
    self.peers.map_inplace_c(|p|{
//    for p in self.peers.iter() {
      r.push((p.1).0.clone());
      i = i + 1;
      if i == nbnode {Err(Error("".to_string(), ErrorKind::ExpectedError, None))} else {Ok(())}
      
    });
    r
  }

  fn commit_store(& mut self) -> bool{
    // TODO implement serialization??? -> only for the hashmap, and load on start
    true
  }


}

impl<P : Peer, V : KeyVal, T : Transport> BTKad<P,V,T,HashMap<P::Key, PeerInfo<P,V,T>>>
  where for<'a> P::Key : DHTElemBytes<'a>,
        for<'a> V::Key : DHTElemBytes<'a>,
        for<'a> P : DHTElemBytes<'a>,
        P::Key : Hash,
{
  #[inline]
  pub fn new(k : &P::Key, bucket_size : usize, hash_size : usize) -> BTKad<P,V,T,HashMap<P::Key, PeerInfo<P,V,T>>> {
    Self::new_with_cache(k, HashMap::new(), bucket_size, hash_size)
  }
}

impl<P : Peer, V : KeyVal, T : Transport, C : KVCache<P::Key, PeerInfo<P,V,T>>> BTKad<P,V,T,C>
  where for<'a> P::Key : DHTElemBytes<'a>,
        for<'a> V::Key : DHTElemBytes<'a>,
        for<'a> P : DHTElemBytes<'a>,
        P::Key : Hash,
{
    // TODO spawn a cleaner of the dht for poping oldest or pop in knodetable on update over full
    // TODO use with detail for right size of key (more in knodetable)
  pub fn new_with_cache(k : &P::Key, c : C, bucket_size : usize, hash_size : usize) -> BTKad<P,V,T,C> {
    let usid = BitVec::from_bytes(k.bytes_ref_keb().borrow());
    let ktable = KNodeTable::new(usid, bucket_size, hash_size);
    BTKad{ kad : ktable, peers : c, _phdat : PhantomData}
  }
}




#[cfg(test)]
mod test {
  use rustc_serialize as serialize;
  use super::super::byte_rep::{
    DHTElem,
    SimpleDHTElemBytes,
    DHTElemBytes,
  };


  use super::super::Route;
  use super::BTKad;
  use super::super::test;
  use keyval::KeyVal;
  use std::sync::{Arc};
  use std::collections::VecDeque;
  use peer::node::{Node,NodeID};
  use num::{BigUint};
  use num::bigint::RandBigInt;
  use std::net::{SocketAddr};
  use utils::{SocketAddrExt};
  use utils;
  use std::io::Result as IoResult;
  use peer::Peer;
  use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};
  use std::fs::File;
  use rand;
  use rand::Rng;
  use std::net::{Ipv4Addr};
  use keyval::{Attachment,SettableAttachment};
  use std::str::FromStr;
  use transport::tcp::Tcp;

// TODO a clean nodeK, with better serialize (use as_vec) , but here good for testing as key is not
// same type as id
//#[derive(RustcDecodable,RustcEncodable,Debug,PartialEq,Eq,Clone)]
//pub struct NodeK(Node,BigUint);
type NodeK = Node;
/*
impl KeyVal for NodeK {
    type Key = NodeID;
    /*
    #[inline]
    fn get_key_ref<'a>(&'a self) -> &'a NodeID {
        self.0.get_key_ref()
    }*/

    #[inline]
    fn get_key(&self) -> NodeID {
        self.0.get_key()
    }
    noattachment!();
}

impl SettableAttachment for NodeK { }

impl Peer for NodeK {
  type Address = <Node as Peer>::Address;
  #[inline]
  fn to_address(&self) -> <Node as Peer>::Address {
    self.0.to_address()
  }

}
*/


impl<'a> DHTElemBytes<'a> for NodeK {
  // return ing Vec<u8> is stupid but it is for testing
  type Bytes = Vec<u8>;
  fn bytes_ref_keb (&'a self) -> Self::Bytes {
    self.nodeid.bytes_ref_keb()
  }
  fn kelem_eq_keb(&self, other : &Self) -> bool {
    self == other
  }
}
impl<'a> DHTElemBytes<'a> for NodeID {
  // return ing Vec<u8> is stupid but it is for testing
  type Bytes = Vec<u8>;
  fn bytes_ref_keb (&'a self) -> Self::Bytes {
    self.as_bytes().to_vec()
  }
  fn kelem_eq_keb(&self, other : &Self) -> bool {
    self == other
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

const HASH_SIZE : usize = 160;
const BUCKET_SIZE : usize = 10;

fn random_id(hash_size : usize) -> NodeID {
  let mut rng = rand::thread_rng();
  let mut res : Vec<u8> = [0;HASH_SIZE / 8].to_vec();
  rng.fill_bytes(&mut res[..]);
  unsafe{
    String::from_utf8_unchecked(res)
  }
}

fn initpeer() -> Arc<NodeK> {
  let id = random_id(HASH_SIZE); // TODO hash size in btkad params 
//    let sid = to_str_radix(id,10);
  Arc::new(Node{nodeid:id, address: SocketAddrExt(utils::sa4(Ipv4Addr::new(127,0,0,1), 8080))})
}


#[test]
fn test(){
  let myid = random_id(HASH_SIZE); // TODO hash size in btkad params 
  let mut route : BTKad<NodeK, NodeK,Tcp,_> = BTKad::new(&myid,BUCKET_SIZE,HASH_SIZE);
    let nodes  = [
    initpeer(),
    initpeer(),
    initpeer(),
    initpeer(),
    initpeer(),
    ];
  let kv = random_id(HASH_SIZE).to_string();

  test::test_route(&nodes, & mut route, kv);
}

} 
