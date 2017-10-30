extern crate tunnel;
extern crate mydht;
extern crate serde;
extern crate rand;
extern crate mydht_slab;
extern crate readwrite_comp;

use std::io::Cursor;
use std::mem::replace;
use std::borrow::Borrow;
use rand::OsRng;
use tunnel::info::error::{
  MultipleErrorInfo,
  MultipleErrorMode,
  MulErrorProvider,
  NoErrorProvider,
};
use tunnel::info::multi::{
  MultipleReplyMode,
  MultipleReplyInfo,
  ReplyInfoProvider,
};
use readwrite_comp::{
  ExtWrite,
  ExtRead,
  MultiRExt,
  CompExtWInner,
  CompExtRInner,
};
use tunnel::{
  TunnelNoRep,
  TunnelReaderNoRep,
  TunnelReadProv,
  TunnelNoRepReadProv,
};
use tunnel::full::{
  Full,
  FullReadProv,
  FullW,
  FullR,
  DestFull,
  GenTunnelTraits,
  TunnelCachedWriterExt,
  ErrorWriter,
  ReplyWriter,
};

use tunnel::nope::{
  Nope,
  TunnelNope,
};

use tunnel::{
  SymProvider,
};
use std::collections::VecDeque;
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
  Proto,
  OneResult,
};
use mydht::transportif::{
  Transport,
  Address,
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
  MainLoopCommand,
  MainLoopSubCommand,
  ShadowAuthType,
  ProtoMessage,
  ProtoMessageSend,
  PeerCacheEntry,
  AddressCacheEntry,
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

mod tunnelconf;
use tunnelconf::{
  TunnelTraits,
  ReplyTraits,
  CachedInfoManager,
  Rp,
};

#[derive(Clone,Debug)]
pub struct TunPeer<P,RP>(RP,PhantomData<P>);

impl<P,RP> TunPeer<P,RP> {
  #[inline]
  fn new(rp : RP) -> Self {
    TunPeer(rp,PhantomData)
  }
}
impl<P : Peer, RP : Ref<P> + Clone + Debug> TPeer for TunPeer<P,RP> {
  type Address = P::Address;
  type ShadRead = P::ShadowRAuth;
  type ShadWrite = P::ShadowWAuth;

  fn get_address(&self) -> &Self::Address {
    self.0.borrow().get_address()
  }
  fn new_shadw(&self) -> Self::ShadWrite {
    self.0.borrow().get_shadower_w_auth()
  }
  fn new_shadr(&self) -> Self::ShadRead {
    self.0.borrow().get_shadower_r_auth()
  }

}


/// lightened mydht conf for tunnel
/// At this time we put a minimum, but it may include more (especially optionnal service like api),
/// this makes it force Send conf (not SRef) and force Slab implementation (some chan and handle in
/// it) : TODO when stable move all associated types ??
pub trait MyDHTTunnelConf : 'static + Send + Sized {
  const REPLY_ONCE_BUF_SIZE : usize = 256;
  const TUN_CACHE_KEY_LENGTH : usize = 128;
  const INIT_ROUTE_LENGTH : usize;
  const INIT_ROUTE_BIAS : usize;
  type TransportAddress : Address + Hash;
  type Transport : Transport<Address = Self::TransportAddress>;
  // constraint for hash type
  type PeerKey : Hash + Serialize + DeserializeOwned + Debug + Eq + Clone + 'static + Send + Sync;
  type Peer : Peer<Key = Self::PeerKey,Address = <Self::Transport as Transport>::Address>;
  type PeerRef : Ref<Self::Peer> + Serialize + DeserializeOwned + Clone + Debug
    // This send trait should be remove if spawner moved to this level
    + Send;
  type PeerMgmtMeths : PeerMgmtMeths<Self::Peer>;
  type DHTRules : DHTRules + Clone;
  type InnerCommand : ApiQueriable + PeerStatusListener<Self::PeerRef>
    // clone as include in both local and global (local constraint)
     + Clone
    // This send trait should be remove if spawner moved to this level
    + Send;
  type InnerReply : ApiQueriable + OptInto<GlobalTunnelReply<Self>>
    // This send trait should be remove if spawner moved to this level
    + Send;
  // TODO change those by custom dest !!!+ OptInto<
//    GlobalReply<Self::Peer,Self::PeerRef,Self::InnerCommand,Self::InnerReply>
 //   > + OptInto<LocalReply<MyDHTTunnelConfType<Self>>>;
  type InnerServiceProto : Clone + Send;
  type InnerService : Service<
    // global command use almost useless (run non auth)
    CommandIn = GlobalCommand<Self::PeerRef,Self::InnerCommand>,
    CommandOut = Self::InnerReply,
  >
    // This send trait should be remove if spawner moved to this level
    + Send;

  /// TODO unclear we do not need that much convs TODO when stable test without
  type ProtoMsg : Into<MCCommand<MyDHTTunnelConfType<Self>>> + SettableAttachments + GettableAttachments
    + OptFrom<Self::InnerCommand>
    + Into<Self::InnerCommand>
    ;
  type MsgEnc : MsgEnc<Self::Peer, Self::ProtoMsg>;

//  type SlabEntry;
//  type Slab : SlabCache<Self::SlabEntry>;
  type PeerCache : Cache<Self::PeerKey,PeerCacheEntry<Self::PeerRef>>;
  type AddressCache : Cache<Self::TransportAddress,AddressCacheEntry>;
  type ChallengeCache : Cache<Vec<u8>,ChallengeEntry<MyDHTTunnelConfType<Self>>>;
  type Route : Route<MyDHTTunnelConfType<Self>>;
  type PeerKVStore : KVStore<Self::Peer>
    // This send trait should be remove if spawner moved to this level
    + Send;


  type LimiterW : ExtWrite + Clone + Send;
  type LimiterR : ExtRead + Clone + Send;

  type SSW : ExtWrite + Send;
  type SSR : ExtRead + Send;
  type SP : SymProvider<Self::SSW,Self::SSR> + Clone + Send;


  type CacheSSW : Cache<Vec<u8>,SSWCache<Self>>
    + Send;
  type CacheSSR : Cache<Vec<u8>,MultiRExt<Self::SSR>>
    + Send;

  type CacheErW : Cache<Vec<u8>,(ErrorWriter,Self::TransportAddress)>
    + Send;
  type CacheErR : Cache<Vec<u8>,Vec<MultipleErrorInfo>>
    + Send;

  //type GenTunnelTraits : GenTunnelTraits + Send;

  fn init_ref_peer(&mut self) -> Result<Self::PeerRef>;
  fn init_transport(&mut self) -> Result<Self::Transport>;
  fn init_inner_service(Self::InnerServiceProto, Self::PeerRef) -> Result<Self::InnerService>;
  fn init_inner_service_proto(&mut self) -> Result<Self::InnerServiceProto>;
  fn init_peermgmt_proto(&mut self) -> Result<Self::PeerMgmtMeths>;
  fn init_dhtrules_proto(&mut self) -> Result<Self::DHTRules>;
  fn init_enc_proto(&mut self) -> Result<Self::MsgEnc>;
  fn init_route(&mut self) -> Result<Self::Route>;
  fn init_main_loop_peer_cache(&mut self) -> Result<Self::PeerCache>;
  fn init_main_loop_address_cache(&mut self) -> Result<Self::AddressCache>;
  fn init_main_loop_challenge_cache(&mut self) -> Result<Self::ChallengeCache>;
  fn init_peer_kvstore(&mut self) -> Result<Box<Fn() -> Result<Self::PeerKVStore> + Send>>;
  fn init_cache_ssw(&mut self) -> Result<Self::CacheSSW>;
  fn init_cache_ssr(&mut self) -> Result<Self::CacheSSR>;
  fn init_cache_err(&mut self) -> Result<Self::CacheErR>;
  fn init_cache_erw(&mut self) -> Result<Self::CacheErW>;
  fn init_shadow_provider(&mut self) -> Result<Self::SP>;
  fn init_limiter_w(&mut self) -> Result<Self::LimiterW>;
  fn init_limiter_r(&mut self) -> Result<Self::LimiterR>;

}

