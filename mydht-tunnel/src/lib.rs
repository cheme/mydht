extern crate tunnel;
extern crate mydht;
extern crate serde;
extern crate mydht_slab;

use mydht_slab::slab::{
  Slab,
};
use std::marker::PhantomData;
use std::fmt::Debug;
use std::hash::Hash;
use std::time::Instant;
use std::time::Duration;
use serde::{Serialize};
use serde::de::{DeserializeOwned};
#[cfg(test)]
extern crate mydht_basetest;
#[cfg(test)]
#[macro_use]
extern crate serde_derive;
use mydht::keyval::{
  GettableAttachments,
  SettableAttachments,
  Attachment,
};
use mydht::msgencif::{
  MsgEnc,
};
use mydht::dhtif::{
  Result,
  PeerMgmtMeths,
  DHTRules,
  KeyVal,
};
use mydht::dhtimpl::{
  Cache,
  SlabCache,
  SimpleCache,
  SimpleCacheQuery,
};
use mydht::kvstoreif::{
  KVStore,
};
use std::collections::HashMap;
use std::io::{
  Write,
  Read,
};
use tunnel::Peer as TPeer;
use mydht::peer::Peer;
use mydht::utils::{
  Ref,
  OneResult,
};
use mydht::transportif::{
  Transport,
};
use mydht::{
  MyDHTConf,
  PeerStatusListener,
  PeerStatusCommand,
  HashMapQuery,
  Route,
};
use mydht::api::{
  Api,
  ApiResult,
  ApiResultSend,
  ApiQueriable,
  ApiQueryId,
  ApiRepliable,
};
use mydht::{
  GlobalCommand,
  GlobalReply,
  LocalReply,
  FWConf,
  MCReply,
  MCCommand,
  ShadowAuthType,
  ProtoMessage,
  ProtoMessageSend,
  PeerCacheEntry,
  RWSlabEntry,
  ChallengeEntry,
};
use mydht::utils::{
  OptInto,
  OptFrom,
};
use mydht::service::{
  Service,
 // MioChannel,
 // SpawnChannel,
  MpscChannel,
  MpscChannelRef,
  //NoRecv,
  LocalRcChannel,
  SpawnerYield,
 // LocalRc,
 // MpscSender,
  NoSend,

  Blocker,
  RestartOrError,
  Coroutine,
 // RestartSameThread,
 // ThreadBlock,
  ThreadPark,
  ThreadParkRef,

  NoSpawn,
  NoChannel,
 
};
 

#[cfg(test)]
mod test;

/// lightened mydht conf for tunnel
/// At this time we put a minimum, but it may include more (especially optionnal service like api),
/// this makes it force Send conf (not SRef) and force Slab implementation (some chan and handle in
/// it) : TODO when stable move all associated types ??
pub trait MyDHTTunnelConf : 'static + Send + Sized {
  type Transport : Transport;
  // constraint for hash type
  type PeerKey : Hash + Serialize + DeserializeOwned + Debug + Eq + Clone + 'static + Send + Sync;
  type Peer : Peer<Key = Self::PeerKey,Address = <Self::Transport as Transport>::Address>;
  type PeerRef : Ref<Self::Peer> + Serialize + DeserializeOwned + Clone 
    // This send trait should be remove if spawner moved to this level
    + Send;
  type PeerMgmtMeths : PeerMgmtMeths<Self::Peer>;
  type DHTRules : DHTRules + Clone;
  type InnerCommand : ApiQueriable + PeerStatusListener<Self::PeerRef>
    // clone as include in both local and global (local constraint)
     + Clone
    // This send trait should be remove if spawner moved to this level
    + Send;
  type InnerReply : ApiRepliable
    // This send trait should be remove if spawner moved to this level
    + Send;
  // TODO change those by custom dest !!!+ OptInto<
