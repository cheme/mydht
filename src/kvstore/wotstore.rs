extern crate bincode;
use time::Timespec;
use time;
use std::io::Error as IoError;
use std::fs::File;
use std::io::ErrorKind as IoErrorKind;
use std::io::Result as IoResult;
use std::path::Path;
//use mydht::utils;
//use mydht::kvstoreif::{FileKeyVal,KeyVal,KVStore};
use kvstore::{KeyVal,FileKeyVal,KVStore};
use kvstore::{KVStoreRel,Key};
use kvstore::{Attachment};
use std::sync::Arc;
//use mydht::queryif::CachePolicy;
use query::cache::CachePolicy;
use std::old_path::GenericPath;
use rustc_serialize::{Encodable, Decodable, Encoder, Decoder};
//use super::trustedpeers::{TrustedPeer,PromSigns,PeerSign};
use peer::trustedpeer::{RSAPeer,TrustedPeer,PromSigns,PeerSign};
use peer::trustedpeer::{TrustedVal,Truster};
use std::collections::VecMap;
#[cfg(test)]
use query::simplecache::SimpleCache;
use std::num::Int;
use std::num::{ToPrimitive};
use std::iter;
use utils::TimeSpecExt;
use utils::ArcKV;
use std::marker::PhantomFn;
use std::marker::PhantomData;
use std::ops::Deref;
use utils;
use utils::SocketAddrExt;
#[cfg(test)]
use std::net::Ipv4Addr; 


static NULL_TIMESPEC : Timespec = Timespec{ sec : 0, nsec : 0};

/*derive_enum_keyval!(WotKV {TP => TrustedPeer,}, WotK, {
    0 , Peer       => TP,
    1 , P2S        => PromSigns<TP>,
    2 , Sign       => PeerSign<TP>,
    3 , TrustQuery => TrustQuery<TP>,
    //  1 , File  => fKV,
    });*/
#[derive(RustcDecodable,RustcEncodable,Debug,PartialEq,Eq,Clone)]
pub enum WotKV<TP : TrustedPeer> {
  /// A peer
  Peer(ArcKV<TP>),
  // TODO consider promsign in mutex (sending arc of mutex (changes a lot))
  /// Promoted signature about a peer.
  P2S(PromSigns<TP>),
  // TODO consider Arc
  /// Signature from a peer.
  Sign(PeerSign<TP>),
  // TODO consider promsign in mutex (sending arc of mutex (change a lot)): for now seen as small
  /// used for query of trust only (change a lot).
  TrustQuery(TrustQuery<TP>),
}

#[derive(RustcDecodable,RustcEncodable,Debug,PartialEq,Eq,Hash,Clone,PartialOrd,Ord)]
pub enum WotK {
  Peer(Vec<u8>),
  P2S(Vec<u8>),
  Sign((Vec<u8>, Vec<u8>)),
  TrustQuery(Vec<u8>),
}
  
impl<TP : TrustedPeer> KeyVal for WotKV<TP> {
  type Key = WotK;
  derive_enum_keyval_inner!(WotKV<TP>, WotKV, WotK, WotK, {
    0 , Peer       => ArcKV<TP>,
    1 , P2S        => PromSigns<TP>,
    2 , Sign       => PeerSign<TP>,
    3 , TrustQuery => TrustQuery<TP>,
  });
}
/* 
derive_enum_keyval!(WotKV { }, WotK, {
    0 , Peer       => RSAPeer,
    1 , P2S        => PromSigns<RSAPeer>,
    2 , Sign       => PeerSign<RSAPeer>,
    3 , TrustQuery => TrustQuery<RSAPeer>,
    //  1 , File  => fKV,
    });
*/

impl Key for Vec<u8> {}

/// Storage of Wot info, here we use three substore. The one with relationship,
/// should be a trensiant internal implementation loaded at start or bd
/// related call. Here we use this implemetation for the sake of simplicity.
///
/// WotStore check peers before storing them, and also check peersign.
/// This is not optimal because result is also return to caller which must also 
/// check it. This is due to value being simply KeyVal.
///
/// WotStore also manage a store of promoted trust, to facilitate trust transmission.
///
/// WotStore manage trust locally, this explain that we do not attach trust to peer but that trust
/// is only accessible through a special KeyVal : TrustQuery.
pub struct WotStore<TP : TrustedPeer, T : WotTrust<TP>> {
  /// Peer indexed by PeerId
  peerstore : Box<KVStore<ArcKV<TP>>>,
  /// Special KeyVal to query signature about a peer.
  promstore : Box<KVStore<PromSigns<TP>>>,
  /// Cache of curent trust calculation state (and trust level)
  /// This may be stored, yet we can recalculate it.
  wotstore  : Box<KVStore<T>>,
  /// Sign trust
  signstore : Box<KVStoreRel<<TP as KeyVal>::Key,<TP as KeyVal>::Key, PeerSign<TP>>>,
  /// Max size of promoted trust (we keep the last ones)
  promsize  : usize,
  /// Rule used to calculate trust level from PeerSign (s).
  rules     : T::Rule,
}

/// classic trust : upper trust are prioritary and you need a number of trust (see rules
/// treshold) to get promote and trust are capped to their originator trust.
#[derive(Debug, PartialEq, Eq, Clone,RustcEncodable,RustcDecodable)]
pub struct ClassicWotTrust<TP : TrustedPeer> {
  peerid : <TP as KeyVal>::Key,
  trust  : u8,
  /// associate an origniator trust (`from`) with all its signature level.
  ///
  calcmap: Vec<Vec<usize>>,
  lastdiscovery : TimeSpecExt,
}