//type SSWCache<MC : MyDHTTunnelConf> = (TunnelCachedWriterExt<MC::SSW,MC::LimiterW>,<TunPeer<MC::Peer,MC::PeerRef> as TPeer>::Address);
type SSWCache<MC : MyDHTTunnelConf> = (TunnelCachedWriterExt<MC::SSW,MC::LimiterW>,MC::TransportAddress);
pub struct MyDHTTunnel<MC : MyDHTTunnelConf> {
  me : MC::PeerRef,
}
pub enum ReadMsgState<MC : MyDHTTunnelConf> {
  /// no reply : simply use in service and drop service reply
  NoReply,
  /// use in service then directly forward to write with read addition
  /// Will get read token from local
  ReplyDirectInit(ReplyWriterTConf<MC>,MC::TransportAddress),

  /// use in service then send reply directly with the writer
  ReplyDirectNoInit(ReplyWriterTConf<MC>,MC::TransportAddress),
  /// reply requires state read from global
  /// Will get read token from local
  ReplyToGlobalWithRead(DestFullRTConf<MC>),
}

/// type to send message
pub enum TunnelMessaging<MC : MyDHTTunnelConf> {
  /// send query with tunnelwriter
  TunnelSendOnce(Option<FullWTConf<MC>>,MC::ProtoMsg),
  /// proxy from reader
  /// Will get read token from local
  ProxyFromReader(FullRTConf<MC>),
  /// read from reader to global (need global state)
  /// Will get read token from local
  ReadToGlobal(FullRTConf<MC>),
  /// reply based on borrow reader content
  ReplyFromReader(MC::ProtoMsg,ReadMsgState<MC>),
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
      MCCommand::Global(gl_tunnel_command) => match gl_tunnel_command {
        GlobalTunnelCommand::TunnelSendOnce(tunn_w, inner) => inner.opt_into().map(|inner_proto|
          TunnelMessaging::TunnelSendOnce(Some(tunn_w), inner_proto)),
        GlobalTunnelCommand::Inner(..)
          | GlobalTunnelCommand::NewOnline(..)
          | GlobalTunnelCommand::Offline(..)
          | GlobalTunnelCommand::DestReplyFromGlobal(..)
          => None, 
      },
      MCCommand::PeerStore(..) | MCCommand::TryConnect(..) => None,
    }
  }
}