//    GlobalReply<Self::Peer,Self::PeerRef,Self::InnerCommand,Self::InnerReply>
 //   > + OptInto<LocalReply<MyDHTTunnelConfType<Self>>>;
  type InnerService : Service<
    // global command use almost useless (run non auth)
    CommandIn = GlobalCommand<Self::PeerRef,Self::InnerCommand>,
    CommandOut = Self::InnerReply,
  >
    // This send trait should be remove if spawner moved to this level
    + Send;

  type ProtoMsg : Into<MCCommand<MyDHTTunnelConfType<Self>>> + SettableAttachments + GettableAttachments + OptFrom<MCCommand<MyDHTTunnelConfType<Self>>>;
  type MsgEnc : MsgEnc<Self::Peer, Self::ProtoMsg> + Clone;

//  type SlabEntry;
//  type Slab : SlabCache<Self::SlabEntry>;
  type PeerCache : Cache<Self::PeerKey,PeerCacheEntry<Self::PeerRef>>;
  type ChallengeCache : Cache<Vec<u8>,ChallengeEntry<MyDHTTunnelConfType<Self>>>;
  type Route : Route<MyDHTTunnelConfType<Self>>;
  type PeerKVStore : KVStore<Self::Peer>
    // This send trait should be remove if spawner moved to this level
    + Send;

  fn init_ref_peer(&mut self) -> Result<Self::PeerRef>;
  fn init_transport(&mut self) -> Result<Self::Transport>;
  fn init_peermgmt_proto(&mut self) -> Result<Self::PeerMgmtMeths>;
  fn init_dhtrules_proto(&mut self) -> Result<Self::DHTRules>;
  fn init_enc_proto(&mut self) -> Result<Self::MsgEnc>;
  fn init_route(&mut self) -> Result<Self::Route>;
  fn init_main_loop_peer_cache(&mut self) -> Result<Self::PeerCache>;
  fn init_main_loop_challenge_cache(&mut self) -> Result<Self::ChallengeCache>;
  fn init_peer_kvstore(&mut self) -> Result<Box<Fn() -> Result<Self::PeerKVStore> + Send>>;
 
}
pub struct MyDHTTunnel<MC : MyDHTTunnelConf> {
  me : MC::PeerRef,
}

/// type to send message
pub enum TunnelMessaging<MC : MyDHTTunnelConf> {
  /// send query with tunnelwriter
  SendQuery(MC::ProtoMsg),
  /// proxy from reader
  ProxyFromReader,
  /// reply based on borrow reader content
  ReplyFromReader(MC::ProtoMsg),
}
impl<MC : MyDHTTunnelConf> OptFrom<MCCommand<MyDHTTunnelConfType<MC>>> for TunnelMessaging<MC> {
  fn can_from(m : &MCCommand<MyDHTTunnelConfType<MC>>) -> bool {
    match *m {
      MCCommand::Local(..) | MCCommand::Global(..) => true,
      MCCommand::PeerStore(..) | MCCommand::TryConnect(..) => false,
    }
  }
  fn opt_from(m : MCCommand<MyDHTTunnelConfType<MC>>) -> Option<Self> {
    match m {
      MCCommand::Local(..) => unimplemented!(), 
      MCCommand::Global(..) =>  unimplemented!(),
      MCCommand::PeerStore(..) | MCCommand::TryConnect(..) => None,
    }
  }
}
impl<MC : MyDHTTunnelConf> Into<MCCommand<MyDHTTunnelConfType<MC>>> for TunnelMessaging<MC> {
  fn into(self) -> MCCommand<MyDHTTunnelConfType<MC>> {
    match self {
      TunnelMessaging::SendQuery(inner_pmes) => unimplemented!(),
      TunnelMessaging::ReplyFromReader(inner_pmes) => unimplemented!(),
      ProxyFromReader => unimplemented!(),
    }
  }
}


/// a special msg enc dec using tunnel primitive (tunnelw tunnelr in protomessage)
pub struct TunnelWriterReader<MC : MyDHTTunnelConf> {
  pub inner_enc : MC::MsgEnc,
}

