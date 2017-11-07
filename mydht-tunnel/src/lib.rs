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
  Tunnel,
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
  ProxyFull,
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
use mydht::peer::{
  Peer,
  NoShadow,
};
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
  ReaderBorrowable,
  RegReaderBorrow,
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
  ReadReply,
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
  ReadYield,
  WriteYield,
  NoYield,
  YieldReturn,
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
  const PROXY_BUF_SIZE : usize = 256;
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
  type InnerReply : ApiQueriable
    // This send trait should be remove if spawner moved to this level
    + Send;
  // TODO change those by custom dest !!!+ OptInto<
//    GlobalReply<Self::Peer,Self::PeerRef,Self::InnerCommand,Self::InnerReply>
 //   > + OptInto<LocalReply<MyDHTTunnelConfType<Self>>>;
  type InnerServiceProto : Clone + Send;
  type InnerService : Service<
    // global command use almost useless (run non auth)
    CommandIn = GlobalCommand<Self::PeerRef,Self::InnerCommand>,
    CommandOut = GlobalTunnelReply<Self>,
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

/// TODO this is overdoing it (plus having the readstream could be confusing : reading for it
/// before mainloop switch state would block the service reading) : borrow could simply reference read service token and mainloop
/// handle it before proxying to write
type ReadBorrowStream<MC : MyDHTTunnelConf> = (<MC::Transport as Transport>::ReadStream,<MC::Peer as Peer>::ShadowRMsg,usize);

pub enum ReadMsgState<MC : MyDHTTunnelConf> {
  /// no reply : simply use in service and drop service reply
  NoReply,
  /// use in service then directly forward to write with read addition
  /// Will get read token from local
  ReplyDirectInit(ReplyWriterTConf<MC>,Option<DestFullRTConf<MC>>,MC::TransportAddress,Option<ReadBorrowStream<MC>>,Option<(MC::LimiterR,MC::LimiterW,usize)>),

  /// use in service then send reply directly with the writer
  ReplyDirectNoInit(ReplyWriterTConf<MC>,MC::TransportAddress),
  /// reply requires state read from global
  /// Will get read token from local
  ReplyToGlobalWithRead(Option<DestFullRTConf<MC>>,Option<ReadBorrowStream<MC>>),
}

/// type to send message
pub enum TunnelMessaging<MC : MyDHTTunnelConf> {
  /// send query with tunnelwriter
  TunnelSendOnce(Option<FullWTConf<MC>>,MC::ProtoMsg),
  /// reply (to send to write service)
  TunnelReplyOnce(MC::ProtoMsg,Option<ReplyWriterInitTConf<MC>>,Option<ReplyWriterTConf<MC>>,DestFullRTConf<MC>,ReadBorrowStream<MC>),
  /// proxy
  TunnelProxyOnce(ProxyRTConf<MC>,ReadBorrowStream<MC>),
  /// proxy from reader
  /// Will get read token from local
  ProxyToGlobal(FullRTConf<MC>),
  /// enough info to proxy directly
  ProxyFromReader(ProxyRTConf<MC>,<MC::Peer as Peer>::Address),
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
        GlobalTunnelCommand::TunnelReplyOnce(inner,rinit,rw,des_r,r_borrow) => inner.opt_into().map(|inner_proto|
            TunnelMessaging::TunnelReplyOnce(inner_proto,Some(rinit),Some(rw),des_r,r_borrow)),
        GlobalTunnelCommand::TunnelSendOnce(tunn_w, inner) => inner.opt_into().map(|inner_proto|
          TunnelMessaging::TunnelSendOnce(Some(tunn_w), inner_proto)),
        GlobalTunnelCommand::TunnelProxyOnce(proxy_w,bs) => Some(TunnelMessaging::TunnelProxyOnce(proxy_w,bs)),
        GlobalTunnelCommand::Inner(..)
          | GlobalTunnelCommand::NewOnline(..)
          | GlobalTunnelCommand::Offline(..)
          | GlobalTunnelCommand::DestReplyFromGlobal(..)
          | GlobalTunnelCommand::TransmitReply(..)
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
      TunnelMessaging::TunnelReplyOnce(inner_pmes,rwinit,rw,destr,rbs) => unreachable!(),
      TunnelMessaging::TunnelProxyOnce(..) => unreachable!(),
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
      TunnelMessaging::ProxyFromReader(proxy_r,dest_add) => {
        MCCommand::Local(LocalTunnelCommand::Proxy(proxy_r,dest_add,None))
      },
      TunnelMessaging::ProxyToGlobal(tun_r) => unimplemented!(),
      TunnelMessaging::ReadToGlobal(tun_r) => unimplemented!(),
    }
  }
}


