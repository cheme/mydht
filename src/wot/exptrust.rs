use kvstore::{KeyVal,Key,Attachment};
use rustc_serialize::{Encodable, Decodable, Encoder, Decoder};
use super::{TrustedPeer};
use std::iter;
use utils::TimeSpecExt;
use utils::NULL_TIMESPEC;
use super::WotTrust;
use num::traits::{Bounded,ToPrimitive};
#[cfg(test)]
use utils;
#[cfg(test)]
use std::net::Ipv4Addr; 
#[cfg(test)]
use query::simplecache::SimpleCache;

#[cfg(test)]
#[cfg(feature="openssl-impl")]
use super::rsa_openssl::RSAPeer;



use super::classictrust::TrustRules;

/// Experimental wot trust
#[derive(Debug, PartialEq, Eq, Clone,RustcEncodable,RustcDecodable)]
pub struct ExpWotTrust<TP : KeyVal<Key = Vec<u8>>> {
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


impl<TP : KeyVal<Key=Vec<u8>>> KeyVal for ExpWotTrust<TP> {
  // not that good (pkey can contain private and lot a clone... TODO (for now easier this way)
  type Key = <TP as KeyVal>::Key;
  fn get_key(&self) -> <TP as KeyVal>::Key {
    self.peerid.clone()
  }
  nospecificencoding!(ExpWotTrust<TP>);
  noattachment!();
}


// TODO do crypto test and striplet test to
#[cfg(test)]
#[cfg(feature="openssl-impl")]
impl ExpWotTrust<RSAPeer> {

  pub fn wottrust_update() {
    let peer = RSAPeer::new ("myname".to_string(), None, utils::sa4(Ipv4Addr::new(127,0,0,1), 8080));
    let rules : TrustRules = vec![1,1,2,2,2];
    let mut trust : ExpWotTrust<RSAPeer> = WotTrust::new(&peer, &rules);
    assert_eq!(<u8 as Bounded>::max_value(), trust.trust());
    // start at trust 4 (by level 4 updates)
    trust.update(0,<u8 as Bounded>::max_value(),4,4, &rules);
    assert_eq!(<u8 as Bounded>::max_value(), trust.trust());
    trust.update(0,<u8 as Bounded>::max_value(),4,4, &rules);
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
    assert_eq!(<u8 as Bounded>::max_value(), trust2.trust);
    // start at trust 3 (by master update)
    trust2.update(0,<u8 as Bounded>::max_value(),0,3, &rules2);
    assert_eq!(3, trust2.trust);
    // add a trust to two by tw update (not enough
    trust2.update(0,<u8 as Bounded>::max_value(),2,2, &rules2);
    assert_eq!(3, trust2.trust);
    // add a trust to one by one update (not enough but consider promoting previous 22
    trust2.update(0,<u8 as Bounded>::max_value(),1,1, &rules2);
    assert_eq!(2, trust2.trust);
    // remove trust to 22 and put it to null
    trust2.update(2,2,2,<u8 as Bounded>::max_value(), &rules2);
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

     assert_eq!(<u8 as Bounded>::max_value(), trust.trust);
// start at trust 4 (by master update)
trust.update(0,<u8 as Bounded>::max_value(),0,4, &rules);
assert_eq!(<u8 as Bounded>::max_value(), trust.trust);
trust.update(0,<u8 as Bounded>::max_value(),0,4, &rules);
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


impl<TP : KeyVal<Key=Vec<u8>>> WotTrust<TP> for ExpWotTrust<TP> {
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
      trust  : <u8 as Bounded>::max_value(),
      // TODO transfor internal vec to Int/Bigint being counter to every states
      // for now just stick to simple imp until stable. (with counter taking acount of bigger so
      calcmap: (0..(rules.len())).map(|ix|vec![0usize; ix+1]).collect(),
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
      let mut new_trust     = <u8 as Bounded>::max_value();
      let mut decreasetrust = false;
      let mut changedcache  = false;
      let mut nbtrust : Vec<usize> = vec![0usize; rules.len()];
      let mut cur_level     = 0;
      for count in self.calcmap.iter_mut() {
        if cur_level == cap_from_old_trust {
          let mut icount = count.get_mut(from_old_trust.to_usize().unwrap()).unwrap();
          if *icount > 0 {
            *icount -= 1;
            changedcache = true;
          };
        };

        if cur_level == cap_from_new_trust {
          let mut icount = count.get_mut(from_new_trust.to_usize().unwrap()).unwrap();
          *icount += 1;
          changedcache = true;
        };
        debug!("count is {:?}, {:?}",count,cur_level);

        if new_trust == <u8 as Bounded>::max_value(){
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
  rules.get(&cap_from_old_trust.to_usize().unwrap()).map (|treshold|
  // TODO test under 0 ??? (means bug)
  // if nothing we don't decrease
  self.calcmap.get_mut(&cap_from_old_trust.to_usize().unwrap()).map(|mut oldtrustnb|{
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
rules.get(&cap_from_new_trust.to_usize().unwrap()).map (|treshold|{
let (add, nb) = match self.calcmap.get_mut(&cap_from_new_trust.to_usize().unwrap()){
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
   self.calcmap.insert(cap_from_new_trust.to_usize().unwrap(), nb);
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
new_trust = <u8 as Bounded>::max_value();
while !stop {
nextrust += 1;
stop = true;
rules.get(&nextrust.to_usize().unwrap()).map (|treshold|
self.calcmap.get_mut(&nextrust.to_usize().unwrap()).map(|nb|
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


#[test]
#[cfg(feature="openssl-impl")]
fn exp_wottrust_update() {
  ExpWotTrust::wottrust_update()
}


