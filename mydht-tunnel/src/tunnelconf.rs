//! tunnel definition for this crate
use tunnel::{
  Peer,
  TunnelCache,
  TunnelCacheErr,
  CacheIdProducer,
  RouteProvider,
};
use readwrite_comp::{
  MultiRExt,
};
use rand::thread_rng;
use rand::OsRng;
use rand::Rng;

use mydht::transportif::{
  Transport,
};
use std::io::{
  Result,
  Error as IoError,
  ErrorKind as IoErrorKind,
};
use std::fmt::Debug;
use tunnel::info::multi::{
  MultipleReplyMode,
  ReplyInfoProvider,
  MultipleReplyInfo,
};
use tunnel::info::error::{
  MultipleErrorInfo,
  MultipleErrorMode,
  MulErrorProvider,
  NoErrorProvider,
};
use tunnel::full::{
  Full,
  FullW,
  GenTunnelTraits,
  TunnelCachedWriterExt,
  ErrorWriter,
};

use mydht::peer::{
  Peer as MPeer,
};
use mydht::utils::{
  Ref,
};
//use mydht::peer::Peer;
use tunnel::nope::{
  Nope,
  TunnelNope,
};
use super::{
  MyDHTTunnelConf,
  TunPeer,
  SSWCache,
};
use std::marker::PhantomData;
use mydht::kvcache::{
  Cache,
};


/// route provider
pub struct Rp<RP> {
  pub me : RP,
  /// connected peers
  pub peers : Vec<RP>,
  /// lifetime relatid temp
  pub dests : RP,
  pub positions : Vec<usize>,
  pub positions_res : Vec<usize>,
  /// route length : static (tunnel api is minimal could change from this static one)
  pub routelength : usize,
  /// to test route as variable length, a random bias can be use : + or - this (0 for full static
  /// length)
  pub routebias : usize,
  pub rng : OsRng,
}

use std::collections::HashMap;

pub struct ReplyTraits<MC : MyDHTTunnelConf>(PhantomData<MC>);
pub struct TunnelTraits<MC : MyDHTTunnelConf>(PhantomData<MC>);

pub struct CachedInfoManager<MC : MyDHTTunnelConf> {
  pub cache_ssw : MC::CacheSSW,
  pub cache_ssr : MC::CacheSSR,

  pub cache_erw : MC::CacheErW,
  pub cache_err : MC::CacheErR,

  pub keylength : usize,

  pub rng : OsRng,

  pub _ph : PhantomData<MC>,
}


impl<MC : MyDHTTunnelConf> TunnelCache<SSWCache<MC>,MultiRExt<MC::SSR>>
 for CachedInfoManager<MC> {
  
  #[inline]
  fn put_symw_tunnel(&mut self, k : Vec<u8>, v : SSWCache<MC>) -> Result<()> {
    self.cache_ssw.add_val_c(k,v);
    Ok(())
  }
  #[inline]
  fn get_symw_tunnel(&mut self, k : &Vec<u8>) -> Result<&mut SSWCache<MC>> {
    self.cache_ssw.get_val_mut_c(k).map(|v|Ok(v)).unwrap_or(Err(IoError::new(IoErrorKind::Other, "")))
  }
  #[inline]
  fn remove_symw_tunnel(&mut self, k : &Vec<u8>) -> Result<SSWCache<MC>> {
    self.cache_ssw.remove_val_c(k).map(|v|Ok(v)).unwrap_or(Err(IoError::new(IoErrorKind::Other, "")))
  }

  fn has_symw_tunnel(&mut self, k : &Vec<u8>) -> bool {
    self.cache_ssw.has_val_c(k)
  }

  fn put_symr_tunnel(&mut self, v : MultiRExt<MC::SSR>) -> Result<Vec<u8>> {
    let k =  self.new_cache_id();
    self.cache_ssr.add_val_c(k.clone(),v);
    Ok(k)
  }
  fn get_symr_tunnel(&mut self, k : &Vec<u8>) -> Result<&mut MultiRExt<MC::SSR>> {
    self.cache_ssr.get_val_mut_c(k).map(|v|Ok(v)).unwrap_or(Err(IoError::new(IoErrorKind::Other, "")))
  }
  fn remove_symr_tunnel(&mut self, k : &Vec<u8>) -> Result<MultiRExt<MC::SSR>> {
    self.cache_ssr.remove_val_c(k).map(|v|Ok(v)).unwrap_or(Err(IoError::new(IoErrorKind::Other, "")))
  }

  fn has_symr_tunnel(&mut self, k : &Vec<u8>) -> bool {
    self.cache_ssr.has_val_c(k)
  }
}

