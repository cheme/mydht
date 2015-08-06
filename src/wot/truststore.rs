//! Trustore and its undelying types.



use rustc_serialize::{Encoder,Encodable,Decoder};
use std::cmp::Eq;
use num::traits::Bounded;
use std::collections::VecDeque;

use keyval::{Key,KeyVal,Attachment,SettableAttachment};
use kvstore::{KVStore,KVStoreRel};
use kvcache::{KVCache,NoCache};
use query::cache::CachePolicy;
use utils::ArcKV;
use utils::TimeSpecExt;
use time::Timespec;

use super::WotTrust;
use super::PeerInfoRel;
use super::{TrustedPeer,Truster,TrustedVal,PeerTrustRel};
use super::trustedpeer::PeerSign;
//use std::iter::Iterator;
use std::collections::hash_map::Iter as HMIter;

#[cfg(test)]
use utils;
#[cfg(test)]
use super::classictrust::ClassicWotTrust;
#[cfg(test)]
use super::classictrust::TrustRules;
#[cfg(test)]
use query::simplecache::SimpleCache;
#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
#[cfg(feature="openssl-impl")]
use super::rsa_openssl::RSAPeer;
#[cfg(test)]
use std::net::Ipv4Addr; 
#[cfg(test)]
use peer::Peer; 



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

impl<TP : TrustedPeer> SettableAttachment for WotKV<TP> {
  derive_enum_setattach_inner!(WotKV<TP>, WotKV, WotK, WotK, {
    0 , Peer       => ArcKV<TP>,
    1 , P2S        => PromSigns<TP>,
    2 , Sign       => PeerSign<TP>,
    3 , TrustQuery => TrustQuery<TP>,
  });
}


//type BoxedStore<V> = Box<KVStore<V, Cache = KVCache<K = <V as KeyVal>::Key, V = V>>>;
type BoxedStore<V> = Box<KVStore<V>>;
//type BoxedStoreRel<K1,K2,V> = Box<KVStoreRel<K1,K2,V, Cache = KVCache<K = <V as KeyVal>::Key, V = V>>>;
type BoxedStoreRel<K1,K2,V> = Box<KVStoreRel<K1,K2,V>>;

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
/// TODO some store should be replace by KVCache
/// TODO get rid of BoxedStore?? (no heap no 'static).
pub struct WotStore<TP : TrustedPeer, T : WotTrust<TP>> {
  /// Peer indexed by PeerId
  peerstore : BoxedStore<ArcKV<TP>>,
  /// Special KeyVal to query signature about a peer.
  promstore : BoxedStore<PromSigns<TP>>,
  /// Cache of curent trust calculation state (and trust level)
  /// This may be stored, yet we can recalculate it.
  wotstore  : BoxedStore<T>,
  /// Sign trust
  signstore : BoxedStoreRel<<TP as KeyVal>::Key,<TP as KeyVal>::Key, PeerSign<TP>>,
  /// Max size of promoted trust (we keep the last ones)
  promsize  : usize,
  /// Rule used to calculate trust level from PeerSign (s).
  rules     : T::Rule,
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
  /* 
  #[inline]
  fn get_key_ref<'a>(&'a self) -> &'a <TP as KeyVal>::Key {
    &self.peerid
  }*/
  
  #[inline]
  fn get_key(&self) -> <TP as KeyVal>::Key {
    self.peerid.clone()
  }
  noattachment!();
}

impl<TP : TrustedPeer> SettableAttachment for TrustQuery<TP> { }


/// Special keyval for queries of signature about a peer.
/// TODO consider including signature in it (not for local storage but as reply)
#[derive(Debug, PartialEq, Eq, Clone,RustcEncodable,RustcDecodable)]
pub struct PromSigns<TP : TrustedPeer> {
  /// peer concerned
  peer : <TP as KeyVal>::Key,
  /// points to key of PeerSign information with firt value being peer id and second peersign id
  pub signby : VecDeque<(<TP as KeyVal>::Key, <TP as KeyVal>::Key)>,
  /// max nb of signs (latest are kept) TODO see if global possible
  pub maxsig : usize,
}

impl<TP : TrustedPeer> PromSigns<TP> {
  pub fn new(s : &PeerSign<TP>, ms : usize) -> PromSigns<TP> {
    PromSigns {
      peer : s.about().clone(),
      signby : { 
        let mut sb = VecDeque::new();
        sb.push_front(s.get_key());
        sb
      },
      maxsig : ms,
    }
  }
  // highly inefficient TODO see redesign keyval with multiple key
  pub fn add_sign(&self, s : &PeerSign<TP>) -> Option<PromSigns<TP>> {
    if self.signby.iter().any(|sb| sb.0 == s.get_key().0 && sb.1 == s.get_key().1) {
      None
    } else {
      let mut newsignby = self.signby.clone();
      newsignby.push_front(s.get_key());
      if newsignby.len() > self.maxsig {
        newsignby.pop_back();
      };
      Some(PromSigns {
        peer   : self.peer.clone(),
        signby : newsignby,
        maxsig : self.maxsig,
      })
    }
  }