/// Experimental wot trust
#[derive(Debug, PartialEq, Eq, Clone,RustcEncodable,RustcDecodable)]
pub struct ExpWotTrust<TP : TrustedPeer> {
  peerid : <TP as KeyVal>::Key,
  trust  : u8,
  /// associate a trust level with the current number of trust added for it indexed by their
  /// trust originator level eg if we sign about at level 1 by a from considered level 3, it is
  /// the same as signing 3 (a level cannot up to uper trust). so we increment counter of a
  /// Vec[3][3], if a level 1 sign us to 3 we increment Vec[3][1]
  /// TODO replace internal vector by int/bigint and bit arithmetic
  calcmap: Vec<Vec<usize>>,
  lastdiscovery : TimeSpecExt,
}

impl ClassicWotTrust<RSAPeer> {

  #[cfg(test)]
  pub fn wottrust_update() {
    let peer = RSAPeer::new ("myname".to_string(), None, utils::sa4(Ipv4Addr::new(127,0,0,1), 8080));
    let rules : TrustRules = vec![1,1,2,2,2];
    let mut trust : ClassicWotTrust<RSAPeer> = WotTrust::new(&peer, &rules);

    assert_eq!(<u8 as Int>::max_value(), trust.trust);
    // start at trust 4 (by level 4 updates)
    trust.update(0,<u8 as Int>::max_value(),4,4, &rules);
    assert_eq!(<u8 as Int>::max_value(), trust.trust);
    trust.update(0,<u8 as Int>::max_value(),4,4, &rules);
    assert_eq!(4, trust.trust);
    trust.update(0,5,3,3, &rules);
    assert_eq!(4, trust.trust);
    trust.update(3,3,3,3, &rules);
    assert_eq!(4, trust.trust);
    trust.update(3,4,3,2, &rules);
    assert_eq!(3, trust.trust);
    trust.update(0,4,2,2, &rules);
    assert_eq!(3, trust.trust);
    trust.update(0,4,2,2, &rules);
    assert_eq!(2, trust.trust);
    trust.update(0,0,1,0, &rules);
    assert_eq!(1, trust.trust);
    trust.update(1,0,4,0, &rules);
    assert_eq!(2, trust.trust);
    trust.update(4,4,4,4, &rules);
    assert_eq!(2, trust.trust);
    trust.update(2,2,4,4, &rules);
    assert_eq!(3, trust.trust);
    trust.update(3,3,0,2, &rules);
    assert_eq!(2, trust.trust);
    trust.update(0,2,0,4, &rules);
    assert_eq!(4, trust.trust);


    let rules2 : TrustRules = vec![1,2,2,2,2];
    let mut trust2 : ClassicWotTrust<RSAPeer>  = WotTrust::new(&peer, &rules2);
    assert_eq!(<u8 as Int>::max_value(), trust2.trust);
    // start at trust 3 (by master update)
    trust2.update(0,<u8 as Int>::max_value(),0,3, &rules2);
    assert_eq!(3, trust2.trust);
    // add a trust to two by tw update (not enough
    trust2.update(0,<u8 as Int>::max_value(),2,2, &rules2);
    assert_eq!(3, trust2.trust);
    // add a trust to one by one update (not enough but consider promoting previous 22)
    trust2.update(0,<u8 as Int>::max_value(),1,1, &rules2);
    // still 3  due to 3 give by 0
    assert_eq!(3, trust2.trust);
    // remove it as 0 and put it as 3 so only 1-1 and 2-2
    trust2.update(0,3,3,3, &rules2);
    assert_eq!(2, trust2.trust);
    // remove trust to 22 and put it to null
    trust2.update(2,2,2,<u8 as Int>::max_value(), &rules2);
    assert_eq!(3, trust2.trust);
    // lower a 3 trust to 4 by lower originator trust  
    trust2.update(0,3,4,3, &rules2);
    assert_eq!(3, trust2.trust);
    trust2.update(1,1,5,5, &rules2);
    assert_eq!(4, trust2.trust);
    // put two level 2 trust
    trust2.update(0,<u8 as Int>::max_value(),2,2, &rules2);
    trust2.update(0,<u8 as Int>::max_value(),2,2, &rules2);
    assert_eq!(2, trust2.trust);

    // two level 1 trust to lower trust than 2 level 2 is bigger than level 2 : demote to 4
    trust2.update(0,<u8 as Int>::max_value(),1,4, &rules2);
    trust2.update(0,<u8 as Int>::max_value(),1,4, &rules2);
    assert_eq!(4, trust2.trust);


  }

  /*
     pub fn wottrust_update_old() {
     let rules : TrustRules = vec![(0,1),(1,1),(2,2),(3,2),(4,2)].into_iter().collect();
     let peer = RSAPeer::new ("myname".to_string(), None, Ipv4Addr(127,0,0,1), 8000);
     let mut trust = WotTrust::new(&peer);

     assert_eq!(<u8 as Int>::max_value(), trust.trust);
// start at trust 4 (by master update)
trust.update(0,<u8 as Int>::max_value(),0,4, &rules);
assert_eq!(<u8 as Int>::max_value(), trust.trust);
trust.update(0,<u8 as Int>::max_value(),0,4, &rules);
assert_eq!(4, trust.trust);
trust.update(0,5,0,3, &rules);
assert_eq!(4, trust.trust);
trust.update(3,3,3,3, &rules);
assert_eq!(4, trust.trust);
trust.update(3,4,3,2, &rules);
assert_eq!(3, trust.trust);
trust.update(0,4,0,2, &rules);
assert_eq!(3, trust.trust);
trust.update(0,4,0,2, &rules);
assert_eq!(2, trust.trust);
trust.update(0,0,1,0, &rules);
assert_eq!(1, trust.trust);
trust.update(1,0,4,0, &rules);
assert_eq!(2, trust.trust);
trust.update(0,3,0,4, &rules);
assert_eq!(2, trust.trust);
trust.update(0,2,0,4, &rules);
assert_eq!(4, trust.trust);
}*/



}

