use kvstore::{StoragePriority};
use time::Duration;
use kvstore::{CachePolicy};
use query::{QueryPriority,QueryID};
use rules::DHTRules as DHTRulesIf;
//use peer::{PeerPriority};
use std::sync::Mutex;
use rand::{thread_rng,Rng};
use num::traits::ToPrimitive;
use time;
use procs::ClientMode;

pub struct SimpleRules (Mutex<usize>, DhtRules);

impl SimpleRules {
  pub fn new(dr : DhtRules) -> SimpleRules {
    SimpleRules(Mutex::new(0), dr)
  }
}
// TODO redesign nbhop nbquery depending on prio put higher prio on big num
#[derive(Debug,Clone,RustcDecodable,RustcEncodable)]
pub struct DhtRules {
  pub randqueryid : bool, // used to conf querycache : remnant from old code, still use in fs
  pub nbhopfact : u8, // nbhop is prio * fact or nbhopfact if normal // TODO invert prio (higher being 1)
  pub nbqueryfact : f32, // nbquery is 1 + query * fact
  pub lifetime : i64, // seconds of lifetime, static bound
  pub lifetimeinc : u8, // increment of lifetime per priority inc
  pub cleaninterval : Option<i64>, // in seconds if needed
  pub cacheduration : Option<i64>, // cache in seconds
  pub cacheproxied : bool, // do you cache proxied result
  pub storelocal : bool, // is result stored locally
  pub storeproxied : Option<usize>, // store only if less than nbhop // TODO implement other alternative (see comment)
  pub heavyaccept : bool,
  pub clientmode : ClientMode,
  // TODO further params : clientmode, heavy...
}

impl DHTRulesIf for SimpleRules {


  #[inline]
  fn nbhop_dec (&self) -> u8 {
    1
  }

  // here both a static counter and a rand one just for show // TODO switch QueryID to BigInt
  /*fn newid (&self) -> QueryID {
    if self.1.randqueryid {
      let mut rng = thread_rng();
      // (eg database connection)
      //rng.gen_range(0,65555)
      rng.next_u64().to_usize().unwrap()
    } else {
      let mut i = self.0.lock().unwrap();
      *i += 1;
      *i
    }
  }*/

  fn nbhop (&self, prio : QueryPriority) -> u8 {
    self.1.nbhopfact * prio
  }

  fn lifetime (&self, prio : QueryPriority) -> Duration {
    Duration::seconds(self.1.lifetime + (prio * self.1.lifetimeinc).to_i64().unwrap())
  }

  fn nbquery (&self, prio : QueryPriority) -> u8 {
    1 + (self.1.nbqueryfact * prio.to_f32().unwrap()).to_u8().unwrap()
  }

  fn asynch_clean(&self) -> Option<Duration> {
    self.1.cleaninterval.map(|s|Duration::seconds(s))
  }
  
  fn do_store (&self, islocal : bool, _ : QueryPriority, sprio : StoragePriority, hopnb : Option<usize>) -> (bool,Option<CachePolicy>) {
    let cacheduration = self.1.cacheduration.map(|s|Duration::seconds(s));
    let res = match sprio {
      StoragePriority::Local =>
        if islocal && self.1.storelocal { (true,cacheduration) } else { (false,cacheduration) },
      StoragePriority::PropagateL(_) | StoragePriority::PropagateH(_) => 
        if islocal { (true,cacheduration)  } else {
          match hopnb {
            None => (false, None),
            Some(esthop) => { 
              match self.1.storeproxied {
                None => {
                  (false, None)
                },
                Some(hoptresh) => {
              if esthop > hoptresh {
                  (false, None)
              } else {
                  (false, cacheduration)
              }
                },
              }
            },
          }
        },
        StoragePriority::Trensiant => (false,cacheduration),
        StoragePriority::All =>  (true,cacheduration),
        StoragePriority::NoStore => (false,None),
      };
      (res.0, res.1.map(|d| CachePolicy(time::get_time() + d)))
  } // wether you need to store the keyval or not
  #[inline]
  fn is_authenticated(&self) -> bool {
    true 
  }
  #[inline]
  fn client_mode(&self) -> &ClientMode {
    &self.1.clientmode
  }

  #[inline]
  fn server_mode_conf(&self) -> (usize, usize, usize, Option<Duration>) {
    (0, 0, 0, None)
  }
  
  #[inline]
  fn is_accept_heavy(&self) -> bool {
    self.1.heavyaccept
  }

  #[inline]
  fn is_routing_heavy(&self) -> (bool,bool,bool) {
    (false, false, false)
  }


}


