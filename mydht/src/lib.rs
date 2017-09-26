#![feature(custom_derive)]
#![feature(fs_walk)]
#![feature(path_ext)]
#![feature(convert)]
#![feature(semaphore)]
#![feature(deque_extras)]
#![feature(socket_timeout)]
#![feature(slice_bytes)] // in wot
#![feature(fn_traits)]
#![feature(associated_type_defaults)]

#[macro_use] extern crate log;
#[macro_use] extern crate mydht_base;
#[macro_use] extern crate serde_derive;
extern crate serde;
extern crate serde_json;
extern crate time;
extern crate num;
extern crate bincode;
extern crate byteorder;
extern crate bit_vec;
extern crate rand;
extern crate futures;
extern crate futures_cpupool;
#[cfg(feature="mio-impl")]
extern crate coroutine;
#[cfg(test)]
extern crate mydht_basetest;
extern crate mio;


/// Local service will simply proxy to Global service
#[macro_export]
macro_rules! nolocal(() => (

    type GlobalServiceCommand  = GlobalCommand<Self>; // def
    type GlobalServiceReply  = GlobalReply<Self>; // def
    type LocalService = DefLocalService<Self>; // def
    const LOCAL_SERVICE_NB_ITER : usize = 1; // def
    type LocalServiceSpawn = Blocker; // def
    type LocalServiceChannelIn = NoChannel; // def

    #[inline]
    fn init_local_spawner(&mut self) -> Result<Self::LocalServiceSpawn> {
      Ok(Blocker)
    }
    #[inline]
    fn init_local_channel_in(&mut self) -> Result<Self::LocalServiceChannelIn> {
      Ok(NoChannel)
    }
    #[inline]
    fn init_local_service(me : Self::PeerRef, with : Option<Self::PeerRef>) -> Result<Self::LocalService> {
      Ok(DefLocalService{
        from : me,
        with : with,
        })
    }
));




mod kvcache{
pub use mydht_base::kvcache::*;
}
mod keyval{
pub use mydht_base::keyval::*;
}
mod simplecache{
pub use mydht_base::simplecache::*;
}
mod kvstore{
pub use mydht_base::kvstore::*;
}
mod mydhtresult{
pub use mydht_base::mydhtresult::*;
}
mod service{
pub use mydht_base::service::*;
}



#[cfg(test)]
mod node{
  extern crate mydht_basetest;
  pub use self::mydht_basetest::node::*;
}



mod peer;
mod pool;
mod procs;
mod query;
//mod route;
mod transport;
mod msgenc;
pub mod utils;
pub mod rules;
//pub mod wot;
#[cfg(test)]
mod test;

// reexport
pub use peer::{PeerPriority,PeerState};
pub use procs::{DHT, RunningContext, RunningProcesses, ArcRunningContext, RunningTypes};
pub use procs::{store_val, find_val, find_local_val};
pub use query::{QueryConf,QueryPriority,QueryMode,LastSentConf};
pub use kvstore::{CachePolicy};
pub use mydht_base::kvstore::{StoragePriority};
pub use mydht_base::keyval::{Attachment,SettableAttachment};
// TODO move msgenc to mod dhtimpl
pub use msgenc::json::{Json};
//pub use msgenc::bencode::{Bencode};
//pub use msgenc::bincode::{Bincode};
//pub use msgenc::bencode::{Bencode_bt_dht};
//pub use transport::tcp::{Tcp};
//pub use transport::udp::{Udp};
//pub use wot::{TrustedVal,Truster,TrustedPeer};
pub use query::{QueryID,Query};

pub mod dhtimpl {
//  pub use mydht_base::node::{Node};
  pub use peer::NoShadow;
  //#[cfg(feature="openssl-impl")]
  //pub use wot::rsa_openssl::RSAPeer;
  //#[cfg(feature="rust-crypto-impl")]
  //pub use wot::ecdsapeer::ECDSAPeer;

//  pub use wot::trustedpeer::PeerSign;
  pub use mydht_base::kvcache::{NoCache};
  pub use query::simplecache::{SimpleCacheQuery};
  pub use simplecache::{SimpleCache};
  //pub use route::inefficientmap::{Inefficientmap};

  //pub use route::btkad::{BTKad};
//  pub use wot::truststore::{WotKV,WotK,WotStore};
//  pub use wot::classictrust::{TrustRules,ClassicWotTrust};

  pub use rules::simplerules::{DhtRules,SimpleRules};

}
pub mod queryif{
  pub use query::cache::{QueryCache};
  pub use kvstore::{CachePolicy};
}
pub mod dhtif{
  pub use mydhtresult::{Result,Error,ErrorKind};
  pub use mydht_base::keyval::{KeyVal,FileKeyVal,Key};
  pub use rules::DHTRules;
  pub use peer::{Peer,PeerMgmtMeths};
  pub use procs::{ClientMode,ServerMode};
}
pub mod kvstoreif{
  pub use mydht_base::kvcache::{KVCache};
  pub use mydht_base::kvstore::{KVStore, KVStoreRel};
  pub use procs::mesgs::{KVStoreMgmtMessage};
}
pub mod transportif{
  pub use transport::{Transport,TransportStream};
}
pub mod msgencif{
  pub use msgenc::{MsgEnc};
}

#[cfg(test)]
pub mod testexp {
  pub mod common {
    pub use test::*;
  }
}



