//! Store propagate service : to manage KeyVal.
//! Use internally at least for peer store, still not mandatory (could be replace by a fake
//! service) 
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
  QueryID,
  QueryMsg,
  PropagateMsg,
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

pub struct KVStoreService<P,V,S,I> {
//pub struct KVStoreService<V : KeyVal, S : KVStore<V>> {
  /// Fn to init store, is expected to be called only once (so returning error at second call
  /// should be fine)
  init_store : I,
  store : Option<S>,
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
  Find(QueryMsg<P>, V::Key),
  FindLocally(V::Key,ApiQueryId),
}

pub enum KVStoreReply {
  /// nb propagate, Option usize is optional query TODO replace by ApiReply as generic forward
  //Send(Option<usize>,usize),
  //Error(Option<usize>,usize),
  TODO,
  /// TODO APi with ServiceReply as val
  Api,
}

impl<P : Peer, V : KeyVal, S : KVStore<V>, F : Fn() -> Result<S> + Send + 'static> Service for KVStoreService<P,V,S,F> {
  type CommandIn = KVStoreCommand<P,V>;
  type CommandOut = KVStoreReply;

  fn call<Y : SpawnerYield>(&mut self, req: Self::CommandIn, async_yield : &mut Y) -> Result<Self::CommandOut> {
    if self.store.is_none() {
      self.store = Some(self.init_store.call(())?);
    }
    let store = self.store.as_mut().unwrap();
    match req {
      KVStoreCommand::Start => (),
      KVStoreCommand::Find(querymess, key) => {

        match store.get_val(&key) {
          Some(val) => {
            // send store to peer in mode info through mainloop write proxy of ProtoMsg TODO write
            // msg of LocalServiceCommand (using opt_into de kvstoreCommand to localservicecommand) then write will send MC::ProtoMsg using Ref<ProtoMsg> : 
            // using 
//  MainLoopCommand::ProxyWritePeer(key,address,Service(MC::LocalServiceCommand)),
          },
          None => {
            // TODO check DHTRULES : need DHTRules in store!!
            // - 
    //pub fn new_hop<QR : DHTRules> (&self, p : R, qid : QueryID, qr : &QR) -> Self { for msg or
    //inner??
            // check proxy rule and possibly forward (send to mainloop where route with filter on
            // last_sent ) + recalc nb hop... Use a MainLoopCommand::ProxyWriteRoute(routefilter,
            // nb forward, Serviec(Mc::LocalServiceCommand from Kvstorcommandfinde)
          },
        }

      },
      KVStoreCommand::FindLocally(key,apiqueryid) => {
        let o_val = store.get_val(&key);
        panic!("TODO api reply and error");
        return Ok(KVStoreReply::Api)
      },

    }
    Ok(KVStoreReply::TODO)
  }
}