/// a special msg enc dec using tunnel primitive (tunnelw tunnelr in protomessage)
/// Warning currently inner encoder can not use Yield (always call with NoYield)
pub struct TunnelWriterReader<MC : MyDHTTunnelConf> {
  pub inner_enc : MC::MsgEnc,
  pub current_writer : Option<FullWTConf<MC>>,
  pub current_reply_writer : Option<ReplyWriterTConf<MC>>,
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
      current_reply_writer : None,
      current_reader : None,
      nb_attach_rem : 0,
      do_finalize_read : false,
      t_readprov : self.t_readprov.new_tunnel_read_prov(),
    }
  }
}

impl<MC : MyDHTTunnelConf> MsgEnc<MC::Peer,TunnelMessaging<MC>> for TunnelWriterReader<MC> {

  #[inline]
  fn encode_into<'a,W : Write,EW : ExtWrite,S : SpawnerYield> (&mut self, w : &mut W, shad : &mut EW, y : &mut S, m : &ProtoMessageSend<'a,MC::Peer>) -> Result<()>
    where <MC::Peer as Peer>::Address : 'a {
    self.inner_enc.encode_into(w,shad,y,m)
  }
  #[inline]
  fn decode_from<R : Read,ER : ExtRead,S : SpawnerYield>(&mut self, r : &mut R, shad : &mut ER, y : &mut S) -> Result<ProtoMessage<MC::Peer>> {
    self.inner_enc.decode_from(r,shad,y)
  }

  fn encode_msg_into<'a,W : Write,EW : ExtWrite,S : SpawnerYield> (&mut self, w : &mut W, wshad : &mut EW, y : &mut S, mesg : &mut TunnelMessaging<MC>) -> Result<()> {
    match *mesg {
      TunnelMessaging::TunnelSendOnce(None, _) => unreachable!(),
      TunnelMessaging::TunnelProxyOnce(ref mut proxy, (ref mut rs,ref mut rshad,_)) => {

 //       println!("start a proxying");
        let mut readbuf = vec![0;MC::PROXY_BUF_SIZE];
        let mut y2 = y.opt_clone().unwrap();
        let mut ry = ReadYield(rs,y);
        let mut reader = CompExtRInner(&mut ry,rshad);
        proxy.read_header(&mut reader)?;

        let mut wy = WriteYield(w,&mut y2);
        let mut w = CompExtWInner(&mut wy,wshad);
        proxy.write_header(&mut w)?;
        // unknown length
        let mut ix;
//        let mut total=0;
        while  {
          let l = proxy.read_from(&mut reader, &mut readbuf)?;
 //           println!("reda {} {}",l,total);
         // if l == 0 {
          //  panic!("Read0 at {:?}",total); 
//            println!("do not w 0 lenth l!!!");
         // }
 //         total += l;
          ix = 0;
          while ix < l {
            let nb = proxy.write_into(&mut w, &mut readbuf[..l])?;
            ix += nb;
 //           total  += nb;
          }
          l > 0
        } {}
 //       println!("end a proxying{:?}", total);
        proxy.read_end(&mut reader)?;
        proxy.write_end(&mut w)?;
        proxy.flush_into(&mut w)?;
        w.flush()?;
//        println!("end a proxying");
      },
      TunnelMessaging::TunnelReplyOnce(ref mut proto_m, ref mut rwinit, ref mut orw, ref mut destr, (ref mut rs,ref mut rshad,_)) => {
        println!("Replying!");
        let mut y2 = y.opt_clone().unwrap();
        let mut ry = ReadYield(rs,y);
        let mut reader = CompExtRInner(&mut ry,rshad);

        let mut wy = WriteYield(w,&mut y2);
        let mut w = CompExtWInner(&mut wy,wshad);
        <Full<TunnelTraits<MC>>>::reply_writer_init(replace(rwinit,None).unwrap(), orw.as_mut().unwrap(), destr, &mut reader, &mut w)?;
        // TODO dup code with send
        let nb_att = proto_m.get_nb_attachments();
        {
          let rw = orw.as_mut().unwrap();
          rw.write_header(&mut w)?;
          let mut tunn_w = CompExtWInner(&mut w, rw);
          self.inner_enc.encode_msg_into(&mut tunn_w, &mut NoShadow, &mut NoYield(YieldReturn::Loop), proto_m)?;
        }
        self.current_reply_writer = None;
        if nb_att > 0 {
          self.nb_attach_rem = nb_att;
          self.current_reply_writer = replace(orw, None);
        } else {
          let tunn_we = orw.as_mut().unwrap();
          tunn_we.write_end(&mut w)?;
          tunn_we.flush_into(&mut w)?;
          w.flush()?;
        }
      },
      TunnelMessaging::TunnelSendOnce(ref mut o_tunn_we, ref mut proto_m) => {

        let mut wy = WriteYield(w,y);
        let mut w = CompExtWInner(&mut wy,wshad);

        let nb_att = proto_m.get_nb_attachments();
        {
          let tunn_we = o_tunn_we.as_mut().unwrap();
          tunn_we.write_header(&mut w)?;
          let mut tunn_w = CompExtWInner(&mut w, tunn_we);
          self.inner_enc.encode_msg_into(&mut tunn_w,&mut NoShadow, &mut NoYield(YieldReturn::Loop),proto_m)?;
        }
        self.current_writer = None;
        if nb_att > 0 {
          self.nb_attach_rem = nb_att;
          self.current_writer = replace(o_tunn_we, None);
        } else {
          let tunn_we = o_tunn_we.as_mut().unwrap();
        //  println!("bef tunwe write end");
          tunn_we.write_end(&mut w)?;
       //   println!("aft tunwe write end");
          tunn_we.flush_into(&mut w)?;
          w.flush()?;
        }
      },
      TunnelMessaging::ProxyToGlobal(..) => unimplemented!(),
      TunnelMessaging::ProxyFromReader(..) => unimplemented!(),
      TunnelMessaging::ReplyFromReader(..) => unimplemented!(),
      TunnelMessaging::ReadToGlobal(..) => unimplemented!(),
    }
    Ok(())
  }

  fn attach_into<W : Write,EW : ExtWrite,S : SpawnerYield> (&mut self, w : &mut W, shad : &mut EW, y : &mut S, att : &Attachment) -> Result<()> {
    // attachment require storing tunn_w in MsgEnc and flushing/writing end somehow
    // (probably by storing nb attach when encode and write end at last one 
    // or at end of encode if none)

    let mut wy = WriteYield(w,y);
    let mut w = CompExtWInner(&mut wy,shad);
    if let Some(tunn_we) = self.current_writer.as_mut() {

      {
        let mut tunn_w = CompExtWInner(&mut w, tunn_we);
        self.inner_enc.attach_into(&mut tunn_w, &mut NoShadow, &mut NoYield(YieldReturn::Loop), att)?;
      }
      self.nb_attach_rem -= 1;
      if self.nb_attach_rem == 0 {
        tunn_we.flush_into(&mut w)?;
        tunn_we.write_end(&mut w)?;
      }
    }
    // TODO fn for duplicated code
    if let Some(tunn_we) = self.current_reply_writer.as_mut() {
      {
        let mut tunn_w = CompExtWInner(&mut w, tunn_we);
        self.inner_enc.attach_into(&mut tunn_w, &mut NoShadow, &mut NoYield(YieldReturn::Loop), att)?;
      }
      self.nb_attach_rem -= 1;
      if self.nb_attach_rem == 0 {
        tunn_we.flush_into(&mut w)?;
        tunn_we.write_end(&mut w)?;
      }
    }
    Ok(())
  }
  fn decode_msg_from<R : Read,ER : ExtRead,S : SpawnerYield>(&mut self, rs : &mut R, rshad : &mut ER, y : &mut S)  -> Result<TunnelMessaging<MC>> {

    let mut ry = ReadYield(rs,y);
    let mut r = CompExtRInner(&mut ry,rshad);
    let r = &mut r;
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
            self.inner_enc.decode_msg_from(&mut tunn_r,&mut NoShadow, &mut NoYield(YieldReturn::Loop))?
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
            if need_init {
              let dest_reader = if self.nb_attach_rem > 0 {
                self.current_reader = Some(dest_reader);
                None
              } else {
                Some(dest_reader)
              };
              let init_init = self.t_readprov.reply_writer_init_init()?;
              // send to write with read (run inner service in local)
              TunnelMessaging::ReplyFromReader(proto_m, ReadMsgState::ReplyDirectInit(reply_w,dest_reader,dest,None,init_init))
            } else {
              // send to write without read (run inner service in local)
              TunnelMessaging::ReplyFromReader(proto_m, ReadMsgState::ReplyDirectNoInit(reply_w,dest))
            }
          } else {
            let dest_reader = if self.nb_attach_rem > 0 {
              self.current_reader = Some(dest_reader);
              None
            } else {
              Some(dest_reader)
            };
            // send to global service as it requires cache + if owr.1 is true send read else
            // no send read
            TunnelMessaging::ReplyFromReader(proto_m, ReadMsgState::ReplyToGlobalWithRead(dest_reader,None))
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

      if self.t_readprov.can_proxy_writer(&tunn_re) {
      if let Some((oproxy,dest_address)) = self.t_readprov.new_proxy_writer(tunn_re)? {
          Ok(TunnelMessaging::ProxyFromReader(oproxy,dest_address))
      } else { unreachable!() }
      } else {
          unimplemented!();// TODO send to global service, WARNING From address must be added,
          // this is a reference address (peer address not transport address), their is a major
          // design issue here : that require us to run auth to init owith!! in read service
          // then add this owith to message as we did with read (next to read)
          // Simplier, change tunnel proxy api and include peer address in proxy frame : ok in
          // all case : some overhead but maybe best. For instance returning from in Info (Error or
          // Reply) on do_cache() method. Getting address directly from stream could still be 
          // achieved through a Info implementation constrained on a reader with a get_address
          // method, this is not the case in mydht (reader not multiplexed with writer).
          // TODO use ProxyToGlobal
      }
    }
  }
  fn attach_from<R : Read,ER : ExtRead,S : SpawnerYield>(&mut self, rs : &mut R, rshad : &mut ER, y : &mut S, max_size : usize) -> Result<Attachment> {
    let mut ry = ReadYield(rs,y);
    let mut r = CompExtRInner(&mut ry,rshad);
    let r = &mut r;
 
    if let Some(dest_reader) = self.current_reader.as_mut() {
      let att = {
        let mut tunn_r = CompExtRInner(r, dest_reader);
        self.inner_enc.attach_from(&mut tunn_r, &mut NoShadow, &mut NoYield(YieldReturn::Loop), max_size)?
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
  /// add read stream and forward to mainloop
  Proxy(ProxyRTConf<MC>,<MC::Peer as Peer>::Address,Option<ReadBorrowStream<MC>>),
}

impl<MC : MyDHTTunnelConf> RegReaderBorrow<MyDHTTunnelConfType<MC>> for GlobalTunnelCommand<MC> {

  fn get_read(&self) -> Option<&<MC::Transport as Transport>::ReadStream> {
    match *self {
      GlobalTunnelCommand::TunnelReplyOnce(_,_,_,_,(ref rs,_,_)) |
      GlobalTunnelCommand::TunnelProxyOnce(_,(ref rs,_,_)) => Some(rs),
      _ => None,
    }
  }
}

impl<MC : MyDHTTunnelConf> ReaderBorrowable<MyDHTTunnelConfType<MC>> for LocalTunnelCommand<MC> {
  #[inline]
  fn is_borrow_read(&self) -> bool {
    match *self {
      LocalTunnelCommand::Inner(..) => false,
      LocalTunnelCommand::LocalFromRead(_,ReadMsgState::NoReply) => false,
      LocalTunnelCommand::LocalFromRead(_,ReadMsgState::ReplyDirectInit(..)) => true,
      LocalTunnelCommand::LocalFromRead(_,ReadMsgState::ReplyDirectNoInit(..)) => false,
      LocalTunnelCommand::LocalFromRead(_,ReadMsgState::ReplyToGlobalWithRead(..)) => true,
      LocalTunnelCommand::Proxy(..) => true,
    }
  }
  #[inline]
  fn is_borrow_read_end(&self) -> bool {
    match *self {
      LocalTunnelCommand::Inner(..) => false,
      LocalTunnelCommand::LocalFromRead(_,ReadMsgState::NoReply) => true, // end for now as no mechanism to consume end of query (possible reply payload for instance)
      LocalTunnelCommand::LocalFromRead(_,ReadMsgState::ReplyDirectInit(..)) => true,
      LocalTunnelCommand::LocalFromRead(_,ReadMsgState::ReplyDirectNoInit(..)) => true, // same as NoReply case TODO some read callback in ReaderBorrowable trait
      LocalTunnelCommand::LocalFromRead(_,ReadMsgState::ReplyToGlobalWithRead(..)) => true,
      LocalTunnelCommand::Proxy(..) => true,
    }
  }
  #[inline]
  fn put_read(&mut self, read : <MC::Transport as Transport>::ReadStream, shad : <MC::Peer as Peer>::ShadowRMsg, token : usize, wr : &mut TunnelWriterReader<MC>) {
    match *self {
      LocalTunnelCommand::Inner(..) => (),
      LocalTunnelCommand::LocalFromRead(_,ReadMsgState::NoReply) => (),
      LocalTunnelCommand::LocalFromRead(_,ReadMsgState::ReplyDirectInit(_,ref mut dr,_,ref mut ost,_)) 
      | LocalTunnelCommand::LocalFromRead(_,ReadMsgState::ReplyToGlobalWithRead(ref mut dr,ref mut ost))
        => {
          if wr.current_reader.is_some() {
            *dr = replace(&mut wr.current_reader,None);
          }
          *ost = Some((read,shad,token))
        },
      LocalTunnelCommand::LocalFromRead(_,ReadMsgState::ReplyDirectNoInit(..)) => (),
      LocalTunnelCommand::Proxy(_,_,ref mut ost) => *ost = Some((read,shad,token)),
    }
  }

}
pub enum LocalTunnelReply<MC : MyDHTTunnelConf> {
  /// TODO remove ??
  Inner(MC::InnerReply),
  Api(MC::InnerReply),
// TODO  DestFromReader(MC::InnerCommand),
}

pub type MovableRead<MC : MyDHTTunnelConf> = (<MC::Transport as Transport>::ReadStream, <MC::Peer as Peer>::ShadowRMsg);
pub enum GlobalTunnelCommand<MC : MyDHTTunnelConf> {
  Inner(MC::InnerCommand),
  NewOnline(MC::PeerRef),
  Offline(MC::PeerRef),

  // dest is global service
 
  /// message has been read, reply need to be instantiated from global (withread)
  DestReplyFromGlobal(MC::InnerCommand,DestFullRTConf<MC>,usize,Option<MovableRead<MC>>),
  TransmitReply(ReadMsgState<MC>,MC::InnerCommand),

  // dest is write service
  
  TunnelSendOnce(FullWTConf<MC>,MC::InnerCommand),
  /// Reply to query with tunnel info
  TunnelReplyOnce(MC::InnerCommand,ReplyWriterInitTConf<MC>,ReplyWriterTConf<MC>,DestFullRTConf<MC>,ReadBorrowStream<MC>),
  TunnelProxyOnce(ProxyRTConf<MC>,ReadBorrowStream<MC>),

}

// TODO box it !!!!!!!!! (better yet full of heap struct (vec))
type FullWTConf<MC : MyDHTTunnelConf> = FullW<MultipleReplyInfo<MC::TransportAddress>,MultipleErrorInfo,TunPeer<MC::Peer,MC::PeerRef>, MC::LimiterW,
  FullW<MultipleReplyInfo<MC::TransportAddress>, MultipleErrorInfo,TunPeer<MC::Peer,MC::PeerRef>, MC::LimiterW,Nope>>; 


type FullRTConf<MC : MyDHTTunnelConf> = FullR<MultipleReplyInfo<MC::TransportAddress>, MultipleErrorInfo, TunPeer<MC::Peer,MC::PeerRef>, MC::LimiterR>;

type ProxyRTConf<MC : MyDHTTunnelConf> = ProxyFull<FullRTConf<MC>, MC::SSW, MC::LimiterW, MC::LimiterR>;

type DestFullRTConf<MC : MyDHTTunnelConf> = DestFull<FullRTConf<MC>,MC::SSR, MC::LimiterR>;

type ReplyWriterTConf<MC : MyDHTTunnelConf> = ReplyWriter<MC::LimiterW,MC::SSW>; 

type ReplyWriterInitTConf<MC : MyDHTTunnelConf> = (MC::LimiterR,MC::LimiterW,usize); 

pub enum GlobalTunnelReply<MC : MyDHTTunnelConf> {
  /// inner service return a result for api
  Api(MC::InnerReply),
  SendCommandTo(MC::PeerRef,MC::InnerCommand),
  TryReplyFromReader(MC::InnerCommand),
  NoRep,
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
        Ok(match rep {
          GlobalTunnelReply::TryReplyFromReader(inner_reply) => {
            // nope (inner command might be remove)
            unreachable!()
          },
          GlobalTunnelReply::Api(inner_reply) => {
            // from global
            unreachable!()
          },
          GlobalTunnelReply::SendCommandTo(dest, inner_command) => {
            // from global
            unreachable!()
          },
          GlobalTunnelReply::NoRep => LocalReply::Read(ReadReply::NoReply),
        })
 
      },
      LocalTunnelCommand::LocalFromRead(cmd,read_state) => {
        let rep = self.inner.call(GlobalCommand::Distant(self.with.clone(),cmd),async_yield)?;
        Ok(match rep {
          GlobalTunnelReply::TryReplyFromReader(inner_reply) => {

            match read_state {
/*              ReadMsgState::ReplyDirectInit(repconf,dest_add) => {
                LocalReply::Read(ReadReply::Global(GlobalTunnelCommand::Reply(repconf,inner_reply))),
              },
              ReadMsgState::ReplyDirectNoInit(repconf,dest_add) => {
                LocalReply::Read(ReadReply::Global(GlobalTunnelCommand::Reply(repconf,inner_reply))),
              },
              ReadMsgState::ReplyToGlobalWithRead(destrtconf) => {
                LocalReply::Read(ReadReply::Global(GlobalTunnelCommand::TransmitReply(destrtconf,inner_reply))),
              },*/
              ReadMsgState::NoReply => {
                // TODO log warning with slog use
                println!("tunnel without reply, local tunnel service reply not send");
                LocalReply::Read(ReadReply::NoReply)
              },
              ReadMsgState::ReplyDirectInit(mut reply_writer,Some(mut dest_read),dest_add,Some(rs),Some(reply_writer_init)) => {
                let command : GlobalTunnelCommand<MC> = GlobalTunnelCommand::TunnelReplyOnce(inner_reply,reply_writer_init,reply_writer,dest_read,rs);
                LocalReply::Read(ReadReply::MainLoop(MainLoopCommand::ForwardServiceOnce(
                   None,Some(dest_add),FWConf {
                    nb_for : 0,
                    discover : true,
                  }, command)))
              },
              _ => {
                // TODO need a second command to mainloop to rewire read stream!! plus unyield on
                // rewire as racy !!! -> bad design, read should not be call from global service
                // (tunnel api should init without read with intermediatory type needing init from
                // tunnel)
                LocalReply::Read(ReadReply::Global(GlobalCommand::Distant(self.with.clone(),GlobalTunnelCommand::TransmitReply(read_state,inner_reply))))
              },
            }
  //GlobalTunnelCommand::DestReplyFromGlobal(MC::InnerCommand,DestFullRTConf<MC>,usize,Option<MovableRead<MC>>),
            //LocalReply::Read(ReadReply::Global(GlobalCommand::Distant(Some(dest.clone()),GlobalTunnelCommand::(inner_command))))
          },
          GlobalTunnelReply::Api(inner_reply) => {
            // from global
            if inner_reply.is_api_reply() {
              LocalReply::Api(LocalTunnelReply::Api(inner_reply))
            } else {
              LocalReply::Read(ReadReply::NoReply)
            }
            // TODO end reading from read state in message (similar to send read)!! currently
            // single message, issue with reuse : should be include in reuse strategy for transport
            // stream (previous read_state in msgdec). TODO put a call back on stream close?
            // warning do not do the reading elsewhere than readservice (mechanism for borrowing
            // read stream is wrong here, or do it at reuse of stream which means putting the
            // reader back in the msgenc)
          },
          GlobalTunnelReply::SendCommandTo(dest, inner_command) => {
            // only for global call
            unreachable!()
          },
          GlobalTunnelReply::NoRep => LocalReply::Read(ReadReply::NoReply),
          // Same as api : todo read end of message (could have a reply payload)
        })
      },
      LocalTunnelCommand::Proxy(_prox_w,_dest_add,None) => unreachable!(),
      LocalTunnelCommand::Proxy(prox_w,dest_add,Some(rbs)) => {
        // TODO use a MCCommand instead for direct proxy (here we use local service which is wrong
        // (at the time local service will mostly be configured as blocking thread because read is
        // close after so no use to do it before we reuse read service))
        Ok(LocalReply::Read(ReadReply::MainLoop(MainLoopCommand::ForwardServiceOnce(
               None,Some(dest_add),FWConf {
                nb_for : 0,
                discover : true,
              }, GlobalTunnelCommand::TunnelProxyOnce(prox_w,rbs)))))
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
      GlobalTunnelCommand::TransmitReply(state,reply_command) => {
        match state {
          ReadMsgState::NoReply => GlobalReply::NoRep, // TODO add a warning as it should not have been send
          ReadMsgState::ReplyDirectInit(_,_,_,_,Some(_)) => unreachable!(),
          ReadMsgState::ReplyDirectInit(mut reply_writer,Some(mut dest_read),dest_add,Some(rs),None) => {
            let reply_writer_init = self.tunnel.reply_writer_init_init(&mut reply_writer, &mut dest_read)?;
            let dest_k = self.address_key.get(&dest_add).map(|k|k.clone());
            let command : GlobalTunnelCommand<MC> = GlobalTunnelCommand::TunnelReplyOnce(reply_command,reply_writer_init,reply_writer,dest_read,rs);
              GlobalReply::ForwardOnce(dest_k,Some(dest_add), FWConf {
                nb_for : 0,
                discover : true,
              }, command)
          },
          ReadMsgState::ReplyDirectNoInit(reply_writer,dest_add) => {
            unimplemented!(); // same as direct init without the read
          },
          ReadMsgState::ReplyToGlobalWithRead(Some(dest_read),Some((read_stream,read_shadow,read_slab_token))) => {
            unimplemented!(); // TODO send back to a reader with cache read result ?? Do NOT read into global service
          },
          ReadMsgState::ReplyDirectInit(..) => unreachable!(), // must have borrowed stream, bug otherwhise
          ReadMsgState::ReplyToGlobalWithRead(..) => unreachable!(),
        }
      },
      GlobalTunnelCommand::DestReplyFromGlobal(inner_command,dest_full_read,read_old_token,o_read) => {
        unimplemented!()
      },
      GlobalTunnelCommand::NewOnline(pr) => {
        self.address_key.insert(pr.borrow().get_address().clone(),pr.borrow().get_key());
        self.tunnel.route_prov.add_online(TunPeer::new(pr));
        if self.to_send.len() > 0 && self.tunnel.route_prov.enough_peer() {
          let mut cmds = Vec::with_capacity(self.to_send.len());
          // send all cached
          while let Some((dest, inner_command)) = self.to_send.pop_front() {
            let (tunn_we,dest_add) = self.tunnel.new_writer(&TunPeer::new(dest));
            let dest_k = self.address_key.get(&dest_add).map(|k|k.clone());
            let command : GlobalTunnelCommand<MC> = GlobalTunnelCommand::TunnelSendOnce(tunn_we,inner_command);
            cmds.push(GlobalReply::ForwardOnce(dest_k,Some(dest_add), FWConf {
              nb_for : 0,
              discover : true,
            }, command));
          }
          GlobalReply::Mult(cmds)
        } else {
          GlobalReply::NoRep
        }
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
        match rep {
          GlobalTunnelReply::TryReplyFromReader(inner_reply) => {
            // only for local
            unreachable!()
          },
          GlobalTunnelReply::Api(inner_reply) => {
            if inner_reply.is_api_reply() {
              GlobalReply::Api(GlobalTunnelReply::Api(inner_reply))
            } else {
              GlobalReply::NoRep
            }
          },
          GlobalTunnelReply::SendCommandTo(dest, inner_command) => {
            if self.tunnel.route_prov.enough_peer() {

              let (tunn_we,dest_add) = self.tunnel.new_writer(&TunPeer::new(dest));
              let dest_k = self.address_key.get(&dest_add).map(|k|k.clone());
              let command : GlobalTunnelCommand<MC> = GlobalTunnelCommand::TunnelSendOnce(tunn_we,inner_command);
              GlobalReply::ForwardOnce(dest_k,Some(dest_add), FWConf {
                nb_for : 0,
                discover : true,
              }, command)
            } else {
              //debug!("query peers : {:?}", self.tunnel.route_prov.route_len());
              self.to_send.push_back((dest,inner_command));
              // plus one for case where dest in pool
              GlobalReply::MainLoop(MainLoopSubCommand::PoolSize(self.tunnel.route_prov.route_len() + 1))
            }
          },

          GlobalTunnelReply::NoRep => GlobalReply::NoRep,
        }
/*        match rep.get_api_reply() {
          Some(aid),r
        }

  fn get_api_reply(&self) -> Option<ApiQueryId>;*/
      },
      GlobalTunnelCommand::TunnelSendOnce(..)
      | GlobalTunnelCommand::TunnelProxyOnce(..)
      | GlobalTunnelCommand::TunnelReplyOnce(..) => unreachable!(),
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
      current_reply_writer : None,
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
      | TunnelMessaging::TunnelReplyOnce(ref inner_pmes,_,_,_,_)
      | TunnelMessaging::ReplyFromReader(ref inner_pmes,_) 
        => inner_pmes.get_nb_attachments(),
      TunnelMessaging::ProxyToGlobal(..)
      | TunnelMessaging::ProxyFromReader(..)
      | TunnelMessaging::TunnelProxyOnce(..)
      | TunnelMessaging::ReadToGlobal(..)
        => 0,
    }

  }
  fn get_attachments(&self) -> Vec<&Attachment> {
    match *self {
      TunnelMessaging::TunnelSendOnce(_,ref inner_pmes)
      | TunnelMessaging::TunnelReplyOnce(ref inner_pmes,_,_,_,_)
      | TunnelMessaging::ReplyFromReader(ref inner_pmes,_) 
        => inner_pmes.get_attachments(),
      TunnelMessaging::ProxyToGlobal(..)
      | TunnelMessaging::ProxyFromReader(..)
      | TunnelMessaging::TunnelProxyOnce(..)
      | TunnelMessaging::ReadToGlobal(..)
        => Vec::new(),
    }
  }
}

