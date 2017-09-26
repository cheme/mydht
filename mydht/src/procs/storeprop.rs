//! Store propagate service : to manage KeyVal.
//! Use internally at least for peer store, still not mandatory (could be replace by a fake
//! service) 
use kvstore::StoragePriority;
use rules::DHTRules;
use std::time;
use std::time::Instant;
use query::cache::{
  QueryCache,
};
use super::api::{
  ApiQueryId,
  ApiCommand,
};
use serde::{Serializer,Serialize,Deserializer};
use serde::de::DeserializeOwned;
use peer::{
  Peer,
};
use query::{
  Query,
  QReply,
  QueryID,
  QueryModeMsg,
  QueryMsg,
  PropagateMsg,
  QueryPriority,
};
use mydhtresult::{
  Result,
};
use super::mainloop::{
  MainLoopCommand,
};
use keyval::{
  KeyVal,
};
use kvstore::{
  KVStore,
  CachePolicy,
};
use service::{
  Service,
  Spawner,
  SpawnUnyield,
  SpawnSend,
  SpawnRecv,
  SpawnHandle,
  SpawnChannel,
  MioChannel,
  MioSend,
  MioRecv,
  NoYield,
  YieldReturn,
  SpawnerYield,
  WriteYield,
  ReadYield,
  DefaultRecv,
  DefaultRecvChannel,
  NoRecv,
  NoSend,
};
use super::server2::{
  ReadDest,
};
use super::{
  MyDHTConf,
  GetOrigin,
  GlobalHandleSend,
};
use super::server2::{
  ReadReply,
};
use std::marker::PhantomData;
use utils::{
  Ref,
};

pub struct KVStoreService<P,RP,V,S,I,DR,QC> {
//pub struct KVStoreService<V : KeyVal, S : KVStore<V>> {
  /// Fn to init store, is expected to be called only once (so returning error at second call
  /// should be fine)
  me : RP,
  init_store : I,
  store : Option<S>,
  dht_rules : DR,
  query_cache : QC,
  _ph : PhantomData<(P,V)>,
}

/// Proto msg for kvstore : Not mandatory (only OptInto need implementation) but can be use when
/// building Service protomsg. TODO make it multivaluated
pub enum KVStoreProtoMsg<P : Peer, V : KeyVal,R : Ref<V>> {
  FIND(QueryMsg<P>, V::Key),
  /// Depending upon stored query, should propagate
  STORE(Option<QueryID>, R),
  /// first usize is remaining nb_hop, and second is nb query forward (same behavior as for Query
  /// Msg)
  PROPAGATE(PropagateMsg<P>, R),
}
/*pub enum KVStoreProtoMsgSend<'a, P : Peer,V : KeyVal> {
  FIND(QueryMsg<P>, &'a V::Key),
  STORE(Option<QueryID>, &'a V),
}*/

/*pub struct QueryMsg<P : Peer> {
  /// Info required to identify query and its mode
  /// TODO most of internal info should be in queryconfmsg
  pub modeinfo : QueryModeMsg<P>,
  /// history of previous hop for routing (especialy in small non anonymous networks)
  pub hop_hist : Option<LastSent<P>>,
  /// storage mode (propagate, store if proxy...) 
  pub storage : StoragePriority,
  /// remaining nb hop
  pub rem_hop : u8,
  /// nb query forward
  pub nb_forw : u8,
  /// prio
  pub prio : QueryPriority,
  /// nb result expected
  pub nb_res : usize,
}*/
/*
pub enum QueryModeMsg<P> {
    /// The node to reply to, and the managed query id for this node (not our id).
    AProxy(Arc<P>, QueryID), // reply to preceding Node which keep a trace of this query  // TODO switc to arc node to avoid all clone
    /// The node to reply to, and the managed query id for this node (not our id).
    Asynch(Arc<P>, QueryID), // reply directly to given Node which keep a trace of this query
    /// The remaining number of hop before switching to AProxy. The node to reply to, and the managed query id for this node (not our id).
    AMix(u8, Arc<P>, QueryID), // after a few hop switch to asynch
}
*/
/*
pub enum LastSent<P : Peer> {
  LastSentHop(usize, VecDeque<P::Key>),
  LastSentPeer(usize, VecDeque<P::Key>),
}
*/