impl<MC : MyDHTTunnelConf> Clone for TunnelWriterReader<MC> {
  fn clone(&self) -> Self {
    TunnelWriterReader{
      inner_enc : self.inner_enc.clone(),
    }
  }
}

impl<MC : MyDHTTunnelConf> MsgEnc<MC::Peer,TunnelMessaging<MC>> for TunnelWriterReader<MC> {
  fn encode_into<'a,W : Write> (&self, w : &mut W, m : &ProtoMessageSend<'a,MC::Peer>) -> Result<()>
    where <MC::Peer as Peer>::Address : 'a {
      self.inner_enc.encode_into(w,m)
  }
  fn decode_from<R : Read>(&self, r : &mut R) -> Result<ProtoMessage<MC::Peer>> {
      self.inner_enc.decode_from(r)
  }

  fn encode_msg_into<'a,W : Write> (&self, w : &mut W, mesg : &TunnelMessaging<MC>) -> Result<()> {
    unimplemented!()
  }

  fn attach_into<W : Write> (&self, w : &mut W, att : &Attachment) -> Result<()> {
    unimplemented!()
  }
  fn decode_msg_from<R : Read>(&self, r : &mut R) -> Result<TunnelMessaging<MC>> {
    unimplemented!()
  }
  fn attach_from<R : Read>(&self, r : &mut R, max_size : usize) -> Result<Attachment> {
    unimplemented!()
  }
}

pub struct MyDHTTunnelConfType<MC : MyDHTTunnelConf>(pub MC);
pub enum LocalTunnelCommand<MC : MyDHTTunnelConf> {
  Inner(MC::InnerCommand),
}
pub enum LocalTunnelReply<MC : MyDHTTunnelConf> {
  Inner(MC::InnerReply),
}
pub enum GlobalTunnelCommand<MC : MyDHTTunnelConf> {
  Inner(MC::InnerCommand),
}
pub enum GlobalTunnelReply<MC : MyDHTTunnelConf> {
  Inner(MC::InnerReply),
}
pub struct LocalTunnelService<MC : MyDHTTunnelConf> {
  pub inner : MC::InnerService,
}

impl<MC : MyDHTTunnelConf> Service for LocalTunnelService<MC> {
  type CommandIn = LocalTunnelCommand<MC>;
  type CommandOut = LocalReply<MyDHTTunnelConfType<MC>>;
  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, async_yield : &mut S) -> Result<Self::CommandOut> {
    unimplemented!()
  }
}

pub struct GlobalTunnelService<MC : MyDHTTunnelConf> {
  pub inner : MC::InnerService,
}

impl<MC : MyDHTTunnelConf> Service for GlobalTunnelService<MC> {
  type CommandIn = GlobalCommand<MC::PeerRef,GlobalTunnelCommand<MC>>;
  type CommandOut = GlobalReply<MC::Peer,MC::PeerRef,GlobalTunnelCommand<MC>,GlobalTunnelReply<MC>>;
  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, async_yield : &mut S) -> Result<Self::CommandOut> {
    unimplemented!()
  }
}


/// implement MyDHTConf for MDHTTunnelConf
/// TODO lot should be parameterized in TunnelConf but for now we hardcode as much as possible
/// similarily no SRef or Ref at the time (message containing reader and other
impl<MC : MyDHTTunnelConf> MyDHTConf for MyDHTTunnelConfType<MC> where 
{
  /// test without auth first TODO some
  /// testing with, but the tunnel itself do a kind of auth (with possible replay attack)
  const AUTH_MODE : ShadowAuthType = ShadowAuthType::NoAuth;
  // no established com yet so one seems ok
  const SEND_NB_ITER : usize = 1;

  type Peer = MC::Peer;
  type PeerRef = MC::PeerRef;
  type MainloopSpawn = ThreadPark;
  type MainLoopChannelIn = MpscChannel;
  type MainLoopChannelOut = MpscChannel;