impl ExpWotTrust<RSAPeer> {

#[cfg(test)]
  pub fn wottrust_update() {
    let peer = RSAPeer::new ("myname".to_string(), None, utils::sa4(Ipv4Addr::new(127,0,0,1), 8080));
    let rules : TrustRules = vec![1,1,2,2,2];
    let mut trust : ExpWotTrust<RSAPeer> = WotTrust::new(&peer, &rules);
    assert_eq!(<u8 as Int>::max_value(), trust.trust());
    // start at trust 4 (by level 4 updates)
    trust.update(0,<u8 as Int>::max_value(),4,4, &rules);
    assert_eq!(<u8 as Int>::max_value(), trust.trust());
    trust.update(0,<u8 as Int>::max_value(),4,4, &rules);
    assert_eq!(4, trust.trust());
    trust.update(0,5,3,3, &rules);
    assert_eq!(4, trust.trust());
    trust.update(3,3,3,3, &rules);
    assert_eq!(4, trust.trust());
    trust.update(3,4,3,2, &rules);
    assert_eq!(3, trust.trust());
    trust.update(0,4,2,2, &rules);
    assert_eq!(3, trust.trust());
    trust.update(0,4,2,2, &rules);
    assert_eq!(2, trust.trust());
    trust.update(0,0,1,0, &rules);
    assert_eq!(1, trust.trust());
    trust.update(1,0,4,0, &rules);
    assert_eq!(2, trust.trust());
    trust.update(4,4,0,4, &rules);
    assert_eq!(2, trust.trust());
    trust.update(2,2,0,4, &rules);
    trust.update(2,2,0,4, &rules);
    assert_eq!(3, trust.trust());
    trust.update(3,3,0,2, &rules);
    assert_eq!(2, trust.trust());
    trust.update(0,2,0,4, &rules);
    assert_eq!(4, trust.trust());


    let rules2 : TrustRules = vec![1,2,2,2,2];
    let mut trust2 : ExpWotTrust<RSAPeer> = WotTrust::new(&peer, &rules2);
    assert_eq!(<u8 as Int>::max_value(), trust2.trust);
    // start at trust 3 (by master update)
    trust2.update(0,<u8 as Int>::max_value(),0,3, &rules2);
    assert_eq!(3, trust2.trust);
    // add a trust to two by tw update (not enough
    trust2.update(0,<u8 as Int>::max_value(),2,2, &rules2);
    assert_eq!(3, trust2.trust);
    // add a trust to one by one update (not enough but consider promoting previous 22
    trust2.update(0,<u8 as Int>::max_value(),1,1, &rules2);
    assert_eq!(2, trust2.trust);
    // remove trust to 22 and put it to null
    trust2.update(2,2,2,<u8 as Int>::max_value(), &rules2);
    assert_eq!(3, trust2.trust);
    // lower a 3 trust to 4 by lower originator trust  
    trust2.update(0,3,4,3, &rules2);
    assert_eq!(4, trust2.trust);

  }

  /*
     pub fn wottrust_update_old() {
     let rules : TrustRules = vec![(0,1),(1,1),(2,2),(3,2),(4,2)].into_iter().collect();
     let peer = RSAPeer::new ("myname".to_string(), None, Ipv4Addr(127,0,0,1), 8000);
     let mut trust = WotTrust::new(&peer);

     assert_eq!(<u8 as Int>::max_value(), trust.trust);
// start at trust 4 (by master update)
trust.update(0,<u8 as Int>::max_value(),0,4, &rules);
assert_eq!(<u8 as Int>::max_value(), trust.trust);
trust.update(0,<u8 as Int>::max_value(),0,4, &rules);
assert_eq!(4, trust.trust);
trust.update(0,5,0,3, &rules);
assert_eq!(4, trust.trust);
trust.update(3,3,3,3, &rules);
assert_eq!(4, trust.trust);
trust.update(3,4,3,2, &rules);
assert_eq!(3, trust.trust);
trust.update(0,4,0,2, &rules);
assert_eq!(3, trust.trust);
trust.update(0,4,0,2, &rules);
assert_eq!(2, trust.trust);
trust.update(0,0,1,0, &rules);
assert_eq!(1, trust.trust);
trust.update(1,0,4,0, &rules);
assert_eq!(2, trust.trust);
trust.update(0,3,0,4, &rules);
assert_eq!(2, trust.trust);
trust.update(0,2,0,4, &rules);
assert_eq!(4, trust.trust);
}*/

}

#[test]
fn classic_wottrust_update() {
  ClassicWotTrust::wottrust_update()
}

#[test]
fn exp_wottrust_update() {
  ExpWotTrust::wottrust_update()
}

impl<TP : TrustedPeer> KeyVal for ClassicWotTrust<TP> {
  // not that good (pkey can contain private and lot a clone... TODO (for now easier this way)
  type Key = <TP as KeyVal>::Key;
  fn get_key(&self) -> <TP as KeyVal>::Key {
    self.peerid.clone()
  }
  nospecificencoding!(ClassicWotTrust<TP>);
  noattachment!();
}

impl<TP : TrustedPeer> KeyVal for ExpWotTrust<TP> {
  // not that good (pkey can contain private and lot a clone... TODO (for now easier this way)
  type Key = <TP as KeyVal>::Key;
  fn get_key(&self) -> <TP as KeyVal>::Key {
    self.peerid.clone()
  }
  nospecificencoding!(ExpWotTrust<TP>);
  noattachment!();
}