pub enum KVStoreCommand<P : Peer, V : KeyVal> {
  /// Do nothing but lazy initialize of store as any command.
  Start,
  Find(QueryMsg<P>, V::Key,Option<ApiQueryId>),
  FindLocally(V::Key,ApiQueryId),
  Store(QueryID,V),
  NotFound(QueryID),
  StoreLocally(V,QueryPriority,ApiQueryId),
}

pub enum KVStoreReply<P : Peer, V : KeyVal> {
  Nope,
  Found(QueryModeMsg<P>, V),
  FoundAndProxyQuery(QueryModeMsg<P>,V,QueryMsg<P>),
  FoundApi(Option<V>,ApiQueryId),
  NotFound(QueryModeMsg<P>),
  FoundApiMult(Vec<V>,ApiQueryId),
  FoundMult(QueryModeMsg<P>, Vec<V>),
  StoreLocally(ApiQueryId),
  ProxyQuery(QueryMsg<P>, <V as KeyVal>::Key),
}

impl<
  P : Peer,
  RP : Ref<P>,
  V : KeyVal, 
  S : KVStore<V>, 
  F : Fn() -> Result<S> + Send + 'static,
  DH : DHTRules,
  QC : QueryCache<P,V>,
  > Service for KVStoreService<P,RP,V,S,F,DH,QC> {
  type CommandIn = KVStoreCommand<P,V>;
  type CommandOut = KVStoreReply<P,V>;

  fn call<Y : SpawnerYield>(&mut self, req: Self::CommandIn, async_yield : &mut Y) -> Result<Self::CommandOut> {
    if self.store.is_none() {
      self.store = Some(self.init_store.call(())?);
    }
    let store = self.store.as_mut().unwrap();
    match req {
      KVStoreCommand::Start => (),
      KVStoreCommand::Store(qid,v) => {

        let removereply = match self.query_cache.query_get_mut(&qid) {
         Some(query) => {
           match *query {
             Query(_, QReply::Local(_,ref nb_res,ref mut vres,_,ref qp), _) => {
               let (ds,cp) = self.dht_rules.do_store(true, qp.clone());
               if ds {
                 store.add_val(v.clone(),cp);
               }
               vres.push(v);
               if *nb_res == vres.len() {
                 true
               } else {
                 return Ok(KVStoreReply::Nope);
               }
             },
             Query(_, QReply::Dist(ref old_mode_info,nb_res,ref mut vres,_), _) => {
               // query prio dist to 0 TODO remove StoragePriority
               let (ds,cp) = self.dht_rules.do_store(false, 0);
               if ds {
                 store.add_val(v.clone(),cp);
               }

               if !self.dht_rules.notfoundreply(&old_mode_info.get_mode()) {
                 // TODO this clone should be remove
                 return Ok(KVStoreReply::Found(old_mode_info.clone(), v));
               } else {
                 vres.push(v);
                 if nb_res == vres.len() {
                   true
                 } else {
                   return Ok(KVStoreReply::Nope);
                 }
               }

             },
           }
         },
         None => {
           // TODO log probably timeout before
           return Ok(KVStoreReply::Nope);
         },
       };
       if removereply {
         let query = self.query_cache.query_remove(&qid).unwrap();
         match query.1 {
           QReply::Local(apiqid,_,vres,_,_) => {
             return Ok(KVStoreReply::FoundApiMult(vres, apiqid));
           },
           QReply::Dist(old_mode_info,_,vres,_) => {
             return Ok(KVStoreReply::FoundMult(old_mode_info, vres));
           },
         }
       }

      },
      KVStoreCommand::StoreLocally(v,qprio,api_queryid) => {
        let (ds,cp) = self.dht_rules.do_store(true, qprio);
        if ds {
          store.add_val(v,cp);
        }
        return Ok(KVStoreReply::StoreLocally(api_queryid));
      },
      KVStoreCommand::NotFound(qid) => {
        let remove = match self.query_cache.query_get_mut(&qid) {
         Some(query) => {
          match *query {
             Query(_, QReply::Local(_,_,_,ref mut nb_not_found,_), _) |
             Query(_, QReply::Dist(_,_,_,ref mut nb_not_found), _) => {
               if *nb_not_found > 0 {
                 *nb_not_found -= 1;
                 *nb_not_found == 0 
               } else {
                 false // was at 0 meaning no not found reply : TODO some logging
               }
             },
          }
         },
         None => {
           // TODO log probably timeout before
           false
         },
       };
       if remove {
         let query = self.query_cache.query_remove(&qid).unwrap();
         match query.1 {
           QReply::Local(apiqid,_,vres,_,_) => {
             return Ok(KVStoreReply::FoundApiMult(vres, apiqid));
           },
           QReply::Dist(old_mode_info,_,vres,_) => {
             if vres.len() > 0 {
               return Ok(KVStoreReply::FoundMult(old_mode_info, vres));
             } else {
               if self.dht_rules.notfoundreply(&old_mode_info.get_mode()) {
                 return Ok(KVStoreReply::NotFound(old_mode_info));
               } else {
                 return Ok(KVStoreReply::Nope);
               }
             }
           },
         }
      }
       return Ok(KVStoreReply::Nope);
      },

      KVStoreCommand::Find(mut querymess, key,o_api_queryid) => {
        let oval = store.get_val(&key); 
        if oval.is_some() {
          querymess.nb_res -= 1;
        }
        // early exit when no need to forward
        if querymess.rem_hop == 0 || querymess.nb_res == 0 {
          if let Some(val) = oval {
            match o_api_queryid {
              Some(api_queryid) => {
                return Ok(KVStoreReply::FoundApi(Some(val),api_queryid));
              },
              None => {
                // reply
                return Ok(KVStoreReply::Found(querymess.mode_info, val));
              },
            }
          }
          if self.dht_rules.notfoundreply(&querymess.mode_info.get_mode()) {
            match o_api_queryid {
              Some(api_queryid) => {
                return Ok(KVStoreReply::FoundApi(None,api_queryid));
              },
              None => {
                return Ok(KVStoreReply::NotFound(querymess.mode_info));
              },
            }
          } else {
            return Ok(KVStoreReply::Nope);
          }
        }
        //let do_store = querymess.mode_info.do_store() && querymess.rem_hop > 0;
        let qid = if querymess.mode_info.do_store() {
          self.query_cache.new_id()
        } else {
          querymess.get_query_id()
        };
        let old_mode_info = querymess.to_next_hop(self.me.borrow(),qid, &self.dht_rules);
        // forward
        let mode = querymess.mode_info.get_mode();
        let do_reply_not_found = self.dht_rules.notfoundreply(&mode);
        let nb_not_found = if do_reply_not_found {
          self.dht_rules.notfoundtreshold(querymess.nb_forw, querymess.rem_hop, &mode)
        } else {
          0
        };
        let lifetime = self.dht_rules.lifetime(querymess.prio);
        let expire = Instant::now() + lifetime;
        let (vres,oval) = if oval.is_some() {
          if do_reply_not_found { // result is send
            let mut r = Vec::with_capacity(querymess.nb_res);
            r.push(oval.unwrap());
            (r,None)
          } else {
            (Vec::new(),oval)
          }
        } else {
          (Vec::with_capacity(querymess.nb_res),oval)
        };
        let query = if let Some(apiqid) = o_api_queryid {
          Query(qid, QReply::Local(apiqid,querymess.nb_res,vres,nb_not_found,querymess.prio), Some(expire))
        } else {
          Query(qid, QReply::Dist(old_mode_info.clone(),querymess.nb_res,vres,nb_not_found), Some(expire))
        };
        self.query_cache.query_add(qid, query);
        if oval.is_some() && !do_reply_not_found {
          return Ok(KVStoreReply::FoundAndProxyQuery(old_mode_info,oval.unwrap(),querymess));
        }
        return Ok(KVStoreReply::ProxyQuery(querymess, key));
      },
      KVStoreCommand::FindLocally(key,apiqueryid) => {
        let o_val = store.get_val(&key);
        return Ok(KVStoreReply::FoundApi(o_val,apiqueryid));
      },
    }
    Ok(KVStoreReply::Nope)
  }
}
