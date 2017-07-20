
use bit_vec::BitVec;
//use std::io::Result as IoResult;
use mydht_base::mydhtresult::{ErrorKind,Error};

use std::collections::{HashMap,VecDeque};
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
use mydht_base::kvcache::KVCache;
use std::marker::PhantomData;
use mydht_base::route::{pi_upprio_base};
use mydht_base::mydhtresult::Result as MydhtResult;

use std::borrow::Borrow;

use mydht_base::route::RouteFromBase;
use mydht_base::route::byte_rep::{
  //DHTElem,
  DHTElemBytes,
  SimpleDHTElemBytes
};

use super::knodetable::KNodeTable;


// No priority mgmt only offline or not (only one kad) // TODO minus hashmap : pb is get
/// Routing with a bitorrent kademlia choice of closest peer
/// In fact it need DhtKey for peer and value (converting key to bigint to be able to xor
/// keys).
/// This route need Peer implementing DHTPeer trait for interface with underlying library fork.
/// TODO make it generic over any routing table
/// Note that for element which store a bitvec as a key this is not efficient (for element storing
/// or calculing Vec<u8> its fine).
pub struct BTKadBase<P : Peer, V : KeyVal, T : Transport, C : KVCache<P::Key, PeerInfoBase<P,SI,CI>>,CI,SI> 
  where for<'a> P::Key : DHTElemBytes<'a>,
        for<'a> V::Key : DHTElemBytes<'a>,
        for<'a> P : DHTElemBytes<'a>,
{
  // TODO switch to a table of key only : here P is already in cache!!!!
  kad : KNodeTable<SimpleDHTElemBytes<Arc<P>>>,
//  peers : HashMap<P::Key, (Arc<P>, PeerPriority, Option<ClientChanel<P,V>>)>,
  peers : C, //TODO maybe get node priority out of this container or do in place update
  _phdat : PhantomData<(V,T,CI,SI)>,
}



