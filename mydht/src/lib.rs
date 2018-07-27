#![feature(custom_derive)]
#![feature(fn_traits)]
#![feature(associated_type_defaults)]

#[macro_use] extern crate log;
#[macro_use] extern crate mydht_base;
#[macro_use] extern crate serde_derive;
extern crate readwrite_comp;
extern crate serde;
extern crate serde_json;
extern crate bincode;
extern crate byteorder;
extern crate bit_vec;
extern crate rand;
extern crate service_pre;
extern crate futures;
extern crate futures_cpupool;
#[cfg(test)]
extern crate mio;
#[cfg(test)]
extern crate mydht_basetest;

#[macro_export]
macro_rules! sref_self_mc{($ty:ident) => (

  impl<MC : MyDHTConf> SRef for $ty<MC> {
    type Send = $ty<MC>;
    #[inline]
    fn get_sendable(self) -> Self::Send {
      self
    }
  }

  impl<MC : MyDHTConf> SToRef<$ty<MC>> for $ty<MC> {
    #[inline]
    fn to_ref(self) -> $ty<MC> {
      self
    }
  }

)}



/// Local service will simply proxy to Global service
#[macro_export]
macro_rules! localproxyglobal(() => (
  type GlobalServiceCommand = Self::LocalServiceCommand;
  type GlobalServiceReply  = Self::LocalServiceReply;
  type LocalServiceProto = ();
  type LocalService = DefLocalService<Self>;
  const LOCAL_SERVICE_NB_ITER : usize = 1;
  type LocalServiceSpawn = Blocker;
  type LocalServiceChannelIn = NoChannel;
  #[inline]
  fn init_local_spawner(&mut self) -> Result<Self::LocalServiceSpawn> {
    Ok(Blocker)
  }
  #[inline]
  fn init_local_channel_in(&mut self) -> Result<Self::LocalServiceChannelIn> {
    Ok(NoChannel)
  }
  #[inline]
  fn init_local_service(_proto : Self::LocalServiceProto, me : Self::PeerRef, with : Option<Self::PeerRef>, _read_tok : usize) -> Result<Self::LocalService> {
    Ok(DefLocalService{
      from : me,
      with : with,
    })
  }
  #[inline]
  fn init_local_service_proto(&mut self) -> Result<Self::LocalServiceProto> {
    Ok(())
  }
));

#[macro_export]
macro_rules! nolocal(() => (

  const LOCAL_SERVICE_NB_ITER : usize = 1;// = 1;
  type LocalServiceCommand = NoCommandReply;
  type LocalServiceReply = NoCommandReply;
  type LocalServiceProto = ();
  type LocalService = NoService<Self::LocalServiceCommand,LocalReply<Self>>;
  type LocalServiceSpawn = NoSpawn;
  type LocalServiceChannelIn = NoChannel;

  #[inline]
  fn init_local_spawner(&mut self) -> Result<Self::LocalServiceSpawn> {
    Ok(NoSpawn)
  }
  #[inline]
  fn init_local_channel_in(&mut self) -> Result<Self::LocalServiceChannelIn> {
    Ok(NoChannel)
  }
  #[inline]
  fn init_local_service(_proto : Self::LocalServiceProto, _me : Self::PeerRef, _with : Option<Self::PeerRef>, _read_tok : usize) -> Result<Self::LocalService> {
    Ok(NoService::new())
  }
  #[inline]
  fn init_local_service_proto(&mut self) -> Result<Self::LocalServiceProto> {
    Ok(())
  }

));


pub mod kvcache{
pub use mydht_base::kvcache::*;
}
pub mod keyval{
pub use mydht_base::keyval::*;
}
pub mod simplecache{
pub use mydht_base::simplecache::*;
}
pub mod kvstore{
pub use mydht_base::kvstore::*;
}
pub mod mydhtresult{
pub use mydht_base::mydhtresult::*;
}
pub mod service{
pub use service_pre::*;
pub use service_pre::transport::*;
}



#[cfg(test)]
mod node{
  extern crate mydht_basetest;
  pub use self::mydht_basetest::node::*;
}



pub mod peer;
mod procs;
mod query;
mod transport;
mod msgenc;
pub mod utils;
pub mod rules;
//pub mod wot;
#[cfg(test)]
mod test;
pub use procs::deflocal::{
  DefLocalService,
  GlobalCommand,
  GlobalReply,
  LocalReply,
};
pub use procs::{
  MyDHT,
  ApiCommand,
  Api,
  ApiQueryId,
  ApiResult,
  MyDHTConf,
  PeerCacheRouteBase,
  PeerCacheEntry,
  AddressCacheEntry,
  PeerStatusCommand,
  PeerStatusListener,
  FWConf,
  MCReply,
  MCCommand,
  ShadowAuthType,
  ReadReply,
  MainLoopCommand,
  MainLoopSubCommand,
  RWSlabEntry,
  ChallengeEntry,
  Route,
  ReaderBorrowable,
  RegReaderBorrow,
  storeprop,
  noservice,
  ClientMode,
};
pub use mydht_base::route2::IndexableWriteCache;
pub use procs::api;
// reexport
pub use peer::{PeerPriority,PeerState};
pub use query::{QueryConf,QueryPriority,QueryMode,LastSentConf};
pub use kvstore::{CachePolicy};
pub use mydht_base::kvstore::{StoragePriority};
pub use mydht_base::keyval::{Attachment,SettableAttachment};
// TODO move msgenc to mod dhtimpl
pub use msgenc::json::{Json};
pub use msgenc::ProtoMessage;
pub use msgenc::send_variant::ProtoMessage as ProtoMessageSend;
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
  pub use mydht_base::kvcache::{
    NoCache,
    Cache,
    SlabCache,
  };
  pub use query::simplecache::{
    SimpleCacheQuery,
    HashMapQuery,
  };
  pub use simplecache::{SimpleCache};
  //pub use route::inefficientmap::{Inefficientmap};

  //pub use route::btkad::{BTKad};
//  pub use wot::truststore::{WotKV,WotK,WotStore};
//  pub use wot::classictrust::{TrustRules,ClassicWotTrust};

  pub use rules::simplerules::{DhtRules,SimpleRules,DHTRULES_DEFAULT};

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
}
pub mod kvstoreif{
  pub use mydht_base::kvcache::{KVCache};
  pub use mydht_base::kvstore::{KVStore, KVStoreRel};
}
pub mod transportif{
  pub use mydht_base::transport::SerSocketAddr;
  pub use transport::{
    Transport,
    WriteTransportStream,
    ReadTransportStream,
    Address,
    Registerable,
    TriggerReady,
    Poll,
    Events,
    Event,
  };
  #[cfg(feature="mio-transport")]
  pub use service_pre::eventloop::mio::{
    MioEvents,
  };
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