/// KeyVal to get trust for a peer
#[derive(Debug, PartialEq, Eq, Clone,RustcEncodable,RustcDecodable)]
pub struct TrustQuery<TP : TrustedPeer> {
  pub peerid : <TP as KeyVal>::Key,
  pub lastdiscovery : TimeSpecExt,
  pub trust : u8,
}

impl<TP : TrustedPeer> KeyVal for TrustQuery<TP> {
  type Key = <TP as KeyVal>::Key;
  fn get_key(&self) -> <TP as KeyVal>::Key {
    self.peerid.clone()
  }
  nospecificencoding!(TrustQuery<TP>);
  noattachment!();
}

/// treshold and number of same to promote or demote trust.
/// This mapping associate a trust level (index of the vec element) with the number of trust of this level needed to
/// promote.
/// Note that a value must be set in all valid level!!
/// eg given [1,1,2,2,2] means 4 trust level
/// means to trust level 0 one sign is enough for all
/// means to trust level 1 one sign is enough for levels bellow 0 and 0 consider as 1
/// means to trust level 2 two signs are  needed for levels bellow 1 and 0 or one consider as 2
/// ...
pub type TrustRules = Vec<usize>;

pub trait WotTrust<P : TrustedPeer> : KeyVal<Key = Vec<u8>> {
  type Rule : Send + 'static;
  fn new(p : &P, rules : &Self::Rule) -> Self;

  /// first return bool tells if wottrust change (so we update it in kvstore (no inplace kvstore
  /// update for now), second one tell if trust has changed (so we apply cascade chain of
  /// updates).
  fn update (&mut self, from_old_level : u8, from_old_trust : u8, from_new_level : u8, from_new_trust : u8, rules : &Self::Rule) -> (bool,bool);
  fn trust (&self) -> u8;
  fn lastdiscovery (&self) -> TimeSpecExt;
}


impl<TP : TrustedPeer> WotTrust<TP> for ClassicWotTrust<TP> {
  type Rule = TrustRules;
  #[inline]
  fn trust (&self) -> u8 {
    self.trust
  }
  #[inline]
  fn lastdiscovery (&self) -> TimeSpecExt {
    self.lastdiscovery.clone()
  }

  /// in calcmap, first ix is sign by, internal vec is trust
  /// That way we apply priority given from of sign
  fn new(p : &TP, rules : &TrustRules) -> ClassicWotTrust<TP> {
    ClassicWotTrust {
      peerid : p.get_key(),
      trust  : <u8 as Int>::max_value(),
      // TODO transfor internal vec to Int/Bigint being counter to every states
      // for now just stick to simple imp until stable. (with counter taking acount of bigger so
      calcmap: {
        let s = rules.len();
        (0..(s)).map(|ix|iter::repeat(0us).take(s - ix).collect()).collect()
      },
      lastdiscovery: TimeSpecExt(NULL_TIMESPEC),
    }
  }

  fn update (&mut self, from_old_trust : u8, from_old_sig : u8, from_new_trust : u8, from_new_sig : u8, rules : &TrustRules) -> (bool,bool) {
    let cap_from_old_sig = if from_old_trust > from_old_sig {from_old_trust} else {from_old_sig};
    let cap_from_new_sig = if from_new_trust > from_new_sig {from_new_trust} else {from_new_sig};
    if cap_from_old_sig == cap_from_new_sig 
        && from_old_trust == from_new_trust {
      debug!("no update {:?}, {:?}", cap_from_new_sig, cap_from_old_sig);
      (false, false)
    } else {
      let mut new_trust     = None;
      let mut decreasetrust = false;
      let mut changedcache  = false;
      let nblevel = rules.len().to_u8().unwrap();
      let mut nbtrust : Vec<usize> = iter::repeat(0us).take(rules.len()).collect();
      // first updates
      if from_old_trust < nblevel
          && from_old_trust >= 0
          && cap_from_old_sig >= from_old_trust 
          && cap_from_old_sig < nblevel {
        let ix1 = from_old_trust.to_uint().unwrap();
        let ix2 = (cap_from_old_sig - from_old_trust).to_uint().unwrap();
        if self.calcmap[ix1][ix2] > 0 {
          self.calcmap[ix1][ix2] -= 1;
          changedcache = true;
        }
      };
      if from_new_trust < nblevel
          && from_new_trust >= 0
          && cap_from_new_sig >= from_new_trust 
          && cap_from_new_sig < nblevel {
        let ix1 = from_new_trust.to_uint().unwrap();
        let ix2 = (cap_from_new_sig - from_new_trust).to_uint().unwrap();
        self.calcmap[ix1][ix2] += 1;
        changedcache = true;
      };

      if(changedcache){
        // recalculate
        let mut cur_level     = 0;
        for count in self.calcmap.iter() {
          debug!("count is {:?}, {:?}",count,cur_level);
          if new_trust.is_none() {
            let mut cum = 0;
            let mut slevel : u8 = 0;
            for inbtrust in nbtrust.iter_mut() {
              if slevel >= cur_level {
                let level_ix = slevel - cur_level;
                let icount = count.get(level_ix.to_uint().unwrap()).unwrap();
                // cumulative trust
                debug!("deb2 {:?}, {:?}, {:?}",inbtrust, icount, cum);
                *inbtrust = *inbtrust + *icount;
                cum += *inbtrust;
                let treshold = rules.get(cur_level.to_uint().unwrap()).unwrap();
                if cum >= *treshold && new_trust.is_none() {
                  new_trust = Some(slevel);
                }
              } else {
                cum += *inbtrust;
              }
              slevel += 1;
            }
          }

          debug!("nbtrust : {:?}",nbtrust);
          cur_level += 1;
        }
      };
      match new_trust {
        None => (changedcache, false),
        Some(ntrust) => {
          if ntrust == self.trust {
            debug!("final trust not changed");
            // no impact on value but calcmap has changed
            (changedcache, false)
          } else {
            debug!("final trust changed to {:?}", ntrust);
            self.trust = ntrust;
            (changedcache, true)
          }
        },
      }
    }
  }
}

