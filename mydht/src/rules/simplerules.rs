use std::time::Duration;
use kvstore::{CachePolicy};
use query::{
  QueryPriority,
  QueryMode,
};
use rules::DHTRules as DHTRulesIf;
//use peer::{PeerPriority};
use procs::ClientMode;



#[derive(Clone)]
/// TODO might not require sync anymore
/// TODO if rules read only make it simply sync??
/// TODO fuse with DhtRules
pub struct SimpleRules ((), DhtRules);

impl SimpleRules {
  pub fn new(dr : DhtRules) -> SimpleRules {
    SimpleRules((), dr)
  }
}

// TODO redesign nbhop nbquery depending on prio put higher prio on big num
#[derive(Debug,Clone,Deserialize,Serialize)]
pub struct DhtRules {
  pub randqueryid : bool, // used to conf querycache : remnant from old code, still use in fs
  pub nbhopfact : u8, // nbhop is prio * fact or nbhopfact if normal // TODO invert prio (higher being 1)
  pub nbqueryfact : f32, // nbquery is 1 + query * fact
  pub lifetime : u64, // seconds of lifetime, static bound
  pub lifetimeinc : u8, // increment of lifetime per priority inc
  pub cleaninterval : Option<u64>, // in seconds if needed
  pub cacheduration : Option<u64>, // cache in seconds
  pub cacheproxied : bool, // do you cache proxied result
  pub storelocal : bool, // is result stored locally
  pub storeproxied : Option<usize>, // store only if less than nbhop // TODO implement other alternative (see comment)
  pub heavyaccept : bool,
  pub clientmode : ClientMode,
  pub tunnellength : u8,
  pub not_found_reply : bool,
  // TODO further params : clientmode, heavy...
}
pub const DHTRULES_DEFAULT : DhtRules = DhtRules {
  randqueryid : false,
  // nbhop = prio * fact
  nbhopfact : 1,
  // nbquery is 1 + query * fact
  nbqueryfact : 1.0, 
  //query lifetime second
  lifetime : 15,
  // increment of lifetime per priority inc
  lifetimeinc : 2,
  cleaninterval : None, // in seconds if needed
  cacheduration : None, // cache in seconds
  cacheproxied : false, // do you cache proxied result
  storelocal : true, // is result stored locally
  storeproxied : None, // store only if less than nbhop
  heavyaccept : false,
  clientmode : ClientMode::ThreadedOne,
  // TODO client mode param + testing for local tcp and mult tcp in max 2 thread and in pool 2
  // thread
  tunnellength : 3,
  not_found_reply : true,
};


impl DHTRulesIf for SimpleRules {

  #[inline]
  fn tunnel_length(&self, _ : QueryPriority) -> u8 {
    self.1.tunnellength
  }
  #[inline]
  fn nbhop_dec (&self) -> u8 {
    1
  }

  fn notfoundreply (&self, _mode : &QueryMode) -> bool {
    self.1.not_found_reply
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
    Duration::from_secs(self.1.lifetime + (prio * self.1.lifetimeinc) as u64  )
  }

  fn nbquery (&self, prio : QueryPriority) -> u8 {
    1 + (self.1.nbqueryfact * prio as f32) as u8
  }

  fn asynch_clean(&self) -> Option<Duration> {
    self.1.cleaninterval.map(|s|Duration::from_secs(s))
  }
  
  fn do_store (&self, islocal : bool, _ : QueryPriority) -> (bool,Option<CachePolicy>) {
    let cacheduration = self.1.cacheduration.map(|s|CachePolicy(Duration::from_secs(s)));
    if islocal && self.1.storelocal { 
      (true,cacheduration) 
    } else { 
      (false,cacheduration) 
    }
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