impl<MC : MyDHTTunnelConf> TunnelCacheErr<(ErrorWriter,<MC::Transport as Transport>::Address), MultipleErrorInfo> 
//impl<MC : MyDHTTunnelConf> TunnelCacheErr<(ErrorWriter,<TunPeer<MC::Peer,MC::PeerRef> as Peer>::Address), MultipleErrorInfo> 
 for CachedInfoManager<MC> {
  fn put_errw_tunnel(&mut self, k : Vec<u8>, v : (ErrorWriter,<MC::Transport as Transport>::Address)) -> Result<()> {
    self.cache_erw.add_val_c(k,v);
    Ok(())
  }

  fn get_errw_tunnel(&mut self, k : &Vec<u8>) -> Result<&mut (ErrorWriter,<MC::Transport as Transport>::Address)> {
    self.cache_erw.get_val_mut_c(k).map(|v|Ok(v)).unwrap_or(Err(IoError::new(IoErrorKind::Other, "")))
  }
  fn has_errw_tunnel(&mut self, k : &Vec<u8>) -> bool {
    self.cache_erw.has_val_c(k)
  }

  fn put_errr_tunnel(&mut self, k : Vec<u8>, v : Vec<MultipleErrorInfo>) -> Result<()> {
    self.cache_err.add_val_c(k,v);
    Ok(())
  }
  fn get_errr_tunnel(&mut self, k : &Vec<u8>) -> Result<&Vec<MultipleErrorInfo>> {
    self.cache_err.get_val_c(k).map(|v|Ok(v)).unwrap_or(Err(IoError::new(IoErrorKind::Other, "")))
  }
  fn has_errr_tunnel(&mut self, k : &Vec<u8>) -> bool {
    self.cache_err.has_val_c(k)
  }

}

impl<MC : MyDHTTunnelConf> CacheIdProducer
 for CachedInfoManager<MC> {
  fn new_cache_id (&mut self) -> Vec<u8> {
    let mut res = vec![0;self.keylength];
    self.rng.fill_bytes(&mut res);
    res
  }
}
/// currently same conf as in tunnel crate tests
impl<MC : MyDHTTunnelConf> GenTunnelTraits for ReplyTraits<MC> {
  type P = TunPeer<MC::Peer,MC::PeerRef>;
  type SSW = MC::SSW;
  type SSR = MC::SSR;
  type TC = Nope;
  type LW = MC::LimiterW;
  type LR = MC::LimiterR;
  type RP = Nope;
  type RW = Nope;
  type REP = ReplyInfoProvider<
    Self::P,
    Self::SSW,
    Self::SSR,
    Self::SP,
  >;
  type SP = MC::SP;
  type EP = NoErrorProvider;
  type TNR = TunnelNope<Self::P>;
  type EW = ErrorWriter;
}
/// currently same conf as in tunnel crate tests
impl<MC : MyDHTTunnelConf> GenTunnelTraits for TunnelTraits<MC> {
  type EW = ErrorWriter;
  type P = TunPeer<MC::Peer,MC::PeerRef>;
  type SSW = MC::SSW;
  type SSR = MC::SSR;
  type TC = CachedInfoManager<MC>;
  type LW = MC::LimiterW;
  type LR = MC::LimiterR;
  type RP = Rp<TunPeer<MC::Peer,MC::PeerRef>>;
  type TNR = Full<ReplyTraits<MC>>;
  type RW = FullW<MultipleReplyInfo<<Self::P as Peer>::Address>, MultipleErrorInfo,Self::P, Self::LW,Nope>;
  type REP = ReplyInfoProvider<
//    SizedWindows<TestSizedWindows>,
//    Full<ReplyTraits<P>>,
    Self::P,
    Self::SSW,
    Self::SSR,
    Self::SP,
  >;
  type SP = MC::SP;
  type EP = MulErrorProvider;
}