impl<TP : TrustedPeer> WotTrust<TP> for ExpWotTrust<TP> {
  type Rule = TrustRules;
  #[inline]
  fn trust (&self) -> u8 {
    self.trust
  }
  #[inline]
  fn lastdiscovery (&self) -> TimeSpecExt {
    self.lastdiscovery.clone()
  }

  /// in calcmap first ix is level signed, second is trust of whom signed
  /// That way we apply priority given actual sign for whoever sign
  /// eg : a 1 trust sign at three and a 2 trust sign at 2 then the trust is 2, 
  /// it makes thing particullarily hard to revoke something. It will more look like a shared
  /// categorization of network (likely to converge to big level : we may need to apply a
  /// second type of trust to actually revoke)
  fn new(p : &TP, rules : &TrustRules) -> ExpWotTrust<TP> {
    ExpWotTrust {
      peerid : p.get_key(),
      trust  : <u8 as Int>::max_value(),
      // TODO transfor internal vec to Int/Bigint being counter to every states
      // for now just stick to simple imp until stable. (with counter taking acount of bigger so
      calcmap: (0..(rules.len())).map(|ix|iter::repeat(0us).take(ix+1).collect()).collect(),
      lastdiscovery: TimeSpecExt(NULL_TIMESPEC),
    }
  }

  fn update (&mut self, from_old_trust : u8, from_old_sig : u8, from_new_trust : u8, from_new_sig : u8, rules : &TrustRules) -> (bool,bool) {
    let cap_from_old_trust = if from_old_trust > from_old_sig {from_old_trust} else {from_old_sig};
    let cap_from_new_trust = if from_new_trust > from_new_sig {from_new_trust} else {from_new_sig};
    if cap_from_old_trust == cap_from_new_trust {
      debug!("no update {:?}, {:?}", cap_from_new_trust, cap_from_old_trust);
      (false, false)
    } else {
      let mut new_trust     = <u8 as Int>::max_value();
      let mut decreasetrust = false;
      let mut changedcache  = false;
      let mut nbtrust : Vec<usize> = iter::repeat(0us).take(rules.len()).collect();
      let mut cur_level     = 0;
      for count in self.calcmap.iter_mut() {
        if cur_level == cap_from_old_trust {
          let mut icount = count.get_mut(from_old_trust.to_uint().unwrap()).unwrap();
          if *icount > 0 {
            *icount -= 1;
            changedcache = true;
          };
        };

        if cur_level == cap_from_new_trust {
          let mut icount = count.get_mut(from_new_trust.to_uint().unwrap()).unwrap();
          *icount += 1;
          changedcache = true;
        };
        debug!("count is {:?}, {:?}",count,cur_level);

        if new_trust == <u8 as Int>::max_value(){
          let mut cum = 0;
          for ((icount, inbtrust), treshold) in count.iter().zip(nbtrust.iter_mut()).zip(rules.iter()) {
            // cumulative trust
            *inbtrust = *inbtrust + *icount;
            cum += *inbtrust;
            if !(cum < *treshold) {
              new_trust = cur_level;
            }
          }
        }

        debug!("nbtrust : {:?}",nbtrust);
        cur_level += 1;
      }

      if (new_trust == self.trust) {
        debug!("final trust not changed");
        // no impact on value but calcmap has changed
        (changedcache, false)
      } else {
        debug!("final trust changed to {:?}", new_trust);
        self.trust = new_trust;
        (changedcache, true)
      }
    }
  }

  /* 
  // awkward variant of update where uper level count is not taken as usable for lower lever
  // (kept in case we want a trust with partial hierarchic trust)
  // Need vecmap instead of vec to store state (doable easily with vec).
  fn update_old (&mut self, from_old_trust : u8, from_old_sig : u8, from_new_trust : u8, from_new_sig : u8, rules : &TrustRules) -> (bool,bool) {
  let cap_from_old_trust = if from_old_trust > from_old_sig {from_old_trust} else {from_old_sig};
  let cap_from_new_trust = if from_new_trust > from_new_sig {from_new_trust} else {from_new_sig};
  if cap_from_old_trust == cap_from_new_trust {
  println!("no update {:?}, {:?}", cap_from_new_trust, cap_from_old_trust);
  (false,false)
  } else {
  let mut new_trust = self.trust;
  let mut decreasetrust = false;
  let mut changedcache  = false;
  // remove old trust and see if decrease, if no treshold it is an unmanaged trust so nothing
  // to do
  rules.get(&cap_from_old_trust.to_uint().unwrap()).map (|treshold|
  // TODO test under 0 ??? (means bug)
  // if nothing we don't decrease
  self.calcmap.get_mut(&cap_from_old_trust.to_uint().unwrap()).map(|mut oldtrustnb|{
  println!("rem old");
   *oldtrustnb -= 1;
   println!("new val : {:?}",*oldtrustnb);
   changedcache = true;
   if (*oldtrustnb < *treshold) {
// new_trust being self.trust
if (new_trust == cap_from_old_trust) {
decreasetrust = true;
};
};
})
);

// add new trust and see if promoted
rules.get(&cap_from_new_trust.to_uint().unwrap()).map (|treshold|{
let (add, nb) = match self.calcmap.get_mut(&cap_from_new_trust.to_uint().unwrap()){
Some(mut oldtrustnb) => {
println!("add new");
   *oldtrustnb += 1;
   (false,*oldtrustnb)
   }
   None => {
   (true,1)
   }
   };
   if add {
   self.calcmap.insert(cap_from_new_trust.to_uint().unwrap(), nb);
   };
   changedcache = true;
   if !(nb < *treshold) {
   println!("new val over tresh");
   if (cap_from_new_trust < new_trust){
   println!("tresh as newval");
   decreasetrust = false;
   new_trust = cap_from_new_trust;
   };
   };
   });
// actually decrease trust
if decreasetrust {
println!("do decrease");
// found in cache next lower correct treshold
let mut stop = false;
let mut nextrust = self.trust;
new_trust = <u8 as Int>::max_value();
while !stop {
nextrust += 1;
stop = true;
rules.get(&nextrust.to_uint().unwrap()).map (|treshold|
self.calcmap.get_mut(&nextrust.to_uint().unwrap()).map(|nb|
if *nb < *treshold {
stop = false;
} else {
  new_trust = nextrust;
  stop = true;
}
)
);
}
}
if (new_trust == self.trust) {
  println!("final trust not changed");
  // no impact on value but calcmap has changed
  (changedcache, false)
} else {
  println!("final trust changed to {:?}", new_trust);
  self.trust = new_trust;
  (changedcache, true)
}
}
}*/
}