impl<MC : MyDHTTunnelConf> SettableAttachments for TunnelMessaging<MC> {
  fn attachment_expected_sizes(&self) -> Vec<usize> {
    match *self {
      TunnelMessaging::TunnelSendOnce(_,ref inner_pmes)
      | TunnelMessaging::TunnelReplyOnce(ref inner_pmes,_,_,_,_)
      |  TunnelMessaging::ReplyFromReader(ref inner_pmes,_) 
        => inner_pmes.attachment_expected_sizes(),
      TunnelMessaging::ProxyToGlobal(..)
      | TunnelMessaging::ProxyFromReader(..)
      | TunnelMessaging::TunnelProxyOnce(..)
      | TunnelMessaging::ReadToGlobal(..)
        => Vec::new(),
    }
  }
  fn set_attachments(& mut self, at : &[Attachment]) -> bool {
    match *self {
      TunnelMessaging::TunnelSendOnce(_,ref mut  inner_pmes)
      | TunnelMessaging::TunnelReplyOnce(ref mut inner_pmes,_,_,_,_)
      |  TunnelMessaging::ReplyFromReader(ref mut inner_pmes,_) 
        => inner_pmes.set_attachments(at),
      TunnelMessaging::ProxyToGlobal(..) 
      | TunnelMessaging::ProxyFromReader(..)
      | TunnelMessaging::TunnelProxyOnce(..)
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
      LocalTunnelCommand::Proxy(..) => false,
    }
  }
  fn set_api_reply(&mut self, i : ApiQueryId) {
    match *self {
      LocalTunnelCommand::LocalFromRead(ref mut inn_c,_) |
      LocalTunnelCommand::Inner(ref mut inn_c) => inn_c.set_api_reply(i),
      LocalTunnelCommand::Proxy(..) => (),
    }
  }
  fn get_api_reply(&self) -> Option<ApiQueryId> {
    match *self {
      LocalTunnelCommand::LocalFromRead(ref inn_c,_) |
      LocalTunnelCommand::Inner(ref inn_c) => inn_c.get_api_reply(),
      LocalTunnelCommand::Proxy(..) => None,
    }
  }
}