  type Transport = <MC as MyDHTTunnelConf>::Transport;
  type ProtoMsg = TunnelMessaging<MC>;
  type MsgEnc = TunnelWriterReader<MC>;
  type PeerMgmtMeths = MC::PeerMgmtMeths;
  type DHTRules = MC::DHTRules;
  type Route = MC::Route;
  //type Slab = MC::Slab;
  type Slab = Slab<RWSlabEntry<Self>>;
  type PeerCache = MC::PeerCache;
  type ChallengeCache = MC::ChallengeCache;

  type PeerMgmtChannelIn = MpscChannel;
  type ReadChannelIn = MpscChannel;
  type ReadSpawn = ThreadPark;
  type WriteDest = NoSend;
 
  type WriteChannelIn = MpscChannel;
  type WriteSpawn = ThreadPark;

  type LocalServiceCommand = LocalTunnelCommand<MC>;
  type LocalServiceReply = LocalTunnelReply<MC>;
  type LocalService = LocalTunnelService<MC>;
  const LOCAL_SERVICE_NB_ITER : usize = 1;
  type LocalServiceSpawn = Blocker;
  type LocalServiceChannelIn = NoChannel;
 
  type GlobalServiceCommand = GlobalTunnelCommand<MC>;
  type GlobalServiceReply = GlobalTunnelReply<MC>;
  type GlobalService = GlobalTunnelService<MC>;
  type GlobalServiceSpawn = ThreadPark;
  type GlobalServiceChannelIn = MpscChannel;
  type ApiReturn = OneResult<(Vec<ApiResult<Self>>,usize,usize)>;
  type ApiService = Api<Self,HashMap<ApiQueryId,(OneResult<(Vec<ApiResult<Self>>,usize,usize)>,Instant)>>;

  type ApiServiceSpawn = ThreadPark;
  type ApiServiceChannelIn = MpscChannel;


  type PeerStoreQueryCache = SimpleCacheQuery<Self::Peer,Self::PeerRef,Self::PeerRef,HashMapQuery<Self::Peer,Self::PeerRef,Self::PeerRef>>;
  type PeerStoreServiceSpawn = ThreadPark;
  type PeerStoreServiceChannelIn = MpscChannel;
  type PeerKVStore = MC::PeerKVStore;

  type SynchListenerSpawn = ThreadPark;

  // currently test with async only
  const NB_SYNCH_CONNECT : usize = 0;
  type SynchConnectChannelIn = NoChannel;
  type SynchConnectSpawn = NoSpawn;

  fn init_peer_kvstore(&mut self) -> Result<Box<Fn() -> Result<Self::PeerKVStore> + Send>> {
    self.0.init_peer_kvstore()
  }
  fn do_peer_query_forward_with_discover(&self) -> bool {
    false
  }
  fn init_peer_kvstore_query_cache(&mut self) -> Result<Box<Fn() -> Result<Self::PeerStoreQueryCache> + Send>> {
    Ok(Box::new(
      ||{
        // non random id
        Ok(SimpleCacheQuery::new(false))
      }
    ))
  }
  fn init_peerstore_channel_in(&mut self) -> Result<Self::PeerStoreServiceChannelIn> {
    Ok(MpscChannel)
  }
  fn init_peerstore_spawner(&mut self) -> Result<Self::PeerStoreServiceSpawn> {
    Ok(ThreadPark)
  }
//impl<P : Peer, V : KeyVal, RP : Ref<P>> SimpleCacheQuery<P,V,RP,HashMapQuery<P,V,RP>> {
// QueryCache<Self::Peer,Self::PeerRef,Self::PeerRef>;
  fn init_ref_peer(&mut self) -> Result<Self::PeerRef> {
    self.0.init_ref_peer()
  }
  fn get_main_spawner(&mut self) -> Result<Self::MainloopSpawn> {
    //Ok(Blocker)
    Ok(ThreadPark)
//      Ok(ThreadParkRef)
  }
  fn init_main_loop_slab_cache(&mut self) -> Result<Self::Slab> {
    Ok(Slab::new())
  }
  fn init_main_loop_peer_cache(&mut self) -> Result<Self::PeerCache> {
    self.0.init_main_loop_peer_cache()
  }
  fn init_main_loop_challenge_cache(&mut self) -> Result<Self::ChallengeCache> {
    self.0.init_main_loop_challenge_cache()
  }
  fn init_main_loop_channel_in(&mut self) -> Result<Self::MainLoopChannelIn> {
    Ok(MpscChannel)
  }
  fn init_main_loop_channel_out(&mut self) -> Result<Self::MainLoopChannelOut> {
    Ok(MpscChannel)
  }
  fn init_read_spawner(&mut self) -> Result<Self::ReadSpawn> {
    Ok(ThreadPark)
  }
  fn init_write_spawner(&mut self) -> Result<Self::WriteSpawn> {
    Ok(ThreadPark)
  }
  fn init_global_spawner(&mut self) -> Result<Self::GlobalServiceSpawn> {
    Ok(ThreadPark)
  }
  fn init_local_spawner(&mut self) -> Result<Self::LocalServiceSpawn> {
    Ok(Blocker)
  }
  fn init_local_channel_in(&mut self) -> Result<Self::LocalServiceChannelIn> {
    Ok(NoChannel)
  }