impl<TP : TrustedPeer, T : WotTrust<TP>> WotStore<TP, T> {
  pub fn new
  <S1 : KVStore<ArcKV<TP>>, 
  S2 : KVStore<PromSigns<TP>>, 
  S3 : KVStoreRel<Vec<u8>,Vec<u8>,PeerSign<TP>>,
  S4 : KVStore<T>
  > 
  (mut s1 : S1, mut s2 : S2, mut s3 : S3,  mut s4 : S4, ame : ArcKV<TP>, psize : usize, rules : T::Rule) 
  -> WotStore<TP, T> {
    let me = &(*ame);
    let psignme : PeerSign<TP> = PeerSign::new(&ame, &ame, 0, 0).unwrap();
    // add me with manually set trust
    let mut me2 = me.clone();
    let mut trust : T = WotTrust::new(me, &rules);
    // update from non existing 0 peer from non existing 1 sign to non existing 0 peer :
    // consequently update of 0 calculate direct level as 0 (plus changing)
    trust.update(0,1,0,0, &rules);
    //      me2.trust = 0;
    s4.add_val(trust, (true,None));
    s1.add_val(ArcKV::new(me2), (true,None));
    // add max trust
    s3.add_val(psignme, (true,None));

    WotStore {
      peerstore : Box::new(s1),
      promstore : Box::new(s2),
      signstore : Box::new(s3),
      wotstore  : Box::new(s4),
      promsize  : psize,
      rules     : rules,
    }
  }

  pub fn get_peer_trust(&self, key : &T::Key) -> u8 {
    self.wotstore.get_val(key).map(|p|p.trust()).unwrap_or(<u8 as Int>::max_value())
  }

  pub fn clean (){
    // TODO 
    // remove peersign with no more peer
    // remove promote with no more peer
    // remove peersign with a latter version (with actual key their is no way we got two
    // recalculate all trust
  }
  // update trust on new sign and recursivly propagate
  // currently update wot implementation must converge as we 
  // got no mechanism to avoid infinite recursion TODO max recursion depth???
  fn update_trust (&mut self, skv : &PeerSign<TP>) {
    let otrust = self.wotstore.get_val(&skv.about).or_else(||{
        match self.peerstore.get_val(&skv.about) {
        None => {
        info!("trust about non known peer is ignored yet stored");
        None
        },
        // new trust
        Some(apeer) => {
        // should not happen but yet
        let tmp = WotTrust::new(&(*apeer), &self.rules);
//        let trust : Arc<T> = Arc::new(tmp);
        let trust : T = tmp;
        self.wotstore.add_val(trust.clone(), (true,None));
        Some(trust)
        },
        }
        }
        );
    otrust.map(|trust|{
        // TODO in place update of kvstore to possibly avoid clone
        let mut newtrust = (trust).clone();
        let from_trust = self.get_peer_trust(&skv.from);
        debug!("update with {:?}, {:?}, {:?}, {:?}", from_trust, trust.trust(), from_trust, skv.trust);
        let (upd, pro) = newtrust.update(from_trust, trust.trust(), from_trust, skv.trust, &self.rules);
        if upd {
        self.wotstore.add_val(newtrust, (true,None));
        };
        if pro {
        // TODO trust has change, do update all inpacted sign from this user
        for si in self.signstore.get_vals_from_left(&trust.get_key()){
        // TODO document on tail rec opt with rust
        self.update_trust(&(si));
        }

        };
        });


  }



}

/*  pub type KVNodeStore = Box<KVStore<TrustedPeer>>;
    pub type KVRelStore  = Box<KVStore<PromSigns>>;
    pub type KVSignStore = Box<KVStore<PeerSign>>;
    derive_kvstore!(WotStore, WotKV, WotK, {
    peerstore => KVNodeStore,
    promstore  => KVRelStore,
    signstore => KVSignStore,
    },
    {
    Peer   => peerstore,
    P2S    => promstore,
    Sign   => signstore,
    }
    );*/
// TODO trait for relation store, with get val with two param (from and with), set val with two
// param , remove val with two param -> to avoid getting/updatting a list of everything everytime
// TODO make it multiple key (more generic)

impl<TP : TrustedPeer, T : WotTrust<TP>> KVStore<WotKV<TP>> for WotStore<TP, T> {