impl<MC : MyDHTTunnelConf> Clone for LocalTunnelCommand<MC> {
  fn clone(&self) -> Self {
    match *self {
      LocalTunnelCommand::LocalFromRead(ref inn_c,_) |
      LocalTunnelCommand::Inner(ref inn_c) => LocalTunnelCommand::Inner(inn_c.clone()),
      LocalTunnelCommand::Proxy(..) => unreachable!(), //TODO clone wrong trait??
    }
  }
}

impl<MC : MyDHTTunnelConf> ApiRepliable for LocalTunnelReply<MC> {
  fn get_api_reply(&self) -> Option<ApiQueryId> {
    match *self {
      LocalTunnelReply::Inner(ref inn_c) => inn_c.get_api_reply(),
      LocalTunnelReply::Api(ref inn_c) => inn_c.get_api_reply(),
    }
  }
}

impl<MC : MyDHTTunnelConf> ApiQueriable for GlobalTunnelCommand<MC> {
  fn is_api_reply(&self) -> bool {
    match *self {
      GlobalTunnelCommand::DestReplyFromGlobal(ref inn_c,_,_,_) |
      GlobalTunnelCommand::Inner(ref inn_c) => inn_c.is_api_reply(),
      GlobalTunnelCommand::NewOnline(..)
      | GlobalTunnelCommand::Offline(..)
      | GlobalTunnelCommand::TransmitReply(..) 
      | GlobalTunnelCommand::TunnelReplyOnce(..)
      | GlobalTunnelCommand::TunnelProxyOnce(..)
      => false,
      GlobalTunnelCommand::TunnelSendOnce(_,ref i_com) => i_com.is_api_reply(),
    }
  }
  fn set_api_reply(&mut self, i : ApiQueryId) {
    match *self {
      GlobalTunnelCommand::DestReplyFromGlobal(ref mut inn_c,_,_,_) |
      GlobalTunnelCommand::Inner(ref mut inn_c) => inn_c.set_api_reply(i),
      GlobalTunnelCommand::NewOnline(..)
      | GlobalTunnelCommand::Offline(..)
      | GlobalTunnelCommand::TransmitReply(..)
      | GlobalTunnelCommand::TunnelProxyOnce(..)
      | GlobalTunnelCommand::TunnelReplyOnce(..) => (),
      GlobalTunnelCommand::TunnelSendOnce(_,ref mut i_com) => i_com.set_api_reply(i),
    }
  }
  fn get_api_reply(&self) -> Option<ApiQueryId> {
    match *self {
      GlobalTunnelCommand::DestReplyFromGlobal(ref inn_c,_,_,_) |
      GlobalTunnelCommand::Inner(ref inn_c) => inn_c.get_api_reply(),
      GlobalTunnelCommand::NewOnline(..)
      | GlobalTunnelCommand::Offline(..)
      | GlobalTunnelCommand::TransmitReply(..)
      | GlobalTunnelCommand::TunnelProxyOnce(..)
      | GlobalTunnelCommand::TunnelReplyOnce(..) => None,
      GlobalTunnelCommand::TunnelSendOnce(_,ref i_com) => i_com.get_api_reply(),
    }
  }
}


impl<MC : MyDHTTunnelConf> Clone for GlobalTunnelCommand<MC> {
  fn clone(&self) -> Self {
    match *self {
      GlobalTunnelCommand::DestReplyFromGlobal(ref inn_c,_,_,_) => unreachable!(),
      GlobalTunnelCommand::TransmitReply(..) => unreachable!(),
      GlobalTunnelCommand::Inner(ref inn_c) => GlobalTunnelCommand::Inner(inn_c.clone()),
      GlobalTunnelCommand::NewOnline(ref rp) => GlobalTunnelCommand::NewOnline(rp.clone()),
      GlobalTunnelCommand::Offline(ref rp) => GlobalTunnelCommand::Offline(rp.clone()),
      GlobalTunnelCommand::TunnelSendOnce(ref tw,ref i_com) => unreachable!(),
      GlobalTunnelCommand::TunnelProxyOnce(..) => unreachable!(),
      GlobalTunnelCommand::TunnelReplyOnce(..) => unreachable!(),
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
      GlobalTunnelReply::TryReplyFromReader(..) => None,
      GlobalTunnelReply::NoRep => None,
    }
  }
}