  fn init_write_spawner_out() -> Result<Self::WriteDest> {
    Ok(NoSend)
  }
  fn init_read_channel_in(&mut self) -> Result<Self::ReadChannelIn> {
    Ok(MpscChannel)
  }
  fn init_write_channel_in(&mut self) -> Result<Self::WriteChannelIn> {
    Ok(MpscChannel)
  }
  fn init_peermgmt_channel_in(&mut self) -> Result<Self::PeerMgmtChannelIn> {
    Ok(MpscChannel)
  }


  fn init_enc_proto(&mut self) -> Result<Self::MsgEnc> {
    Ok(TunnelWriterReader{
      inner_enc : self.0.init_enc_proto()?
    })
  }

  fn init_transport(&mut self) -> Result<Self::Transport> {
    self.0.init_transport()
  }
  fn init_peermgmt_proto(&mut self) -> Result<Self::PeerMgmtMeths> {
    self.0.init_peermgmt_proto()
  }
  fn init_dhtrules_proto(&mut self) -> Result<Self::DHTRules> {
    self.0.init_dhtrules_proto()
  }

  fn init_global_service(&mut self) -> Result<Self::GlobalService> {
    unimplemented!()
  }
  fn init_local_service(me : Self::PeerRef, owith : Option<Self::PeerRef>) -> Result<Self::LocalService> {
    unimplemented!()
  }


  fn init_global_channel_in(&mut self) -> Result<Self::GlobalServiceChannelIn> {
    Ok(MpscChannel)
  }

  fn init_route(&mut self) -> Result<Self::Route> {
    self.0.init_route()
  }

  fn init_api_service(&mut self) -> Result<Self::ApiService> {
    Ok(Api(HashMap::new(),Duration::from_millis(3000),0,PhantomData))
  }

  fn init_api_channel_in(&mut self) -> Result<Self::ApiServiceChannelIn> {
    Ok(MpscChannel)
  }
  fn init_api_spawner(&mut self) -> Result<Self::ApiServiceSpawn> {
    Ok(ThreadPark)
    //Ok(Blocker)
  }
  fn init_synch_listener_spawn(&mut self) -> Result<Self::SynchListenerSpawn> {
    Ok(ThreadPark)
  }
   fn init_synch_connect_spawn(&mut self) -> Result<Self::SynchConnectSpawn> {
    Ok(NoSpawn)
  }
  fn init_synch_connect_channel_in(&mut self) -> Result<Self::SynchConnectChannelIn> {
    Ok(NoChannel)
  }
}

