
//use bit_vec::BitVec;
//use std::io::Result as IoResult;
//use mydht_base::mydhtresult::{ErrorKind,Error};

use std::collections::VecDeque;
use mydht_base::peer::Peer;
//use mydht_base::procs::{ClientChanel};
//use std::sync::mpsc::{Sender,Receiver};
use std::iter::Iterator;
//use std::rc::Rc;
use mydht_base::keyval::KeyVal;
//use mydht_base::keyval::{Attachment,SettableAttachment};
use mydht_base::kvcache::{
  Cache,
  KVCache,
  RandCache,
};
use std::marker::PhantomData;
use mydht_base::mydhtresult::Result;

use std::borrow::Borrow;
use mydht_base::route2::{
  GetPeerRef,
  RouteBaseMessage,
  RouteBase as RouteBase2,
  RouteMsgType,
};


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

  fn get_closest(&mut self, nbnode : usize, mut filter : Option<&mut VecDeque<P::Key>>) -> Vec<usize> {
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
      if let Some(ref mut filt) = filter {
        println!("in filter {:?}",filt);
        if filt.iter().find(|r|**r == nid.0) != None {
          filtered = true
        }
      }
      if !filtered {
        if let Some(ws) = nid.1 {
          r.push(ws);
          filter.as_mut().map(|f|f.push_back(nid.0.clone()));
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
  fn route_base<MSG : RouteBaseMessage<P>>(&mut self, nb : usize, mut m : MSG, _ : RouteMsgType) -> Result<(MSG,Vec<usize>)> {
    let closest = {
      let filter = m.get_filter_mut();
      self.get_closest(nb,filter)
    };
    m.adjust_lastsent_next_hop(closest.len());
//fn update_lastsent_conf<P : Peer> ( queryconf : &QueryMsg<P>,  peers : &Vec<Arc<P>>, nbquery : u8) -> QueryMsg<P> {
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
      let (p,_,ows) = v.get_peer_ref();
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
  fn len_c (&self) -> usize {
    self.peers.len_c()
  }

}

/*
impl<P : Peer,RP : Borrow<P>,GP : GetPeerRef<P,RP> + Clone, C : RandCache<<P as KeyVal>::Key,GP>>
  KVCache<<P as KeyVal>::Key,GP> for InefficientmapBase2<P,RP,GP,C> {

  fn update_val_c<F>(& mut self, k : &<P as KeyVal>::Key, f : F) -> Result<bool> where F : FnOnce(& mut GP) -> Result<()> {
    self.peers.update_val_c(k,f)
  }
 
  fn strict_fold_c<'a, B, F>(&'a self, init: B, f: F) -> B where F: Fn(B, (&'a <P as KeyVal>::Key, &'a GP)) -> B, <P as KeyVal>::Key : 'a, GP : 'a {
    self.peers.strict_fold_c(init,f)
  }

  fn fold_c<'a, B, F>(&'a self, init: B, f: F) -> B where F: FnMut(B, (&'a <P as KeyVal>::Key, &'a GP)) -> B, <P as KeyVal>::Key : 'a, GP : 'a {
    self.peers.fold_c(init,f)
  }

  fn new() -> Self {
    InefficientmapBase2::new(C::new())
  }

}

impl<P : Peer,RP : Borrow<P>,GP : GetPeerRef<P,RP> + Clone, C : RandCache<<P as KeyVal>::Key,GP>>
  RandCache<<P as KeyVal>::Key,GP> for InefficientmapBase2<P,RP,GP,C> { }
*/
