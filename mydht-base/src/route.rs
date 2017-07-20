use transport::{
  Transport,
  Address,
};
use mydhtresult::Result as MydhtResult;

use peer::{Peer,PeerState,PeerStateChange};
use keyval::KeyVal;

use std::sync::Arc;
use std::collections::VecDeque;
use std::marker::PhantomData;

/// peerinfo without access to server and client info
pub type PeerInfoBase<P, SI, CI> = (Arc<P>,PeerState,(Option<SI>, Option<CI>));

#[inline]
pub fn is_peerinfo_online<P, SI, CI>(b : &PeerInfoBase<P, SI, CI>) -> bool {
  match b {
    &(_,PeerState::Online(_),_) => true,
    _ => false,
  }
}

pub struct RouteFromBase<A:Address,P:Peer<Address = A>,V:KeyVal,T:Transport<Address = A>,CI,SI,RB : RouteBase<A,P,V,T,CI,SI>> (pub RB,pub PhantomData<(A,P,V,T,CI,SI)>);

/// Neutral route for basic implementation, others should use mydht crate standard route
/// This is done by abstracting PeerInfo, ClientInfo and ServerInfo
pub trait RouteBase<A:Address,P:Peer<Address = A>,V:KeyVal,T:Transport<Address = A>,CI,SI> 

  {
  /// count of running query (currently only updated in proxy mode)
  fn query_count_inc(& mut self, &P::Key);
  /// count of running query (currently only updated in proxy mode)
  fn query_count_dec(& mut self, &P::Key);
  /// add a peer, note that peer should be added in an offline status (offline or refused), and
  /// then status change by update_priority
  fn add_node(& mut self, PeerInfoBase<P, SI, CI>);
  /// change a peer prio (eg setting offline or normal...), when peerprio is set to none, old
  /// existing priority is used (or unknow) 
  fn update_priority(& mut self, &P::Key, Option<PeerState>, Option<PeerStateChange>);

  /// update fields that can be modified in place (not priority or node because it could structure
  /// route storage).
  fn update_infos<F>(&mut self, &P::Key, f : F) -> MydhtResult<bool> where F : FnOnce(&mut (Option<SI>, Option<CI>)) -> MydhtResult<()>;

  // TODO change
  /// get a peer info (peer, priority (eg offline), and existing channel to client process) 
  fn get_node(& self, &P::Key) -> Option<&PeerInfoBase<P, SI, CI>>;
  /// has node tells if there is node in route
  fn has_node(& self, k : &P::Key) -> bool {
    self.get_node(k).is_some()
  }
 

  // TODO maybe return sender instead
  /// routing method to choose peer for a peer query (no offline or blocked peer)
  /// Notice that it try to get n result (third param), without considering the filter parameter,
  /// that means that the number of node in filter must allways be significantly lower than the
  /// number of queried result
  fn get_closest_for_node(& self, &P::Key, u8, &VecDeque<P::Key>) -> Vec<Arc<P>>;
  /// routing method to choose peer for a KeyVal query(no offline or blocked peer)
  fn get_closest_for_query(& self, &V::Key, u8, &VecDeque<P::Key>) -> Vec<Arc<P>>;
  // will be way better with an iterator so that for instance we could try to connect until 
  // no more or connection pool is fine
  // TODO refactor to using this box iterator (trait returned in box for cast)
  /// Get n peer even if offline
  fn get_pool_nodes(& self, usize) -> Vec<Arc<P>>;

  /// Possible Serialize on quit
  fn commit_store(&mut self) -> bool;

  /// Return random peers from route, the peer must be online.
  /// Return less than expected number when not enough peers 
  fn next_random_peers(&mut self, usize) -> Vec<Arc<P>>;
}






/// trait to use peer and content wich can be route depending upon a byte or bit representation of
/// themselves (mainly in DHT routing)
pub mod byte_rep {

  use bit_vec::BitVec;
  use std::borrow::Borrow;
  use std::sync::Arc;