  pub fn merge(&self, psigns : &PromSigns<TP>) -> Option<PromSigns<TP>> {
    if self.peer == psigns.peer {
      None
    } else {
      let mut change = false;
      let mut newsignby = self.signby.clone();
      for s in self.signby.iter() {
        if !psigns.signby.iter().any(|v| v == s) { 
          newsignby.push_front(s.clone());
          change = true;
        };
      };
      // same as n time pop back
      newsignby.truncate(self.maxsig);

      if change {
        Some(PromSigns {
          peer : self.peer.clone(),
          signby : newsignby,
          maxsig : self.maxsig,
        })
      } else {
        None
      }
    }
  }

  // highly inefficient
  pub fn remove_sign(&self, s : &PeerSign<TP>) -> Option<PromSigns<TP>> {
    match self.signby.iter().position(|sb| sb.0 == s.get_key().0 && sb.1 == s.get_key().1) {
      Some(ix) => {
        let mut newsignby = self.signby.clone();
        newsignby.remove(ix);
        Some(PromSigns {
          peer : self.peer.clone(),
          signby : newsignby,
          maxsig : self.maxsig,
        })
      },
      None => None,
    }
  }

}

impl<TP : TrustedPeer> KeyVal for PromSigns<TP> {
  // not that good (pkey can contain private and lot a clone... TODO (for now easier this way)
  type Key = <TP as KeyVal>::Key;
/*  #[inline]
  fn get_key_ref<'a>(&'a self) -> &'a <TP as KeyVal>::Key {
    &self.peer
  }
*/ 
  #[inline]
  fn get_key(&self) -> <TP as KeyVal>::Key {
    self.peer.clone()
  }

  // TODO may be faster to have distant encoding embedding PeerSign info yet require reference to
  // other store in keyval for encoding.
  noattachment!();
}

impl<TP : TrustedPeer> SettableAttachment for PromSigns<TP> {}