impl<MC : MyDHTTunnelConf> Into<MCCommand<MyDHTTunnelConfType<MC>>> for TunnelMessaging<MC> {
  fn into(self) -> MCCommand<MyDHTTunnelConfType<MC>> {
    match self {
      TunnelMessaging::TunnelSendOnce(tunn_w,inner_pmes) => unreachable!(),
      TunnelMessaging::ReplyFromReader(inner_pmes,st) => {
        let inner_command : MC::InnerCommand = inner_pmes.into(); 
        MCCommand::Local(LocalTunnelCommand::LocalFromRead(inner_command,st))
        /*match st {
          ReadMsgState::NoReply => unimplemented!(),
          ReadMsgState::ReplyDirectInit(rwc,tadd) => MCCommand::Local(
            LocalTunnelCommand::ReplyDirectInit(inner_command,destfullread) 
          ),
          ReadMsgState::ReplyDirectNoInit(rwc,tadd) => unimplemented!(),
          ReadMsgState::ReplyToGlobalWithRead(destfullread) => MCCommand::Local(
            LocalTunnelCommand::DestReplyFromGlobal(inner_command,destfullread) 
          ),
        }*/
      },
      TunnelMessaging::ProxyFromReader(tun_r) => unimplemented!(),
      TunnelMessaging::ReadToGlobal(tun_r) => unimplemented!(),
    }
  }
}


/// a special msg enc dec using tunnel primitive (tunnelw tunnelr in protomessage)
pub struct TunnelWriterReader<MC : MyDHTTunnelConf> {
  pub inner_enc : MC::MsgEnc,
  pub current_writer : Option<FullWTConf<MC>>,
  pub current_reader : Option<DestFullRTConf<MC>>,
  pub nb_attach_rem : usize,
  pub do_finalize_read : bool,
  pub t_readprov : FullReadProv<TunnelTraits<MC>>,
}

impl<MC : MyDHTTunnelConf> Proto for TunnelWriterReader<MC> {
  fn get_new(&self) -> Self {
    TunnelWriterReader{
      inner_enc : self.inner_enc.get_new(),
      current_writer : None,
      current_reader : None,
      nb_attach_rem : 0,
      do_finalize_read : false,
      t_readprov : self.t_readprov.new_tunnel_read_prov(),
    }
  }
}

