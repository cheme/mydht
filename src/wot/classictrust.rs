use keyval::{KeyVal,Key,Attachment,SettableAttachment};
use rustc_serialize::{Encodable, Encoder, Decoder};
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



/// classic trust : upper trust are prioritary and you need a number of trust (see rules
/// treshold) to get promote and trust are capped to their originator trust.
#[derive(Debug, PartialEq, Eq, Clone,RustcEncodable,RustcDecodable)]
pub struct ClassicWotTrust<TP : KeyVal<Key=Vec<u8>>> {
  peerid : <TP as KeyVal>::Key,
  trust  : u8,
  /// associate an origniator trust (`from`) with all its signature level.
  ///
  calcmap: Vec<Vec<usize>>,
  lastdiscovery : TimeSpecExt,
}

impl<TP : KeyVal<Key=Vec<u8>>> KeyVal for ClassicWotTrust<TP> {
  // not that good (pkey can contain private and lot a clone... TODO (for now easier this way)
  type Key = <TP as KeyVal>::Key;
  fn get_key(&self) -> <TP as KeyVal>::Key {
    self.peerid.clone()
  }
  noattachment!();
}
impl<TP : KeyVal<Key=Vec<u8>>> SettableAttachment for ClassicWotTrust<TP> {}

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


// TODO replace with a dumy TrustedPeer??
#[cfg(test)]
#[cfg(feature="openssl-impl")]
impl ClassicWotTrust<RSAPeer> {

  pub fn wottrust_update() {
    let peer = RSAPeer::new ("myname".to_string(), None, utils::sa4(Ipv4Addr::new(127,0,0,1), 8080));
    let rules : TrustRules = vec![1,1,2,2,2];
    let mut trust : ClassicWotTrust<RSAPeer> = WotTrust::new(&peer, &rules);

    assert_eq!(<u8 as Bounded>::max_value(), trust.trust);
    // start at trust 4 (by level 4 updates)
    trust.update(0,<u8 as Bounded>::max_value(),4,4, &rules);
    assert_eq!(<u8 as Bounded>::max_value(), trust.trust);
    trust.update(0,<u8 as Bounded>::max_value(),4,4, &rules);
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
    assert_eq!(<u8 as Bounded>::max_value(), trust2.trust);
    // start at trust 3 (by master update)
    trust2.update(0,<u8 as Bounded>::max_value(),0,3, &rules2);
    assert_eq!(3, trust2.trust);
    // add a trust to two by tw update (not enough
    trust2.update(0,<u8 as Bounded>::max_value(),2,2, &rules2);
    assert_eq!(3, trust2.trust);
    // add a trust to one by one update (not enough but consider promoting previous 22)
    trust2.update(0,<u8 as Bounded>::max_value(),1,1, &rules2);
    // still 3  due to 3 give by 0
    assert_eq!(3, trust2.trust);
    // remove it as 0 and put it as 3 so only 1-1 and 2-2
    trust2.update(0,3,3,3, &rules2);
    assert_eq!(2, trust2.trust);
    // remove trust to 22 and put it to null
    trust2.update(2,2,2,<u8 as Bounded>::max_value(), &rules2);
    assert_eq!(3, trust2.trust);
    // lower a 3 trust to 4 by lower originator trust  
    trust2.update(0,3,4,3, &rules2);
    assert_eq!(3, trust2.trust);
    trust2.update(1,1,5,5, &rules2);
    assert_eq!(4, trust2.trust);
    // put two level 2 trust
    trust2.update(0,<u8 as Bounded>::max_value(),2,2, &rules2);
    trust2.update(0,<u8 as Bounded>::max_value(),2,2, &rules2);
    assert_eq!(2, trust2.trust);

    // two level 1 trust to lower trust than 2 level 2 is bigger than level 2 : demote to 4
    trust2.update(0,<u8 as Bounded>::max_value(),1,4, &rules2);
    trust2.update(0,<u8 as Bounded>::max_value(),1,4, &rules2);
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

impl<TP : KeyVal<Key=Vec<u8>>> WotTrust<TP> for ClassicWotTrust<TP> {
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
      trust  : <u8 as Bounded>::max_value(),
      // TODO transfor internal vec to Int/Bigint being counter to every states
      // for now just stick to simple imp until stable. (with counter taking acount of bigger so
      calcmap: {
        let s = rules.len();
        (0..(s)).map(|ix|vec![0usize; s - ix]).collect()
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
      let mut changedcache  = false;
      let nblevel = rules.len().to_u8().unwrap();
      let mut nbtrust : Vec<usize> = vec![0usize; rules.len()];
      // first updates
      if from_old_trust < nblevel
          && from_old_trust >= 0
          && cap_from_old_sig >= from_old_trust 
          && cap_from_old_sig < nblevel {
        let ix1 = from_old_trust.to_usize().unwrap();
        let ix2 = (cap_from_old_sig - from_old_trust).to_usize().unwrap();
        if self.calcmap[ix1][ix2] > 0 {
          self.calcmap[ix1][ix2] -= 1;
          changedcache = true;
        }
      };
      if from_new_trust < nblevel
          && cap_from_new_sig >= from_new_trust 
          && cap_from_new_sig < nblevel {
        let ix1 = from_new_trust.to_usize().unwrap();
        let ix2 = (cap_from_new_sig - from_new_trust).to_usize().unwrap();
        self.calcmap[ix1][ix2] += 1;
        changedcache = true;
      };

      if changedcache {
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
                let icount = count.get(level_ix.to_usize().unwrap()).unwrap();
                // cumulative trust
                debug!("deb2 {:?}, {:?}, {:?}",inbtrust, icount, cum);
                *inbtrust = *inbtrust + *icount;
                cum += *inbtrust;
                let treshold = rules.get(cur_level.to_usize().unwrap()).unwrap();
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



#[test]
#[cfg(feature="openssl-impl")]
fn classic_wottrust_update() {
  ClassicWotTrust::wottrust_update()
}