impl<P : MPeer,PR : Ref<P> + Clone + Debug> Rp<TunPeer<P,PR>> {
  pub fn enough_peer(&self) -> bool {
    if self.peers.len() < self.route_len() {
      false
    } else {
      true
    }
  }
  pub fn route_len(&self) -> usize {
    self.routelength + self.routebias
  }
  pub fn add_online(&mut self, v : TunPeer<P,PR>) {
    self.peers.push(v)
  }
  pub fn rem_online(&mut self, v : TunPeer<P,PR>) {
    if let Some(ix) = self.peers.iter().position(|tp|tp.get_address() == v.get_address()) {
      self.peers.swap_remove(ix);
    }
  }
  fn exact_rand(&mut self, sending : bool, other : &TunPeer<P,PR>) -> Vec<&TunPeer<P,PR>> {
    let min_no_repeat = if self.routebias > 0 {
      let b = self.rng.next_u32() as usize % ((self.routebias * 2) + 1);
      if b > self.routebias {
        self.routelength - self.routebias + b
      } else {
        self.routelength - b
      } 
    } else {
      self.routelength
    };
    let c_len = self.peers.len();
    assert!(c_len < min_no_repeat);
    /*if c_len < min_no_repeat {
      return Err(IoError::new(IoErrorKind::Other, "Unchecked call to cache exact_rand, insufficient value"));
    }*/
    let mut result = Vec::with_capacity(min_no_repeat);
    if sending {
      result.push(&self.me);
    }
    let mut nb_done = 0;
    while nb_done < min_no_repeat {
      let end = c_len - nb_done;
      let pos = self.rng.next_u64() as usize % end;
      self.positions_res[nb_done] = self.positions[pos];
      self.positions[pos] = self.positions[end - 1];
      self.positions[end - 1] = self.positions_res[nb_done];
      nb_done += 1;
    }
    for p in &self.positions_res[..min_no_repeat] {
      result.push(self.peers.get(*p).unwrap())
    }
    if sending {
      if result.len() > 1 && result.get(result.len()-1).unwrap().get_address() == other.get_address() {
        if result.len() > 2 {
          let (a,b) = (result.len()-2,result.len()-1);
          // can have other in route (we only ensure it is not before itself
          result.swap(a,b);
        } else {
          result.pop();
          // a bit off random distrib
          loop {
            let pos = self.rng.next_u64() as usize % c_len;
            let peer = self.peers.get(pos).unwrap();
            if let None = result.iter().position(|p|p.get_address() == peer.get_address()) {
              result.push(peer);
              break;
            }
          }
        }
      }

      self.dests = other.clone();
      result.push(&self.dests);
    } else {
      if result.len() > 1 && result.get(1).unwrap().get_address() == other.get_address() {
        if result.len() > 2 {
          result.swap(1,2);
        } else {
          // a bit off of random distrib
          loop {
            let pos = self.rng.next_u64() as usize % c_len;
            let peer = self.peers.get(pos).unwrap();
            if let None = result.iter().position(|p|p.get_address() == peer.get_address()) {
              result[1] = peer;
              break;
            }
          }

        }
      }
      result.push(&self.me);
    }

    result
  }
}
impl<P : MPeer,PR : Ref<P> + Clone + Debug> RouteProvider<TunPeer<P, PR>> for Rp<TunPeer<P, PR>> {
  fn new_route (&mut self, dest : &TunPeer<P, PR>) -> Vec<&TunPeer<P, PR>> {
    self.exact_rand(true,dest)
  }
  fn new_reply_route (&mut self, from : &TunPeer<P, PR>) -> Vec<&TunPeer<P, PR>> {
    self.exact_rand(false,from)
  }
}