impl<MC : MyDHTTunnelConf> MsgEnc<MC::Peer,TunnelMessaging<MC>> for TunnelWriterReader<MC> {
  fn encode_into<'a,W : Write> (&mut self, w : &mut W, m : &ProtoMessageSend<'a,MC::Peer>) -> Result<()>
    where <MC::Peer as Peer>::Address : 'a {
    self.inner_enc.encode_into(w,m)
  }
  fn decode_from<R : Read>(&mut self, r : &mut R) -> Result<ProtoMessage<MC::Peer>> {
    self.inner_enc.decode_from(r)
  }

  fn encode_msg_into<'a,W : Write> (&mut self, w : &mut W, mesg : &mut TunnelMessaging<MC>) -> Result<()> {
    match *mesg {
      TunnelMessaging::TunnelSendOnce(None, _) => unreachable!(),
      TunnelMessaging::TunnelSendOnce(ref mut o_tunn_we, ref mut proto_m) => {
        let nb_att = proto_m.get_nb_attachments();
        {
          let tunn_we = o_tunn_we.as_mut().unwrap();
          tunn_we.write_header(w)?;
          let mut tunn_w = CompExtWInner(w, tunn_we);
          self.inner_enc.encode_msg_into(&mut tunn_w,proto_m)?;
        }
        self.current_writer = None;
        if nb_att > 0 {
          self.nb_attach_rem = nb_att;
          self.current_writer = replace(o_tunn_we, None);
        } else {
          let tunn_we = o_tunn_we.as_mut().unwrap();
          tunn_we.flush_into(w)?;
          tunn_we.write_end(w)?;
        }
      },
      TunnelMessaging::ProxyFromReader(..) => unimplemented!(),
      TunnelMessaging::ReplyFromReader(..) => unimplemented!(),
      TunnelMessaging::ReadToGlobal(..) => unimplemented!(),
    }
    Ok(())
  }

  fn attach_into<W : Write> (&mut self, w : &mut W, att : &Attachment) -> Result<()> {
    // attachment require storing tunn_w in MsgEnc and flushing/writing end somehow
    // (probably by storing nb attach when encode and write end at last one 
    // or at end of encode if none)
    let tunn_we = self.current_writer.as_mut().unwrap();
    {
      let mut tunn_w = CompExtWInner(w, tunn_we);
      self.inner_enc.attach_into(&mut tunn_w, att)?;
    }
    self.nb_attach_rem -= 1;
    if self.nb_attach_rem == 0 {
      tunn_we.flush_into(w)?;
      tunn_we.write_end(w)?;
    }
    Ok(())
  }
  fn decode_msg_from<R : Read>(&mut self, r : &mut R) -> Result<TunnelMessaging<MC>> {

    let mut tunn_re = self.t_readprov.new_reader();
    self.do_finalize_read = false;
    tunn_re.read_header(r)?;
    if tunn_re.is_dest().unwrap() {
      if self.t_readprov.can_dest_reader(&tunn_re) {
      match self.t_readprov.new_dest_reader(tunn_re, r)? {
        Some(mut dest_reader) => {
          dest_reader.read_header(r)?;
          let proto_m = {
            let mut tunn_r = CompExtRInner(r, &mut dest_reader);
            self.inner_enc.decode_msg_from(&mut tunn_r)?
          };
          self.nb_attach_rem = proto_m.attachment_expected_sizes().len();
          let (do_reply,need_init,owr) = self.t_readprov.new_reply_writer(&mut dest_reader, r)?;
          if !do_reply || !need_init {
            if self.nb_attach_rem > 0 {
              self.do_finalize_read = true;
            } else {
              dest_reader.read_end(r)?;
            }
          }
          let message : TunnelMessaging<MC> = if !do_reply {
            if self.nb_attach_rem > 0 {
              self.current_reader = Some(dest_reader);
            }
            TunnelMessaging::ReplyFromReader(proto_m, ReadMsgState::NoReply)
          } else if let Some((reply_w,dest)) = owr {
            if self.nb_attach_rem > 0 {
              self.current_reader = Some(dest_reader);
            }

            if need_init {
              // send to write with read (run inner service in local)
              TunnelMessaging::ReplyFromReader(proto_m, ReadMsgState::ReplyDirectInit(reply_w,dest))
            } else {
              // send to write without read (run inner service in local)
              TunnelMessaging::ReplyFromReader(proto_m, ReadMsgState::ReplyDirectNoInit(reply_w,dest))
            }
          } else {
            if self.nb_attach_rem > 0 {
              // at the time Tunnel impl do not use this mode so we simply panic now (currently if
              // we need to go on global we do not need init because we would use cache)
              panic!("No attachment support in this configuration : requires a api redesign (dest_reader in message but require for attachments)"); 
            }
            // send to global service as it requires cache + if owr.1 is true send read else
            // no send read
            TunnelMessaging::ReplyFromReader(proto_m, ReadMsgState::ReplyToGlobalWithRead(dest_reader))
          };
          Ok(message)
        },
        None => {
          // bug, the call to can dest reader should exclude this
          unreachable!();
                
        },
      }
      } else {
        self.nb_attach_rem = 0;
        // Send reader to global as for proxy : requires cache TODO do not remember reading
        // attachement in global
        Ok(TunnelMessaging::ReadToGlobal(tunn_re))
      }

    } else if tunn_re.is_err().unwrap() {
      unimplemented!()
    } else {
      // proxy
      Ok(TunnelMessaging::ProxyFromReader(tunn_re))
    }
  }
  fn attach_from<R : Read>(&mut self, r : &mut R, max_size : usize) -> Result<Attachment> {
    if let Some(dest_reader) = self.current_reader.as_mut() {
      let att = {
        let mut tunn_r = CompExtRInner(r, dest_reader);
        self.inner_enc.attach_from(&mut tunn_r, max_size)?
      };

      if self.do_finalize_read {
        dest_reader.read_end(r)?;
      }
      Ok(att)
    } else {
      // true only in context of read service (read msg then immediatly read attachment)
      unreachable!()
    }
  }
}

pub struct MyDHTTunnelConfType<MC : MyDHTTunnelConf>{
  conf: MC, 
  me : MC::PeerRef,

  reply_mode : MultipleReplyMode,
  error_mode : MultipleErrorMode,
  route_len : usize,
  route_bias : usize,
  tunnel : Option<Full<TunnelTraits<MC>>>,
  tunnel_r : FullReadProv<TunnelTraits<MC>>,
  inner_service_proto : MC::InnerServiceProto,
 //reply_mode : MultipleReplyMode::RouteReply,
 //error_mode : MultipleErrorMode::NoHandling,
}