// implementation of bt kademlia from rust-dht project
impl<A : Address, P : Peer<Address = A>, V : KeyVal, T : Transport<Address = A>, C : KVCache<P::Key, PeerInfoBase<P,SI,CI>>,CI,SI> RouteBase<A,P,V,T,CI,SI> for BTKadBase<P,V,T,C,CI,SI> 
  where for<'a> P::Key : DHTElemBytes<'a>,
        for<'a> V::Key : DHTElemBytes<'a>,
        for<'a> P : DHTElemBytes<'a>,
{
  fn query_count_inc(& mut self, _ : &P::Key){
    // Not used
  }

  fn query_count_dec(& mut self, _ : &P::Key){
    // Not used
  }
  fn add_node(& mut self, pi : PeerInfoBase<P,SI,CI>) {
    self.peers.add_val_c(pi.0.get_key(), pi);
    // TODO if online update val in kad too
  }


  fn update_infos<F>(&mut self, nodeid : &P::Key, f : F) -> MydhtResult<bool> where F : FnOnce(&mut (Option<SI>, Option<CI>)) -> MydhtResult<()> {
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
      pi_upprio_base(v,opri,och)
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

  fn get_node(& self, nid : &P::Key) -> Option<&PeerInfoBase<P,SI,CI>> {
    self.peers.get_val_c(nid)
  }
  fn has_node(& self, nid : &P::Key) -> bool {
    self.peers.has_val_c(nid)
  }

  fn get_closest_for_node(& self, nnid : &P::Key, nbnode : u8, filter : &VecDeque<P::Key>) -> Vec<Arc<P>> {
    let id = BitVec::from_bytes(nnid.bytes_ref_keb().borrow());
    let mut res : Vec<Arc<P>> = Vec::with_capacity(nbnode as usize);

    for n in self.kad.get_closest(&id, nbnode as usize) {
      if filter.iter().find(|r|**r == n.0.get_key()) == None {
        res.push(n.0.clone());
      }
    };
    res
  }

  fn get_closest_for_query(& self, nnid : &V::Key, nbnode : u8, filter : &VecDeque<P::Key>) -> Vec<Arc<P>> {
    let id = BitVec::from_bytes(nnid.bytes_ref_keb().borrow());
    let mut res : Vec<Arc<P>> = Vec::with_capacity(nbnode as usize);

    for n in self.kad.get_closest(&id, nbnode as usize){
      if filter.iter().find(|r|**r == n.0.get_key()) == None {
        res.push(n.0.clone());
      }
    }
    res
  }
 
  fn get_pool_nodes(& self, nbnode : usize) -> Vec<Arc<P>> {
    let mut r = Vec::new();
    let mut i = 0;
    // here by closest but not necessary could be random (ping may define closest)
    if self.peers.map_inplace_c(|p|{
//    for p in self.peers.iter() {
      r.push((p.1).0.clone());
      i = i + 1;
      if i == nbnode {
        Err(Error("".to_string(), ErrorKind::ExpectedError, None))
      } else {
        Ok(())
      }
      
    }).is_err(){
      debug!("expected error ending map earlier"); // TODO check errorkind
    }
    r
  }

  fn commit_store(& mut self) -> bool{
    // TODO implement serialization??? -> only for the hashmap, and load on start
    true
  }

  fn next_random_peers(&mut self, nb : usize) -> Vec<Arc<P>> {
    self.peers.next_random_values(nb, Some(&is_peerinfo_online))
      .into_iter().map(|p|(p.0).clone()).collect()
  }

}

pub type BTKad<A,P,V,T,HM,CI,SI> = RouteFromBase<A,P,V,T,CI,SI,BTKadBase<P,V,T,HM,CI,SI>>;

  #[inline]
  pub fn new
  <A:Address,P:Peer<Address = A>,V:KeyVal,T:Transport<Address = A>,CI,SI> 
  (k : &P::Key, bucket_size : usize, hash_size : usize) -> BTKad<A,P,V,T,HashMap<P::Key, PeerInfoBase<P,SI,CI>>,CI,SI> 
   where for<'a> P::Key : DHTElemBytes<'a>,
        for<'a> V::Key : DHTElemBytes<'a>,
        for<'a> P : DHTElemBytes<'a>,
        P::Key : Hash,
  {
    new_with_cache(k, HashMap::new(), bucket_size, hash_size)
  }


    // TODO spawn a cleaner of the dht for poping oldest or pop in knodetable on update over full
    // TODO use with detail for right size of key (more in knodetable)
  pub fn new_with_cache
  <A:Address,P:Peer<Address = A>,V:KeyVal,T:Transport<Address = A>, C : KVCache<P::Key, PeerInfoBase<P,SI,CI>>,CI,SI> 
  (k : &P::Key, c : C, bucket_size : usize, hash_size : usize) -> BTKad<A,P,V,T,C,CI,SI> 
  where for<'a> P::Key : DHTElemBytes<'a>,
        for<'a> V::Key : DHTElemBytes<'a>,
        for<'a> P : DHTElemBytes<'a>,
        P::Key : Hash,
  {
    let usid = BitVec::from_bytes(k.bytes_ref_keb().borrow());
    let ktable = KNodeTable::new(usid, bucket_size, hash_size);
    RouteFromBase(BTKadBase{ kad : ktable, peers : c, _phdat : PhantomData},PhantomData)
  }




#[cfg(test)]
mod test {
  extern crate rustc_serialize;
  extern crate mydht_basetest;
  extern crate num;
  extern crate rand;


  //use mydht_base::route::RouteBase;
  use super::BTKad;
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
  let mut res : Vec<u8> = vec!(0;hash_size / 8);
  rng.fill_bytes(&mut res[..]);
  unsafe{
    String::from_utf8_unchecked(res)
  }
}

fn initpeer() -> Arc<PeerTest> {
  let id = random_id(HASH_SIZE);
//    let sid = to_str_radix(id,10);
  Arc::new(PeerTest{nodeid:id, address: LocalAdd(0), keyshift: 1})
}


#[test]
fn test(){
  let myid = random_id(HASH_SIZE); // TODO hash size in btkad params 
  let mut route : BTKad<LocalAdd,PeerTest,PeerTest,TransportTest,_,u8,u8> = super::new(&myid,BUCKET_SIZE,HASH_SIZE);
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