  // TODO arc make no sense
  fn add_val(& mut self,  kv : WotKV<TP>, stconf : (bool, Option<CachePolicy>)){
    // TODO !!!! calculate and update trust  + TODO wotstore primitive to change a peer trust!!!
    match kv {
      WotKV::Peer(ref skv) => {
        // do not store invalid peer Peer info
        if skv.key_check() && skv.check_val(skv) {
          self.peerstore.add_val(skv.clone(), stconf);
        } else {
          error!("Trying to store forged peer value in wot store");
        }
      },
        WotKV::P2S (ref skv) => {
          // updated through sign added (here for manual update but may be misused by doing find
          // with storing
          // So we merge add to sign
          match self.promstore.get_val(&skv.get_key()) {
            None => {
              // add as is
              self.promstore.add_val(skv.clone(), stconf);
            },
            Some (xpval) => {
              // add all promsign only
              let new = false;
              let newrel = xpval.merge(&skv).unwrap_or(skv.clone());
              self.promstore.add_val(newrel, stconf);
            },

          };
        },
        WotKV::Sign(ref skv) => {
          match self.peerstore.get_val(&skv.from) {
            None => (),
            Some(from) => {
              let e = skv.get_sign();

              if (skv.check_val(&(from))){
                // TODO some conflict mgmt in keystore
                match self.signstore.get_val(&skv.get_key()) {
                  None => debug!("NNOOO signstore before"),
                  Some(v) => debug!("Signstore before : {:?} new : {:?}", v.tag, skv.tag),
                };
                let newerxistingtag = self.signstore.get_val(&skv.get_key()).map(|v|v.tag > skv.tag);
                debug!("strange : {:?}", newerxistingtag);
                if newerxistingtag.unwrap_or(false) {
                  debug!("new sign with older tag than existing one : not added");
                } else {
                  // TODO here we add even if about is unknown (good??)
                  // Here validation of signature is done by underlyingstore
                  self.signstore.add_val(skv.clone(), stconf);
                  // TODO avoid doing a get val by returning something with add_val
                  match self.promstore.get_val(&skv.about) {
                    None => {
                      debug!("add to promstore");
                      self.promstore.add_val(
                            PromSigns::new(skv, self.promsize)
                            , stconf)
                    },
                         Some (rel) => {

                           debug!("update xisting promstore");
                           match rel.add_sign(&skv) {
                             None => (),
                                  Some(nrel) => self.promstore.add_val(nrel, stconf),
                           }
                         }

                  };

                  // update skv.about trust
                  self.update_trust(skv);

                }
              } else {
                // TODO further action on error
                error!("Received invalid trust signature!!");
              }
            },
          }
        },
        WotKV::TrustQuery(ref skv) => {
          // nothing to do (get only struct).
          // add is done by adding sign or when receiving sign subsequent to find or discover
          // operation
        },

    };
  }


  fn get_val(& self, k : &<WotKV<TP> as KeyVal>::Key) -> Option<WotKV<TP>> {
    match k {
      &WotK::Peer(ref sk) => {
        self.peerstore.get_val(sk).map(|ask| (WotKV::Peer((ask).clone())))
      },
        &WotK::P2S (ref sk) => {
          // only use of P2S
          // TODO expand relation with peers?? -> P2S become another type + its add and rem (not for
          // unknown peer or at lower prio -> not good (it should be recalculated and this is done
          // outside wotstore)
          // TODO some sign may not exist anymore -> do a clean job for dangling pointer or costy
          // remove processes
          match self.promstore.get_val(sk) {
            None => None,
                 Some(ask) => {
                   if ask.signby.len() > 0 {
                     Some((WotKV::P2S((ask).clone())))
                   } else {
                     None
                   }
                 },
          }
        },
        &WotK::Sign(ref sk) => {
          self.signstore.get_val(sk).map(|ask| WotKV::Sign((ask).clone()))
        },
        &WotK::TrustQuery(ref sk) => {
          self.wotstore.get_val(sk).map(|ask| WotKV::TrustQuery(TrustQuery{
peerid : sk.clone(),
lastdiscovery : ask.lastdiscovery(),
trust : ask.trust(),
}))
},

  }
}

fn remove_val(& mut self, k : &<WotKV<TP> as KeyVal>::Key) {
  match k {
    &WotK::Peer(ref sk) => {
      self.peerstore.remove_val(sk);
      self.promstore.remove_val(sk)
        // no right way to remove relation in the other direction : bad
    },
      &WotK::P2S (ref sk) => {
        //self.promstore.remove_val(sk)
        // Done with peer removal
        ()
      },
      &WotK::Sign(ref sk) => {
        match self.signstore.get_val(sk){
          None => (),
          Some (skval) => 
            match self.promstore.get_val(&skval.about) {
              None => (),
              Some (rel) => {
                match rel.remove_sign(&(skval)) {
                  None => (),
                  Some(nrel) => self.promstore.add_val(nrel,(true,None)), // TODO here cache policy reset seems wrong
                }
              }
            },
        };
        self.signstore.remove_val(sk);
        // recompute trust
        let otrust = self.wotstore.get_val(&sk.1);
        otrust.map(|trust|{
            // TODO in place update of kvstore to possibly avoid clone
            let mut newtrust = (trust).clone();
            let from_trust = self.get_peer_trust(&sk.0);
            let (upd, pro) = newtrust.update(from_trust, trust.trust(), from_trust, <u8 as Int>::max_value(), &self.rules);
            if upd {
            self.wotstore.add_val(newtrust, (true,None));
            };
            if pro {
            // TODO trust has change, do update all inpacted sign from this user
            };
            });

      },
      &WotK::TrustQuery(ref sk) => {
        // nothing to do (get only struct)
      },

  }
}
#[inline]
fn commit_store(& mut self) -> bool{
  let mut r = true;
  r = self.peerstore.commit_store() && r;
  r = self.signstore.commit_store() && r;
  r = self.wotstore.commit_store() && r;
  r = self.promstore.commit_store() && r;
  r
}
}