impl<MC : MyDHTTunnelConf> MyDHTTunnelConfType<MC> {
  pub fn new(mut conf : MC, reply_mode : MultipleReplyMode, error_mode : MultipleErrorMode,route_len : Option<usize>,route_bias : Option<usize>) -> Result<Self> {
    let me = conf.init_ref_peer()?;

    let route_len = route_len.unwrap_or(MC::INIT_ROUTE_LENGTH);
    let route_bias = route_bias.unwrap_or(MC::INIT_ROUTE_BIAS);
    let tme = TunPeer::new(me.clone());
    let tunnel_reply : Full<ReplyTraits<_>> = Full {
        me : tme.clone(),
        reply_mode : MultipleReplyMode::RouteReply,
        error_mode : MultipleErrorMode::NoHandling,
        cache : Nope,
        route_prov : Nope,
        reply_prov : ReplyInfoProvider {
          mode : MultipleReplyMode::RouteReply,
          symprov : conf.init_shadow_provider()?,
          _p : PhantomData,
        },
        sym_prov : conf.init_shadow_provider()?,
        error_prov : NoErrorProvider,
        rng : OsRng::new()?,
        limiter_proto_w : conf.init_limiter_w()?,
        limiter_proto_r : conf.init_limiter_r()?,
        tunrep : TunnelNope::new(),
        reply_once_buf_size : MC::REPLY_ONCE_BUF_SIZE,
        _p : PhantomData,
    };


    let tunnel = Full {
        me : tme.clone(),

        reply_mode : reply_mode.clone(),
        error_mode : error_mode.clone(),
        cache : CachedInfoManager {
          cache_ssw : conf.init_cache_ssw()?,
          cache_ssr : conf.init_cache_ssr()?,
          cache_erw : conf.init_cache_erw()?,
          cache_err : conf.init_cache_err()?,
          keylength : MC::TUN_CACHE_KEY_LENGTH,
          rng : OsRng::new()?,
          _ph : PhantomData,
        },
        //  pub sym_prov : TT::SP,
        route_prov : Rp::new(tme.clone(),route_len,route_bias)?,
        reply_prov : ReplyInfoProvider {
          mode : reply_mode.clone(),
          symprov : conf.init_shadow_provider()?,
          _p : PhantomData,
        },
        sym_prov : conf.init_shadow_provider()?,

        error_prov : MulErrorProvider::new(error_mode.clone()).unwrap(),
        rng : OsRng::new()?,
        limiter_proto_w : conf.init_limiter_w()?,
        limiter_proto_r : conf.init_limiter_r()?,
        tunrep : tunnel_reply,
        reply_once_buf_size : MC::REPLY_ONCE_BUF_SIZE,
        _p : PhantomData,
      };
    let tunnel_r = tunnel.new_tunnel_read_prov();
    let i_service_proto = conf.init_inner_service_proto()?;
    Ok(MyDHTTunnelConfType {
      conf,
      me,
      reply_mode,
      error_mode,
      route_len,
      route_bias,
      tunnel : Some(tunnel),
      tunnel_r,
      inner_service_proto : i_service_proto,
    })
  }
}

pub enum LocalTunnelCommand<MC : MyDHTTunnelConf> {
  /// TODO unused ?? remove!!!
  Inner(MC::InnerCommand),
  /// message has been read, need to forward to global with read (insert token)
  LocalFromRead(MC::InnerCommand,ReadMsgState<MC>),
}
pub enum LocalTunnelReply<MC : MyDHTTunnelConf> {
  Inner(MC::InnerReply),
// TODO  DestFromReader(MC::InnerCommand),
}

pub type MovableRead<MC : MyDHTTunnelConf> = (<MC::Transport as Transport>::ReadStream, <MC::Peer as Peer>::ShadowRMsg);
pub enum GlobalTunnelCommand<MC : MyDHTTunnelConf> {
  Inner(MC::InnerCommand),
  NewOnline(MC::PeerRef),
  Offline(MC::PeerRef),
  TunnelSendOnce(FullWTConf<MC>,MC::InnerCommand),
  /// message has been read, reply need to be instantiated from global (withread)
  DestReplyFromGlobal(MC::InnerCommand,DestFullRTConf<MC>,usize,Option<MovableRead<MC>>),
}

// TODO box it !!!!!!!!! (better yet full of heap struct (vec))
type FullWTConf<MC : MyDHTTunnelConf> = FullW<MultipleReplyInfo<MC::TransportAddress>,MultipleErrorInfo,TunPeer<MC::Peer,MC::PeerRef>, MC::LimiterW,
  FullW<MultipleReplyInfo<MC::TransportAddress>, MultipleErrorInfo,TunPeer<MC::Peer,MC::PeerRef>, MC::LimiterW,Nope>>; 


type FullRTConf<MC : MyDHTTunnelConf> = FullR<MultipleReplyInfo<MC::TransportAddress>, MultipleErrorInfo, TunPeer<MC::Peer,MC::PeerRef>, MC::LimiterR>;
type DestFullRTConf<MC : MyDHTTunnelConf> = DestFull<FullRTConf<MC>,MC::SSR, MC::LimiterR>;

type ReplyWriterTConf<MC : MyDHTTunnelConf> = ReplyWriter<MC::LimiterW,MC::SSW>; 

pub enum GlobalTunnelReply<MC : MyDHTTunnelConf> {
  /// inner service return a result for api
  Api(MC::InnerReply),
  SendCommandTo(MC::PeerRef,MC::InnerCommand),
}
pub struct LocalTunnelService<MC : MyDHTTunnelConf> {
  pub me : MC::PeerRef,
  pub with : Option<MC::PeerRef>,
  pub read_token : usize,
  pub inner : MC::InnerService,
}

impl<MC : MyDHTTunnelConf> Service for LocalTunnelService<MC> {
  type CommandIn = LocalTunnelCommand<MC>;
  type CommandOut = LocalReply<MyDHTTunnelConfType<MC>>;
  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, async_yield : &mut S) -> Result<Self::CommandOut> {
    match req {
      LocalTunnelCommand::Inner(cmd) => {
        let rep = self.inner.call(GlobalCommand::Local(cmd),async_yield)?;
    unimplemented!()
      },
      LocalTunnelCommand::LocalFromRead(cmd,read_state) => {
        let rep = self.inner.call(GlobalCommand::Distant(self.with.clone(),cmd),async_yield)?;
    unimplemented!()
      },
    }
  }
}

