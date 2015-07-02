use query::{QueryID, Query};
use std::time::Duration;

use std::sync::mpsc::{Sender};
use procs::mesgs::{PeerMgmtMessage};
use peer::Peer;
use keyval::KeyVal;
use time::Timespec;
use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};

/// cache policies apply to queries to know how long they may cache before we consider current
/// result ok. Currently our cache policies are only an expiration date.
#[derive(Debug,Copy,Clone)]
pub struct CachePolicy(pub Timespec);

impl Decodable for CachePolicy {
    fn decode<D:Decoder> (d : &mut D) -> Result<CachePolicy, D::Error> {
        d.read_i64().and_then(|sec| d.read_i32().map(|nsec| CachePolicy(Timespec{sec : sec, nsec : nsec})))
    }
}

impl Encodable for CachePolicy {
    fn encode<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
        s.emit_i64(self.0.sec).and_then(|()| s.emit_i32(self.0.nsec))
    }
}


//TODO rewrite with parameterization ok!! (generic simplecache) TODO move to mod.rs
// TODO cache of a value : here only cache of peer query
/// cache of query interface
pub trait QueryCache<P : Peer, V : KeyVal> : Send + 'static {
  fn query_add(& mut self, &QueryID, Query<P,V>);
  fn query_get(& mut self, &QueryID) -> Option<&Query<P,V>>; // TODO return Option<Query instead ??
  fn query_remove(& mut self, &QueryID);
  fn cache_clean_nodes(& mut self) -> Vec<Query<P,V>>;
}

pub fn cache_clean
 <P : Peer,
  V : KeyVal,
  QC : QueryCache<P,V>>
 (qc : & mut QC, 
  sp : &Sender<PeerMgmtMessage<P,V>>){
    let to_clean = qc.cache_clean_nodes();
    for q in to_clean.iter(){
      debug!("Cache clean process relase a query");
      q.release_query(sp);
    }
}