#[cfg(test)]
fn addpeer_test<T : WotTrust<RSAPeer>> (wotstore : &mut WotStore<RSAPeer, T>, name : String)-> ArcKV<RSAPeer>{
  let stconf = (true,None);
  let pe = ArcKV::new(RSAPeer::new (name, None, utils::sa4(Ipv4Addr::new(127,0,0,1), 8080) ));
  wotstore.add_val(WotKV::Peer(pe.clone()) ,stconf);

  let qme = wotstore.get_val(&WotK::Peer(pe.get_key())).unwrap();
  assert_eq!(qme.get_key(), WotK::Peer(pe.get_key()));
  pe
}

fn addpeer_trust<T : WotTrust<RSAPeer>> (by : &ArcKV<RSAPeer>, about : &ArcKV<RSAPeer>, wotstore : &mut WotStore<RSAPeer, T>, trust : u8, testtrust : u8, tag : Option<usize>){
  let tagv = tag.unwrap_or(0);

  let stconf = (true,None);
  let psignme : PeerSign<RSAPeer> = PeerSign::new(&(*by), &(*about), trust, tagv).unwrap();
  wotstore.add_val(WotKV::Sign(psignme.clone()) ,stconf);

  let qmesign = wotstore.get_val(&WotK::Sign(psignme.get_key())).unwrap();
  assert_eq!(qmesign.get_key(), WotK::Sign(psignme.get_key()));

  let metrus = wotstore.get_peer_trust(&about.get_key());
  // enable when update of trust ok
  assert_eq!(testtrust, metrus);
}

#[test]
fn test_wot(){
  // initiate with simple cache map
  let mut wotpeers = SimpleCache::new(None);
  let mut wotrels  = SimpleCache::new(None);
  let mut wotsigns = SimpleCache::new(None);
  let mut wotrusts : SimpleCache<ClassicWotTrust<RSAPeer>> = SimpleCache::new(None);
  let mut trustRul : TrustRules = vec![1,1,2,2,2];

  let me = ArcKV::new(RSAPeer::new ("myname".to_string(), None, utils::sa4(Ipv4Addr::new(127,0,0,1), 8080) ));
  let mut wotstore = WotStore::new(
      wotpeers,
      wotrels,
      wotsigns,
      wotrusts,
      me.clone(),
      100,
      trustRul,
      );

  let stconf = (true,None);

  // check master trust
  let metrus = wotstore.get_peer_trust(&me.get_key());
  assert_eq!(0, metrus);

  // add other peer and their trust
  let peer1 = addpeer_test(&mut wotstore,"peer1".to_string());
  let peer2 = addpeer_test(&mut wotstore,"peer2".to_string());
  let peer3 = addpeer_test(&mut wotstore,"peer3".to_string());
  let peer4 = addpeer_test(&mut wotstore,"peer4".to_string());
  // try update of trust
  addpeer_trust(&me, &peer1, &mut wotstore, 2, 2, None);
  // max peer trust
  addpeer_trust(&me, &peer1, &mut wotstore, 1, 1, Some(1));
  // try update of trust do not update with lower tag
  addpeer_trust(&me, &peer1, &mut wotstore, 2, 1, Some(0));

  // moderate peer trust (need two ok, or one for lower trust)
  addpeer_trust(&me, &peer2, &mut wotstore, 2, 2, None);
  addpeer_trust(&me, &peer3, &mut wotstore, 2, 2, None);
  // low peer trust (need three ok, or one for lower trust)
  addpeer_trust(&me, &peer4, &mut wotstore, 3, 3, None);

  // add trust from peer to peers
  // one is not enough to promote
  addpeer_trust(&peer2, &peer4, &mut wotstore, 2, 3, None);
  // two is ok 
  // remove 0 trust to 3 (using out of range trust) // TODO change by remove
  addpeer_trust(&me, &peer4, &mut wotstore, 5, 3, None);
  addpeer_trust(&peer3, &peer4, &mut wotstore, 2, 2, None);
  // yet cap promote at those trust
  addpeer_trust(&peer2, &peer4, &mut wotstore, 1, 2, None);
  addpeer_trust(&peer3, &peer4, &mut wotstore, 1, 2, None);
  // but if peer2 promoted
  addpeer_trust(&me, &peer2, &mut wotstore, 1, 1, None);
  // its previous trust sign apply
  let p4prom = wotstore.get_peer_trust(&peer4.get_key());
  assert_eq!(1, p4prom);

  // TODO set peer1 to 2 will lower it p4 to 1 to


  // remove peer

  // check no get

  // TODO check no get trust sign?? (when mult key and auto remove)

  // remove trust

  // check trust of peers is the same (no recalcul)

  // TODO add existing trust with higher tag

  // TODO check get val use higher

  // TODO check new user trust take account of new val plus removal of previous  

  // test add invalid user fails (no get)

  // test add invalid trust fail (no get) - invalid due to missing user

  // test add invalid trust fail (no get) - invalid due to wrong signature

  // sign me as revoked
  let revokeme : PeerSign<RSAPeer> = PeerSign::new(&(me), &(me), <u8 as Int>::max_value(), 1).unwrap();
  wotstore.add_val(WotKV::Sign(revokeme.clone()) ,stconf);

  // test trust

  // test all other trust TODO does it impact existing (it should but in long term distributed
  // not here) or TODO redesign trust storage to keep trace of its calculus also true for any
  // update




}