pub struct GlobalTunnelService<MC : MyDHTTunnelConf> {
  pub inner : MC::InnerService,
  pub to_send : VecDeque<(MC::PeerRef,MC::InnerCommand)>,
  pub tunnel : Full<TunnelTraits<MC>>,
  pub address_key : HashMap<MC::TransportAddress,MC::PeerKey>,
}

impl<MC : MyDHTTunnelConf> Service for GlobalTunnelService<MC> {
  type CommandIn = GlobalCommand<MC::PeerRef,GlobalTunnelCommand<MC>>;
  type CommandOut = GlobalReply<MC::Peer,MC::PeerRef,GlobalTunnelCommand<MC>,GlobalTunnelReply<MC>>;
  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, async_yield : &mut S) -> Result<Self::CommandOut> {
    let (is_local,owith,req) = match req {
      GlobalCommand::Local(r) => (true,None,r), 
      GlobalCommand::Distant(ow,r) => (false,ow,r), 
    };
    Ok(match req {
      GlobalTunnelCommand::DestReplyFromGlobal(inner_command,dest_full_read,read_old_token,o_read) => {
        unimplemented!()
      },
      GlobalTunnelCommand::TunnelSendOnce(tun_w,i_com) => {
        unreachable!()
      },
      GlobalTunnelCommand::NewOnline(pr) => {
        self.address_key.insert(pr.borrow().get_address().clone(),pr.borrow().get_key());
        self.tunnel.route_prov.add_online(TunPeer::new(pr));
        if self.to_send.len() > 0 && self.tunnel.route_prov.enough_peer() {
          // send all cached
          while let Some((dest, inner_command)) = self.to_send.pop_front() {
            let route_writer = self.tunnel.new_writer(&TunPeer::new(dest));
            panic!("TODO sedn protr mess");
          }
        }

        GlobalReply::NoRep
      },
      GlobalTunnelCommand::Offline(pr) => {
        self.address_key.remove(pr.borrow().get_address());
        self.tunnel.route_prov.rem_online(TunPeer::new(pr));
        GlobalReply::NoRep
      },
      GlobalTunnelCommand::Inner(inner_command) => {
        // no service spawn for now
        let rep = if is_local {
          self.inner.call(GlobalCommand::Local(inner_command),async_yield)?
        } else {
          self.inner.call(GlobalCommand::Distant(owith,inner_command),async_yield)?
        };
        match rep.opt_into() {
          Some(GlobalTunnelReply::Api(inner_reply)) => {
            if inner_reply.is_api_reply() {
              GlobalReply::Api(GlobalTunnelReply::Api(inner_reply))
            } else {
              GlobalReply::NoRep
            }
          },
          Some(GlobalTunnelReply::SendCommandTo(dest, inner_command)) => {
            if self.tunnel.route_prov.enough_peer() {
              let (tunn_we,dest_add) = self.tunnel.new_writer(&TunPeer::new(dest));
              let dest_k = self.address_key.get(&dest_add).map(|k|k.clone());
              let command : GlobalTunnelCommand<MC> = GlobalTunnelCommand::TunnelSendOnce(tunn_we,inner_command);
              GlobalReply::Forward(None,Some(vec![(dest_k,Some(dest_add))]), FWConf {
                nb_for : 0,
                discover : true,
              }, command)
            } else {
              self.to_send.push_back((dest,inner_command));
              GlobalReply::MainLoop(MainLoopSubCommand::PoolSize(self.tunnel.route_prov.route_len()))
            }
          },

          None => GlobalReply::NoRep,
        }
/*        match rep.get_api_reply() {
          Some(aid),r
        }

  fn get_api_reply(&self) -> Option<ApiQueryId>;*/
      },
/*      GlobalCommand(_,GlobalTunnelCommand::NewRoute(route)) => {
        self.tunnel.route_prov.cache_rand_peer(route);
        let mut need_query = 0;
        loop {
          let (block_missing_rand_peer, query_rand_peer) = self.tunnel.route_prov.need_for_next_route();
          if !block_missing_rand_peer {
            if let Some((dest, inner_command)) = self.to_send.pop_front() {
              need_query += query_rand_peer;
              let route_writer = self.tunnel.new_writer(&dest);
              panic!("TODO add writer in the protomessage for write");
            }
          } else { 
            need_query += query_rand_peer;
            break;
          }
        }
        if need_query > 0 {
          return GlobalReply::MainLoop(MainLoopCommand::RandomConnectedSubset(self.tunnel.route_prov.route_len(),need_query, feed_route_provider));
        }
      },*/
    })
  }
}


/*fn feed_route_provider<MC : MyDHTTunnelConf>(peers : Vec<MC::PeerRef>) -> MainLoopCommand<MyDHTTunnelConfType<MC>> {
  MainLoopCommand::ProxyGlobal(GlobalTunnelCommand::NewRoute(peers))
}*/