impl<MC : MyDHTTunnelConf> GettableAttachments for TunnelMessaging<MC> {
  fn get_attachments(&self) -> Vec<&Attachment> {
    match *self {
      TunnelMessaging::SendQuery(ref inner_pmes)
      | TunnelMessaging::ReplyFromReader(ref inner_pmes) 
        => inner_pmes.get_attachments(),
      TunnelMessaging::ProxyFromReader => Vec::new(),
    }
  }
}

impl<MC : MyDHTTunnelConf> SettableAttachments for TunnelMessaging<MC> {
  fn attachment_expected_sizes(&self) -> Vec<usize> {
    match *self {
      TunnelMessaging::SendQuery(ref inner_pmes)
      |  TunnelMessaging::ReplyFromReader(ref inner_pmes) 
        => inner_pmes.attachment_expected_sizes(),
      TunnelMessaging::ProxyFromReader => Vec::new(),
    }
  }
  fn set_attachments(& mut self, at : &[Attachment]) -> bool {
    match *self {
      TunnelMessaging::SendQuery(ref mut  inner_pmes)
      |  TunnelMessaging::ReplyFromReader(ref mut inner_pmes) 
        => inner_pmes.set_attachments(at),
      TunnelMessaging::ProxyFromReader => at.len() == 0,
    }
  }
}




impl<MC : MyDHTTunnelConf> ApiQueriable for LocalTunnelCommand<MC> {
  fn is_api_reply(&self) -> bool {
    match *self {
      LocalTunnelCommand::Inner(ref inn_c) => inn_c.is_api_reply(),
    }
  }
  fn set_api_reply(&mut self, i : ApiQueryId) {
    match *self {
      LocalTunnelCommand::Inner(ref mut inn_c) => inn_c.set_api_reply(i),
    }
  }
  fn get_api_reply(&self) -> Option<ApiQueryId> {
    match *self {
      LocalTunnelCommand::Inner(ref inn_c) => inn_c.get_api_reply(),
    }
  }
}


impl<MC : MyDHTTunnelConf> Clone for LocalTunnelCommand<MC> {
  fn clone(&self) -> Self {
    match *self {
      LocalTunnelCommand::Inner(ref inn_c) => LocalTunnelCommand::Inner(inn_c.clone()),
    }
  }
}

impl<MC : MyDHTTunnelConf> ApiRepliable for LocalTunnelReply<MC> {
  fn get_api_reply(&self) -> Option<ApiQueryId> {
    match *self {
      LocalTunnelReply::Inner(ref inn_c) => inn_c.get_api_reply(),
    }
  }
}

impl<MC : MyDHTTunnelConf> ApiQueriable for GlobalTunnelCommand<MC> {
  fn is_api_reply(&self) -> bool {
    match *self {
      GlobalTunnelCommand::Inner(ref inn_c) => inn_c.is_api_reply(),
    }
  }
  fn set_api_reply(&mut self, i : ApiQueryId) {
    match *self {
      GlobalTunnelCommand::Inner(ref mut inn_c) => inn_c.set_api_reply(i),
    }
  }
  fn get_api_reply(&self) -> Option<ApiQueryId> {
    match *self {
      GlobalTunnelCommand::Inner(ref inn_c) => inn_c.get_api_reply(),
    }
  }
}


impl<MC : MyDHTTunnelConf> Clone for GlobalTunnelCommand<MC> {
  fn clone(&self) -> Self {
    match *self {
      GlobalTunnelCommand::Inner(ref inn_c) => GlobalTunnelCommand::Inner(inn_c.clone()),
    }
  }
}


impl<MC : MyDHTTunnelConf> PeerStatusListener<MC::PeerRef> for GlobalTunnelCommand<MC> {
  const DO_LISTEN : bool = true;
  fn build_command(command : PeerStatusCommand<MC::PeerRef>) -> Self {
    unimplemented!()
  }
}


impl<MC : MyDHTTunnelConf> ApiRepliable for GlobalTunnelReply<MC> {
  fn get_api_reply(&self) -> Option<ApiQueryId> {
    match *self {
      GlobalTunnelReply::Inner(ref inn_c) => inn_c.get_api_reply(),
    }
  }
}