  /// need to contain a fix size u8 representation
  pub trait DHTElem {
  //  const KLEN : usize;
    fn bits_ref (& self) -> & BitVec;
    /// we do not use Eq or Bits compare as in some case (for example a keyval with long key but
    /// short bits it may make sense to compare the long key in the bucket)
    /// return true if equal
    fn kelem_eq(&self, other : &Self) -> bool;
  }

  /// Convenience trait if bitvec representation for kelem is not stored in kelem (computed each time).
  pub trait DHTElemBytes<'a> {
    type Bytes : 'a + Borrow<[u8]>;
  //  const KLENB : usize;
    fn bytes_ref_keb (&'a self) -> Self::Bytes;
    fn kelem_eq_keb(&self, other : &Self) -> bool;
  }

  // TODO test without HRTB : put
  pub struct SimpleDHTElemBytes<KEB> (pub KEB, pub BitVec)
    where for<'a> KEB : DHTElemBytes<'a>;

  impl<KEB> SimpleDHTElemBytes<KEB>
    where for<'a> KEB : DHTElemBytes<'a> {
    pub fn new(v : KEB) -> SimpleDHTElemBytes<KEB> {
      let bv = BitVec::from_bytes(v.bytes_ref_keb().borrow());
      SimpleDHTElemBytes(v,bv)
    }
  }

  impl<KEB> DHTElem for SimpleDHTElemBytes<KEB>
    where for<'a> KEB : DHTElemBytes<'a> {
  //  const KLEN : usize = <Self as DHTElemBytes<'a>>::KLENB;
    fn bits_ref (& self) -> & BitVec {
      &self.1
    }
    fn kelem_eq(&self, other : &Self) -> bool {
      self.0.kelem_eq_keb(&other.0)
    }
  }

  impl<'a, P : DHTElemBytes<'a>> DHTElemBytes<'a> for Arc<P> {
    // return ing Vec<u8> is stupid but it is for testing
    type Bytes = <P as DHTElemBytes<'a>>::Bytes;
    fn bytes_ref_keb (&'a self) -> Self::Bytes {
      (**self).bytes_ref_keb()
    }
    fn kelem_eq_keb(&self, other : &Self) -> bool {
      (**self).kelem_eq_keb(other)
    }
  }

  impl<'a> DHTElemBytes<'a> for String {
    // return ing Vec<u8> is stupid but it is for testing
    type Bytes = Vec<u8>;
    fn bytes_ref_keb (&'a self) -> Self::Bytes {
      self.as_bytes().to_vec()
    }
    fn kelem_eq_keb(&self, other : &Self) -> bool {
      self == other
    }
  }


  impl DHTElem for BitVec {
    #[inline]
    fn bits_ref (& self) -> & BitVec {
      &self
    }
    #[inline]
    fn kelem_eq(&self, other : &Self) -> bool {
    self == other
    }
  }

  impl<'a> DHTElemBytes<'a> for Vec<u8> {
    // return ing Vec<u8> is stupid but it is for testing
    type Bytes = Vec<u8>;
    fn bytes_ref_keb (&'a self) -> Self::Bytes {
      self.clone()
    }
    fn kelem_eq_keb(&self, other : &Self) -> bool {
      self == other
    }
  }

  impl<'a, 'b> DHTElemBytes<'a> for &'b [u8] {
    // using different lifetime is for test : for instance if we store a reference to a KeyVal
    // (typical implementation in route (plus need algo to hash))
    type Bytes = &'a [u8];
    fn bytes_ref_keb (&'a self) -> Self::Bytes {
       self
    }
    fn kelem_eq_keb(&self, other : &Self) -> bool {
      self == other
    }
  }
}

/// fn for updates of cache
pub fn pi_upprio_base<P : Peer,CI,SI> (pi : &mut PeerInfoBase<P,SI,CI>,ostate : Option<PeerState>,och : Option<PeerStateChange>) -> MydhtResult<()> {
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