/// implement MyDHTConf for MDHTTunnelConf
/// TODO lot should be parameterized in TunnelConf but for now we hardcode as much as possible
/// similarily no SRef or Ref at the time (message containing reader and other
impl<MC : MyDHTTunnelConf> MyDHTConf for MyDHTTunnelConfType<MC> {
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
  type AddressCache = MC::AddressCache;
  type ChallengeCache = MC::ChallengeCache;

  type PeerMgmtChannelIn = MpscChannel;
  type ReadChannelIn = MpscChannel;
  type ReadSpawn = ThreadPark;
  type WriteDest = NoSend;
 
  type WriteChannelIn = MpscChannel;
  type WriteSpawn = ThreadPark;

  type LocalServiceCommand = LocalTunnelCommand<MC>;
  type LocalServiceReply = LocalTunnelReply<MC>;
  type LocalServiceProto = MC::InnerServiceProto;
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
    self.conf.init_peer_kvstore()
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
    Ok(self.me.clone())
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
    self.conf.init_main_loop_peer_cache()
  }
  fn init_main_loop_address_cache(&mut self) -> Result<Self::AddressCache> {
    self.conf.init_main_loop_address_cache()
  }
  fn init_main_loop_challenge_cache(&mut self) -> Result<Self::ChallengeCache> {
    self.conf.init_main_loop_challenge_cache()
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
      inner_enc : self.conf.init_enc_proto()?,
      current_writer : None,
      current_reader : None,
      nb_attach_rem : 0,
      do_finalize_read : false,
      t_readprov : self.tunnel_r.new_tunnel_read_prov(),
    })
  }

  fn init_transport(&mut self) -> Result<Self::Transport> {
    self.conf.init_transport()
  }
  fn init_peermgmt_proto(&mut self) -> Result<Self::PeerMgmtMeths> {
    self.conf.init_peermgmt_proto()
  }
  fn init_dhtrules_proto(&mut self) -> Result<Self::DHTRules> {
    self.conf.init_dhtrules_proto()
  }

  fn init_global_service(&mut self) -> Result<Self::GlobalService> {
/*    let me = TunPeer::new(self.me.clone());
    let rl = self.route_len.unwrap_or(MC::INIT_ROUTE_LENGTH);
    let rb = self.route_bias.unwrap_or(MC::INIT_ROUTE_BIAS);

    let tunnel_reply : Full<ReplyTraits<_>> = Full {
        me : me.clone(),
        reply_mode : MultipleReplyMode::RouteReply,
        error_mode : MultipleErrorMode::NoHandling,
        cache : Nope,
        route_prov : Nope,
        reply_prov : ReplyInfoProvider {
          mode : MultipleReplyMode::RouteReply,
          symprov : self.conf.init_shadow_provider()?,
          _p : PhantomData,
        },
        sym_prov : self.conf.init_shadow_provider()?,
        error_prov : NoErrorProvider,
        rng : OsRng::new()?,
        limiter_proto_w : self.conf.init_limiter_w()?,
        limiter_proto_r : self.conf.init_limiter_r()?,
        tunrep : TunnelNope::new(),
        reply_once_buf_size : MC::REPLY_ONCE_BUF_SIZE,
        _p : PhantomData,
    };
*/
    // warn support only one instantiation of global service
    let tunnel = replace(&mut self.tunnel,None).unwrap();
    Ok(GlobalTunnelService {
      address_key : HashMap::new(),
      inner : MC::init_inner_service(self.inner_service_proto.clone(), self.me.clone())?,
      to_send : VecDeque::new(),
      tunnel,
    })
  }
  fn init_local_service_proto(&mut self) -> Result<Self::LocalServiceProto> {
    Ok(self.inner_service_proto.clone())
  }

  fn init_local_service(proto : MC::InnerServiceProto, me : Self::PeerRef, with : Option<Self::PeerRef>, read_token : usize) -> Result<Self::LocalService> {
    Ok(LocalTunnelService {
      me : me.clone(),
      with,
      read_token,
      inner : MC::init_inner_service(proto,me)?,
    })
  }


  fn init_global_channel_in(&mut self) -> Result<Self::GlobalServiceChannelIn> {
    Ok(MpscChannel)
  }

  fn init_route(&mut self) -> Result<Self::Route> {
    self.conf.init_route()
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
  fn get_nb_attachments(&self) -> usize {
    match *self {
      TunnelMessaging::TunnelSendOnce(_,ref inner_pmes)
      | TunnelMessaging::ReplyFromReader(ref inner_pmes,_) 
        => inner_pmes.get_nb_attachments(),
      TunnelMessaging::ProxyFromReader(..)
      | TunnelMessaging::ReadToGlobal(..)
        => 0,
    }

  }
  fn get_attachments(&self) -> Vec<&Attachment> {
    match *self {
      TunnelMessaging::TunnelSendOnce(_,ref inner_pmes)
      | TunnelMessaging::ReplyFromReader(ref inner_pmes,_) 
        => inner_pmes.get_attachments(),
      TunnelMessaging::ProxyFromReader(..)
      | TunnelMessaging::ReadToGlobal(..)
        => Vec::new(),
    }
  }
}