impl<TP : TrustedPeer, T : WotTrust<TP>> WotStore<TP, T> {
  pub fn new
  <S1 : KVStore<ArcKV<TP>> + 'static,
  S2 : KVStore<PromSigns<TP>> + 'static,
  S3 : KVStoreRel<Vec<u8>,Vec<u8>,PeerSign<TP>> + 'static,
  S4 : KVStore<T> + 'static,
  > 
  (mut s1 : S1, s2 : S2, mut s3 : S3,  mut s4 : S4, ame : ArcKV<TP>, psize : usize, rules : T::Rule) 
  -> WotStore<TP, T> {
    let me = &(*ame);
    let psignme : PeerSign<TP> = PeerSign::new(&ame, &ame, 0, 0).unwrap();
    // add me with manually set trust
    let me2 = me.clone();
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
    self.wotstore.get_val(key).map(|p|p.trust()).unwrap_or(<u8 as Bounded>::max_value())
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
    let otrust = self.wotstore.get_val(skv.about()).or_else(||{
        match self.peerstore.get_val(skv.about()) {
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
        let from_trust = self.get_peer_trust(skv.from());
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

impl<TP : TrustedPeer, T : WotTrust<TP>> KVStore<WotKV<TP>> for WotStore<TP, T> {
  //type Cache = Self;

  // TODO arc make no sense
  fn add_val(& mut self,  kv : WotKV<TP>, stconf : (bool, Option<CachePolicy>)){
    // TODO !!!! calculate and update trust  + TODO wotstore primitive to change a peer trust!!!
    match kv {
      WotKV::Peer(ref skv) => {
          println!("valcheck : {:?}", skv.check_val(skv, &PeerInfoRel));
        // do not store invalid peer Peer info
        if skv.key_check() && skv.check_val(skv, &PeerInfoRel) {
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
              let newrel = xpval.merge(&skv).unwrap_or(skv.clone());
              self.promstore.add_val(newrel, stconf);
            },

          };
        },
        WotKV::Sign(ref skv) => {
          match self.peerstore.get_val(skv.from()) {
            None => (),
            Some(from) => {

              if skv.check_val(&from, &PeerTrustRel) {
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
                  match self.promstore.get_val(skv.about()) {
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
        WotKV::TrustQuery(_) => {
          // nothing to do (get only struct).
          // add is done by adding sign or when receiving sign subsequent to find or discover
          // operation
        },

    };
  }

  // TODO better implementation
  fn has_val(& self, k : &<WotKV<TP> as KeyVal>::Key) -> bool {
    self.get_val(k).is_some()
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
      &WotK::P2S (_) => {
        // TODO  realy nothing???
        //self.promstore.remove_val(sk)
        // Done with peer removal
        ()
      },
      &WotK::Sign(ref sk) => {
        match self.signstore.get_val(sk){
          None => (),
          Some (skval) => 
            match self.promstore.get_val(skval.about()) {
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
            let (upd, pro) = newtrust.update(from_trust, trust.trust(), from_trust, <u8 as Bounded>::max_value(), &self.rules);
            if upd {
            self.wotstore.add_val(newtrust, (true,None));
            };
            if pro {
            // TODO trust has change, do update all inpacted sign from this user
            };
            });

      },
      &WotK::TrustQuery(_) => {
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
fn addpeer_test<TP : TrustedPeer, T : WotTrust<TP>, F : Fn(String) -> ArcKV<TP>> (wotstore : &mut WotStore<TP, T>, name : String, init : &F)-> ArcKV<TP> {
  let stconf = (true,None);
  let pe = init(name);
  wotstore.add_val(WotKV::Peer(pe.clone()) ,stconf);

  let qme = wotstore.get_val(&WotK::Peer(pe.get_key())).unwrap();

  assert_eq!(qme.get_key(), WotK::Peer(pe.get_key()));
  pe
}

#[cfg(test)]
fn addpeer_trust<TP : TrustedPeer, T : WotTrust<TP>> (by : &ArcKV<TP>, about : &ArcKV<TP>, wotstore : &mut WotStore<TP, T>, trust : u8, testtrust : u8, tag : Option<usize>) {
  let tagv = tag.unwrap_or(0);

  let stconf = (true,None);
  let psignme : PeerSign<TP> = PeerSign::new(&(*by), &(*about), trust, tagv).unwrap();
  wotstore.add_val(WotKV::Sign(psignme.clone()) ,stconf);

  let qmesign = wotstore.get_val(&WotK::Sign(psignme.get_key())).unwrap();
  assert_eq!(qmesign.get_key(), WotK::Sign(psignme.get_key()));

  let metrus = wotstore.get_peer_trust(&about.get_key());
  // enable when update of trust ok
  assert_eq!(testtrust, metrus);
}

#[test]
#[cfg(feature="openssl-impl")]
fn test_wot_rsa() {
  let initRSAPeer = |name| ArcKV::new(RSAPeer::new (name, None, utils::sa4(Ipv4Addr::new(127,0,0,1), 8080)));
  test_wot_gen(&initRSAPeer)
}
#[cfg(test)]
fn test_wot_gen<T : TrustedPeer, F : Fn(String) -> ArcKV<T>>(init : &F) 
 {
  // initiate with simple cache map
  let mut wotpeers = SimpleCache::new(None);
  let mut wotrels  = SimpleCache::new(None);
  let mut wotsigns = SimpleCache::new(None);
  let mut wotrusts : SimpleCache<ClassicWotTrust<T>,HashMap<<ClassicWotTrust<T> as KeyVal>::Key, ClassicWotTrust<T>>> = SimpleCache::new(None);
  let mut trustRul : TrustRules = vec![1,1,2,2,2];
  let me = init("myname".to_string());

//  let me = ArcKV::new(RSAPeer::new ("myname".to_string(), None, utils::sa4(Ipv4Addr::new(127,0,0,1), 8080) ));
  let mut wotstore = WotStore::new(
      wotpeers,
      wotrels,
      wotsigns,
      wotrusts,
      me.clone(),
      100,
      trustRul,
      );


  // check master trust
  let metrus = wotstore.get_peer_trust(&me.get_key());
  assert_eq!(0, metrus);

  // add other peer and their trust
  let peer1 = addpeer_test(&mut wotstore,"peer1".to_string(), init);
  let peer2 = addpeer_test(&mut wotstore,"peer2".to_string(), init);
  let peer3 = addpeer_test(&mut wotstore,"peer3".to_string(), init);
  let peer4 = addpeer_test(&mut wotstore,"peer4".to_string(), init);
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
  let stconf = (true,None);
  let revokeme : PeerSign<T> = PeerSign::new(&(me), &(me), <u8 as Bounded>::max_value(), 1).unwrap();
  wotstore.add_val(WotKV::Sign(revokeme.clone()) ,stconf);

  // test trust

  // test all other trust TODO does it impact existing (it should but in long term distributed
  // not here) or TODO redesign trust storage to keep trace of its calculus also true for any
  // update




}