impl<MC : MyDHTTunnelConf> SettableAttachments for TunnelMessaging<MC> {
  fn attachment_expected_sizes(&self) -> Vec<usize> {
    match *self {
      TunnelMessaging::TunnelSendOnce(_,ref inner_pmes)
      |  TunnelMessaging::ReplyFromReader(ref inner_pmes,_) 
        => inner_pmes.attachment_expected_sizes(),
      TunnelMessaging::ProxyFromReader(..)
      | TunnelMessaging::ReadToGlobal(..)
        => Vec::new(),
    }
  }
  fn set_attachments(& mut self, at : &[Attachment]) -> bool {
    match *self {
      TunnelMessaging::TunnelSendOnce(_,ref mut  inner_pmes)
      |  TunnelMessaging::ReplyFromReader(ref mut inner_pmes,_) 
        => inner_pmes.set_attachments(at),
      TunnelMessaging::ProxyFromReader(..) 
      | TunnelMessaging::ReadToGlobal(..)
        => at.len() == 0,
    }
  }
}




impl<MC : MyDHTTunnelConf> ApiQueriable for LocalTunnelCommand<MC> {
  fn is_api_reply(&self) -> bool {
    match *self {
      LocalTunnelCommand::LocalFromRead(ref inn_c,_) |
      LocalTunnelCommand::Inner(ref inn_c) => inn_c.is_api_reply(),
    }
  }
  fn set_api_reply(&mut self, i : ApiQueryId) {
    match *self {
      LocalTunnelCommand::LocalFromRead(ref mut inn_c,_) |
      LocalTunnelCommand::Inner(ref mut inn_c) => inn_c.set_api_reply(i),
    }
  }
  fn get_api_reply(&self) -> Option<ApiQueryId> {
    match *self {
      LocalTunnelCommand::LocalFromRead(ref inn_c,_) |
      LocalTunnelCommand::Inner(ref inn_c) => inn_c.get_api_reply(),
    }
  }
}


impl<MC : MyDHTTunnelConf> Clone for LocalTunnelCommand<MC> {
  fn clone(&self) -> Self {
    match *self {
      LocalTunnelCommand::LocalFromRead(ref inn_c,_) |
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
      GlobalTunnelCommand::DestReplyFromGlobal(ref inn_c,_,_,_) |
      GlobalTunnelCommand::Inner(ref inn_c) => inn_c.is_api_reply(),
      GlobalTunnelCommand::NewOnline(..) => false,
      GlobalTunnelCommand::Offline(..) => false,
      GlobalTunnelCommand::TunnelSendOnce(_,ref i_com) => i_com.is_api_reply(),
    }
  }
  fn set_api_reply(&mut self, i : ApiQueryId) {
    match *self {
      GlobalTunnelCommand::DestReplyFromGlobal(ref mut inn_c,_,_,_) |
      GlobalTunnelCommand::Inner(ref mut inn_c) => inn_c.set_api_reply(i),
      GlobalTunnelCommand::NewOnline(..) => (),
      GlobalTunnelCommand::Offline(..) => (),
      GlobalTunnelCommand::TunnelSendOnce(_,ref mut i_com) => i_com.set_api_reply(i),
    }
  }
  fn get_api_reply(&self) -> Option<ApiQueryId> {
    match *self {
      GlobalTunnelCommand::DestReplyFromGlobal(ref inn_c,_,_,_) |
      GlobalTunnelCommand::Inner(ref inn_c) => inn_c.get_api_reply(),
      GlobalTunnelCommand::NewOnline(..) => None,
      GlobalTunnelCommand::Offline(..) => None,
      GlobalTunnelCommand::TunnelSendOnce(_,ref i_com) => i_com.get_api_reply(),
    }
  }
}


impl<MC : MyDHTTunnelConf> Clone for GlobalTunnelCommand<MC> {
  fn clone(&self) -> Self {
    match *self {
      GlobalTunnelCommand::DestReplyFromGlobal(ref inn_c,_,_,_) => unreachable!(),
      GlobalTunnelCommand::Inner(ref inn_c) => GlobalTunnelCommand::Inner(inn_c.clone()),
      GlobalTunnelCommand::NewOnline(ref rp) => GlobalTunnelCommand::NewOnline(rp.clone()),
      GlobalTunnelCommand::Offline(ref rp) => GlobalTunnelCommand::Offline(rp.clone()),
      GlobalTunnelCommand::TunnelSendOnce(ref tw,ref i_com) => unreachable!(),
    }
  }
}


impl<MC : MyDHTTunnelConf> PeerStatusListener<MC::PeerRef> for GlobalTunnelCommand<MC> {
  const DO_LISTEN : bool = true;
  fn build_command(command : PeerStatusCommand<MC::PeerRef>) -> Self {
    match command {
      PeerStatusCommand::PeerOnline(rp,_) => GlobalTunnelCommand::NewOnline(rp),
      PeerStatusCommand::PeerOffline(rp,_) => GlobalTunnelCommand::Offline(rp),
    }
  }
}


impl<MC : MyDHTTunnelConf> ApiRepliable for GlobalTunnelReply<MC> {
  fn get_api_reply(&self) -> Option<ApiQueryId> {
    match *self {
      GlobalTunnelReply::Api(ref inn_c) => inn_c.get_api_reply(),
      GlobalTunnelReply::SendCommandTo(..) => None,
    }
  }
}


