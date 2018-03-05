//! Store propagate service : to manage KeyVal.
//! Use internally at least for peer store, still not mandatory (could be replace by a fake
//! service) 
//! TODO for this KVStore : currently no commit are done : run some : fix rules : on drop and every
//! n insert
//! !!! TODO currently when forwarding, if no route for forward or failure to forward there is no
//! adjustment of the query : need to include a function call back message on error to
//! MainLoop::ForwardService command
//! TODO non auth may refer to owith for replying : need variant using token
use std::collections::VecDeque;
use std::mem::replace;
pub use mydht_base::route2::RouteBaseMessage;
use super::mainloop::{
  MainLoopCommand,
  MainLoopSubCommand,
};
use super::api::{
  ApiCommand,
  ApiQueryId,
};
use std::sync::{
  Arc,
  Mutex,
};
use keyval::{
  KeyVal,
  GettableAttachments,
  SettableAttachments,
  Attachment,
};
use super::deflocal::{
  GlobalDest,
};
use rules::DHTRules;
use std::time::Instant;
use procs::deflocal::{
  GlobalReply,
  GlobalCommand,
};
use query::cache::{
  QueryCache,
};
use serde::{Serialize};
use serde::de::DeserializeOwned;
use peer::{
  Peer,
};
use query::{
  Query,
  QReply,
  QueryID,
  QueryMsg,
  PropagateMsg,
  QueryPriority,
};
use mydhtresult::{
  Result,
};
use kvstore::{
  KVStore,
};
use service::{
  Service,
  Spawner,
  SpawnSend,
  SpawnChannel,
  SpawnerYield,
  SpawnSendWithHandle,
};
use super::{
  MainLoopSendIn,
  ApiHandleSend,
  ApiWeakSend,
  ApiWeakUnyield,
  PeerStoreWeakSend,
  PeerStoreWeakUnyield,
  GlobalHandleSend,
  GlobalWeakSend,
  GlobalWeakUnyield,
  FWConf,
  MyDHTConf,
  OptFrom,
  ApiQueriable,
  ApiRepliable,
  PeerStatusListener,
  RegReaderBorrow,
  PeerStatusCommand,
  MCCommand,
  MCReply,
};
use std::marker::PhantomData;
use utils::{
  Ref,
  SRef,
  SToRef,
};


// kvstore service usable as standard global service
//pub struct KVStoreServiceMD<MC : MyDHTConf,V,S,I,QC> (pub KVStoreService<MC::Peer,MC::PeerRef,V,S,I,MC::DHTRules,QC>);

/// kvstore service inner implementation TODO add local cache like original mydhtimpl (already
/// Ref<KeyVal> usage (looks odd without a cache).
/// Warning, current SRef implementation does not keep cache in handle of SRef kind service
pub struct KVStoreService<P,RP,V,RV,S,DR,QC> {
//pub struct KVStoreService<V : KeyVal, S : KVStore<V>> {
  /// Fn to init store, is expected to be called only once (so returning error at second call
  /// should be fine)
  pub me : RP,
  pub init_store : Box<Fn() -> Result<S> + Send>,
  pub init_cache : Box<Fn() -> Result<QC> + Send>,
  pub store : Option<S>,
  pub dht_rules : DR,
  pub query_cache : Option<QC>,
  /// do we forward with discover peer capability
  pub discover : bool,
  pub _ph : PhantomData<(P,V,RV)>,
}

/// Warn store and query cache are dropped and rebuild at each restart (and cache need probable a costy copy).
/// Consequence is that the service which use a spawner requiring this struct will need to run
/// indefinitely (nb iter at 0) otherwhise some query will be lost.
/// unless cache is make Send (by generalizing SRef and storing its send variant) or making cache
/// SRef (costy).
pub struct KVStoreServiceRef<RP : SRef,S,DR,QC> {
  pub me : <RP as SRef>::Send,
  pub init_store : Box<Fn() -> Result<S> + Send>,
  pub init_cache : Box<Fn() -> Result<QC> + Send>,
  pub dht_rules : DR,
  pub discover : bool,
}


impl<
  P : Peer,
  RP : Ref<P> + Clone,
  V : KeyVal, 
  VR : Ref<V>,
  S : KVStore<V>, 
  DH : DHTRules,
  QC : QueryCache<P,VR,RP>,
  > SRef for KVStoreService<P,RP,V,VR,S,DH,QC> {
  type Send = KVStoreServiceRef<RP,S,DH,QC>;
  fn get_sendable(self) -> Self::Send {
    let KVStoreService {
      me,
      init_store,
      init_cache,
      store : _,
      dht_rules,
      query_cache : _,
      discover,
      _ph,
    } = self;
    KVStoreServiceRef {
      me : me.get_sendable(),
      init_store,
      init_cache,
      dht_rules,
      discover,
    }
  }
}


impl<
  P : Peer,
  RP : Ref<P> + Clone,
  V : KeyVal, 
  VR : Ref<V>,
  S : KVStore<V>, 
  DH : DHTRules,
  QC : QueryCache<P,VR,RP>,
  > SToRef<KVStoreService<P,RP,V,VR,S,DH,QC>> for KVStoreServiceRef<RP,S,DH,QC> {
  fn to_ref(self) -> KVStoreService<P,RP,V,VR,S,DH,QC> {
    let KVStoreServiceRef {
      me,
      init_store,
      init_cache,
      dht_rules,
      discover,
    } = self;
    KVStoreService {
      me : me.to_ref(),
      init_store,
      init_cache,
      store : None,
      dht_rules,
      query_cache : None,
      discover,
      _ph : PhantomData,
    }
  }
}
 

/// Proto msg for kvstore : Not mandatory (only OptInto need implementation) but can be use when
/// building Service protomsg. TODO make it multivaluated
#[derive(Serialize,Deserialize,Debug)]
#[serde(bound(deserialize = ""))]
pub enum KVStoreProtoMsg<P : Peer, V : KeyVal,R : Ref<V> + Serialize + DeserializeOwned + Clone> {
  FIND(QueryMsg<P>, V::Key),
  /// Depending upon stored query, should propagate
  STORE(QueryID, Vec<R>),
  NOTFOUND(QueryID),
  /// first usize is remaining nb_hop, and second is nb query forward (same behavior as for Query
  /// Msg)
  PROPAGATE(PropagateMsg<P>, R),
}



/// Protomessage to use in simple Dht with no localservice command and a KV store as main service
#[derive(Serialize,Deserialize,Debug)]
#[serde(bound(deserialize = ""))]
pub enum KVStoreProtoMsgWithPeer<P : Peer, RP : Ref<P> + Serialize + DeserializeOwned + Clone, V : KeyVal,R : Ref<V> + Serialize + DeserializeOwned + Clone> {
  Main(KVStoreProtoMsg<P,V,R>),
  PeerMgmt(KVStoreProtoMsg<P,P,RP>),
}

impl<P : Peer, RP : Ref<P> + Serialize + DeserializeOwned + Clone, V : KeyVal,R : Ref<V> + Serialize + DeserializeOwned + Clone>
    GettableAttachments for KVStoreProtoMsgWithPeer<P,RP,V,R> {
  fn get_nb_attachments(&self) -> usize {
    0
  }
  fn get_attachments(&self) -> Vec<&Attachment> {
    Vec::new()
  }
}

impl<P : Peer, RP : Ref<P> + Serialize + DeserializeOwned + Clone, V : KeyVal,R : Ref<V> + Serialize + DeserializeOwned + Clone>
    SettableAttachments for KVStoreProtoMsgWithPeer<P,RP,V,R> {
  fn attachment_expected_sizes(&self) -> Vec<usize> {
    Vec::new()
  }
  fn set_attachments(& mut self, at : &[Attachment]) -> bool {
    at.len() == 0
  }
}

impl<MC : MyDHTConf<GlobalServiceCommand = KVStoreCommand<<MC as MyDHTConf>::Peer,<MC as MyDHTConf>::PeerRef,V,R>>, V : KeyVal,R : Ref<V> + Serialize + DeserializeOwned + Clone> 
  Into<MCCommand<MC>> 
  for KVStoreProtoMsgWithPeer<MC::Peer, MC::PeerRef, V,R>  where 
   <MC as MyDHTConf>::GlobalServiceChannelIn : SpawnChannel<GlobalCommand<<MC as MyDHTConf>::PeerRef, KVStoreCommand<<MC as MyDHTConf>::Peer, <MC as MyDHTConf>::PeerRef, V, R>>>,
   <MC as MyDHTConf>::GlobalServiceSpawn: Spawner<<MC as MyDHTConf>::GlobalService, GlobalDest<MC>, <<MC as MyDHTConf>::GlobalServiceChannelIn as SpawnChannel<GlobalCommand<<MC as MyDHTConf>::PeerRef, KVStoreCommand<<MC as MyDHTConf>::Peer, <MC as MyDHTConf>::PeerRef, V, R>>>>::Recv>,
//   KVStoreCommand<<MC as MyDHTConf>::Peer,<MC as MyDHTConf>::PeerRef,V,R> : PeerStatusListener<MC::PeerRef>
{
  fn into(self) -> MCCommand<MC> {
    match self {
      KVStoreProtoMsgWithPeer::Main(pmess) => MCCommand::Global(pmess.into()),
      KVStoreProtoMsgWithPeer::PeerMgmt(pmess) => MCCommand::PeerStore(pmess.into()),
    }
  }
}
impl<MC : MyDHTConf<GlobalServiceCommand = KVStoreCommand<<MC as MyDHTConf>::Peer,<MC as MyDHTConf>::PeerRef,V,R>>, V : KeyVal,R : Ref<V> + Serialize + DeserializeOwned + Clone> 
  OptFrom<MCCommand<MC>>
  for KVStoreProtoMsgWithPeer<MC::Peer, MC::PeerRef, V,R>  where 
   <MC as MyDHTConf>::GlobalServiceChannelIn : SpawnChannel<GlobalCommand<<MC as MyDHTConf>::PeerRef, KVStoreCommand<<MC as MyDHTConf>::Peer, <MC as MyDHTConf>::PeerRef, V, R>>>,
   <MC as MyDHTConf>::GlobalServiceSpawn: Spawner<<MC as MyDHTConf>::GlobalService, GlobalDest<MC>, <<MC as MyDHTConf>::GlobalServiceChannelIn as SpawnChannel<GlobalCommand<<MC as MyDHTConf>::PeerRef, KVStoreCommand<<MC as MyDHTConf>::Peer, <MC as MyDHTConf>::PeerRef, V, R>>>>::Recv>,
//   KVStoreCommand<<MC as MyDHTConf>::Peer,<MC as MyDHTConf>::PeerRef,<MC as MyDHTConf>::Peer,<MC as MyDHTConf>::PeerRef> : PeerStatusListener<MC::PeerRef>,
//   KVStoreCommand<<MC as MyDHTConf>::Peer,<MC as MyDHTConf>::PeerRef,V,R> : PeerStatusListener<MC::PeerRef>
  {
  fn can_from(c : &MCCommand<MC>) -> bool {
    match *c {
      MCCommand::Local(..) => {
        false
      },
      MCCommand::Global(ref gc) => {
        <KVStoreProtoMsg<_,_,_> as OptFrom<KVStoreCommand<_,_,_,_>>>::can_from(gc)
      },
      MCCommand::PeerStore(ref pc) => {
        <KVStoreProtoMsg<_,_,_> as OptFrom<KVStoreCommand<_,_,_,_>>>::can_from(pc)
      },
      MCCommand::TryConnect(..) => {
        false
      },
    }
  }
  fn opt_from(c : MCCommand<MC>) -> Option<Self> {
    match c {
      MCCommand::Global(pmess) => {
        <KVStoreProtoMsg<_,_,_> as OptFrom<KVStoreCommand<_,_,_,_>>>::opt_from(pmess)
          .map(|t|KVStoreProtoMsgWithPeer::Main(t))
      },
      MCCommand::PeerStore(pmess) => {
        <KVStoreProtoMsg<_,_,_> as OptFrom<KVStoreCommand<_,_,_,_>>>::opt_from(pmess)
          .map(|t|KVStoreProtoMsgWithPeer::PeerMgmt(t))
      },
      _ => None,
    }
  }
}

/*
impl OptFrom<MCCommand<TestMdhtConf>> for TestMessage<TestMdhtConf> {
  fn can_from(c : &MCCommand<TestMdhtConf>) -> bool {
    match *c {
      MCCommand::Local(ref lc) => {
        <TestCommand<TestMdhtConf> as OptInto<TestMessage<TestMdhtConf>>>::can_into(lc)
      },
      MCCommand::Global(ref gc) => {
        <TestCommand<TestMdhtConf> as OptInto<TestMessage<TestMdhtConf>>>::can_into(gc)
      },
      MCCommand::PeerStore(ref pc) => {
        <KVStoreProtoMsg<_,_,_> as OptFrom<KVStoreCommand<_,_,_,_>>>::can_from(pc)
      },
      MCCommand::TryConnect(..) => {
        false
      },

    }
  }
  fn opt_from(c : MCCommand<TestMdhtConf>) -> Option<Self> {
    match c {
      MCCommand::Local(lc) => {
        lc.opt_into()
      },
      MCCommand::Global(gc) => {
        gc.opt_into()
      },
      MCCommand::PeerStore(pc) => {
        pc.opt_into().map(|t|TestMessage::PeerMgmt(t))
      },
      MCCommand::TryConnect(..) => {
        None
      },
    }
  }
}*/



/*
#[derive(Serialize,Debug)]
pub enum KVStoreProtoMsgSend<'a, P : Peer, V : KeyVal> {
  FIND(&'a QueryMsg<P>, &'a V::Key),
  /// Depending upon stored query, should propagate
  STORE(QueryID, &'a Vec<&'a V>),
  NOTFOUND(QueryID),
  /// first usize is remaining nb_hop, and second is nb query forward (same behavior as for Query
  /// Msg)
  PROPAGATE(&'a PropagateMsg<P>, &'a V),
}*/


  //type ProtoMsg : Into<MCCommand<Self>> + SettableAttachments + GettableAttachments + OptFrom<MCCommand<Self>>;

//pub enum KVStoreCommand<P : Peer, V : KeyVal, VR> {
impl<P : Peer, PR : Ref<P>, V : KeyVal, VR : Ref<V> + Serialize + DeserializeOwned + Clone> OptFrom<KVStoreCommand<P,PR,V,VR>> for KVStoreProtoMsg<P,V,VR> {
  fn can_from (c : &KVStoreCommand<P,PR,V,VR>) -> bool {
    match *c {
      KVStoreCommand::Start => false,
      KVStoreCommand::CommitStore => false,
      KVStoreCommand::Subset(..) => false,
      KVStoreCommand::WithLocalValue(..) => false,
      KVStoreCommand::Find(..) => true,
      KVStoreCommand::FindLocally(..) => false,
      KVStoreCommand::Store(..) => true,
    //  StoreMult(QueryID,Vec<VR>),
      KVStoreCommand::NotFound(..) => true,
      KVStoreCommand::StoreLocally(..) => false,
    }

  }
  fn opt_from (c : KVStoreCommand<P,PR,V,VR>) -> Option<Self> {
    match c {
      KVStoreCommand::Start => None,
      KVStoreCommand::CommitStore => None,
      KVStoreCommand::Subset(..) => None,
      KVStoreCommand::WithLocalValue(..) => None,
      KVStoreCommand::Find(qmess, key,_) => Some(KVStoreProtoMsg::FIND(qmess,key)),
      KVStoreCommand::FindLocally(..) => None,
      KVStoreCommand::Store(qid,vrs) => {
        Some(KVStoreProtoMsg::STORE(qid,vrs))
      },
    //  StoreMult(QueryID,Vec<VR>),
      KVStoreCommand::NotFound(qid) => Some(KVStoreProtoMsg::NOTFOUND(qid)),
      KVStoreCommand::StoreLocally(..) => None,
    }
  }
}

impl<P : Peer, PR : Ref<P>, V : KeyVal, VR : Ref<V> + Serialize + DeserializeOwned + Clone> Into<KVStoreCommand<P,PR,V,VR>> for KVStoreProtoMsg<P,V,VR> {
  fn into(self) -> KVStoreCommand<P,PR,V,VR> {
    match self {
      KVStoreProtoMsg::FIND(qmes,key) => {
        KVStoreCommand::Find(qmes,key,None)
      },
      KVStoreProtoMsg::STORE(qid,refval) => {
        KVStoreCommand::Store(qid,refval)
      },
      KVStoreProtoMsg::NOTFOUND(qid) => {
        KVStoreCommand::NotFound(qid)
      },
      KVStoreProtoMsg::PROPAGATE(..) => {
        unimplemented!()
      },
    }
  }
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

#[derive(Clone)]
pub enum KVStoreCommand<P : Peer, PR, V : KeyVal, VR> {
  /// Do nothing but lazy initialize of store as any command.
  Start,
  /// force a commit for store
  CommitStore,
  /// exec a fn on a subset of the store
  Subset(usize, fn(Vec<V>) -> GlobalReply<P,PR,KVStoreCommand<P,PR,V,VR>,KVStoreReply<VR>>),
  /// exec on local value found
  WithLocalValue(V::Key, fn(Option<V>) -> GlobalReply<P,PR,KVStoreCommand<P,PR,V,VR>,KVStoreReply<VR>>),
  Find(QueryMsg<P>, V::Key,Option<ApiQueryId>),
  FindLocally(V::Key,ApiQueryId),
  Store(QueryID,Vec<VR>),
//  StoreMult(QueryID,Vec<VR>),
  NotFound(QueryID),
  StoreLocally(VR,QueryPriority,Option<ApiQueryId>),
}

impl<P : Peer, PR, V : KeyVal, VR> RouteBaseMessage<P> for KVStoreCommand<P,PR,V,VR> {
  fn get_filter_mut(&mut self) -> Option<&mut VecDeque<<P as KeyVal>::Key>> {
    match *self {
      KVStoreCommand::Find(ref mut qm,_,_) => qm.get_filter_mut(),
      _ => None,
    }
  }

  fn adjust_lastsent_next_hop(&mut self, nbquery : usize) {
    match *self {
      KVStoreCommand::Find(ref mut qm,_,_) => qm.adjust_lastsent_next_hop(nbquery),
      _ => (),
    }
 
  }
}


impl<P : Peer, PR, V : KeyVal, VR> ApiQueriable for KVStoreCommand<P,PR,V,VR> {
  fn is_api_reply(&self) -> bool {
    match *self {
      KVStoreCommand::Start => false,
      KVStoreCommand::CommitStore => false,
      KVStoreCommand::Subset(..) => false,
      KVStoreCommand::WithLocalValue(..) => false,
      KVStoreCommand::Find(..) => true,
      KVStoreCommand::FindLocally(..) => true,
      KVStoreCommand::Store(..) => false,
      KVStoreCommand::NotFound(..) => false,
      KVStoreCommand::StoreLocally(..) => true,
    }
  }
  fn set_api_reply(&mut self, i : ApiQueryId) {
    match *self {
      KVStoreCommand::Start => (),
      KVStoreCommand::Subset(..) => (),
      KVStoreCommand::WithLocalValue(..) => (),
      KVStoreCommand::CommitStore => (),
      KVStoreCommand::StoreLocally(_,_,ref mut oaqid)
        | KVStoreCommand::Find(_,_,ref mut oaqid) => {
        *oaqid = Some(i);
      },
      KVStoreCommand::FindLocally(_,ref mut oaqid) => {
        *oaqid = i;
      },
      KVStoreCommand::Store(..) => (),
      KVStoreCommand::NotFound(..) => (),
    }

  }
  fn get_api_reply(&self) -> Option<ApiQueryId> {
    match *self {
      KVStoreCommand::Start => None,
      KVStoreCommand::Subset(..) => None,
      KVStoreCommand::WithLocalValue(..) => None,
      KVStoreCommand::CommitStore => None,
      KVStoreCommand::StoreLocally(_,_,ref oaqid)
        | KVStoreCommand::Find(_,_,ref oaqid) => {
          oaqid.clone()
      },
      KVStoreCommand::FindLocally(_,ref oaqid) => {
        Some(oaqid.clone())
      },
      KVStoreCommand::Store(..) => None,
      KVStoreCommand::NotFound(..) => None,
    }
  }
}

impl<MC : MyDHTConf,P : Peer,PR,V : KeyVal,VR> RegReaderBorrow<MC> for KVStoreCommand<P, PR, V, VR> { }

impl<P : Peer,PR,V : KeyVal,VR> PeerStatusListener<PR> for KVStoreCommand<P, PR, V, VR> {
//impl<P : Peer,PR> PeerStatusListener<PR> for KVStoreCommand<P, PR, P, PR> {
  const DO_LISTEN : bool = false;
  #[inline]
  fn build_command(_ : PeerStatusCommand<PR>) -> Option<Self> {
    None
  }
}

// TODO remove local command eg subset
pub enum KVStoreCommandSend<P : Peer, PR, V : KeyVal, VR : SRef> {
  /// Do nothing but lazy initialize of store as any command.
  Start,
  CommitStore,
  Subset(usize, fn(Vec<V>) -> GlobalReply<P,PR,KVStoreCommand<P,PR,V,VR>,KVStoreReply<VR>>),
  WithLocalValue(V::Key, fn(Option<V>) -> GlobalReply<P,PR,KVStoreCommand<P,PR,V,VR>,KVStoreReply<VR>>),
  Find(QueryMsg<P>, V::Key,Option<ApiQueryId>),
  FindLocally(V::Key,ApiQueryId),
  Store(QueryID,Vec<<VR as SRef>::Send>),
//  StoreMult(QueryID,Vec<VR>),
  NotFound(QueryID),
  StoreLocally(<VR as SRef>::Send,QueryPriority,Option<ApiQueryId>),
}

impl<P : Peer, PR : Ref<P>, V : KeyVal, VR : Ref<V>> SRef for KVStoreCommand<P,PR,V,VR> {
  type Send = KVStoreCommandSend<P,PR,V,VR>;
  fn get_sendable(self) -> Self::Send {
    match self {
      KVStoreCommand::Start => KVStoreCommandSend::Start,
      KVStoreCommand::CommitStore => KVStoreCommandSend::CommitStore,
      KVStoreCommand::Subset(nb,f) => KVStoreCommandSend::Subset(nb,f),
      KVStoreCommand::WithLocalValue(k,f) => KVStoreCommandSend::WithLocalValue(k,f),
      KVStoreCommand::Find(qm,k,oaqid) => KVStoreCommandSend::Find(qm,k,oaqid),
      KVStoreCommand::FindLocally(k,oaqid) => KVStoreCommandSend::FindLocally(k,oaqid),
      KVStoreCommand::Store(qid,vrp) => KVStoreCommandSend::Store(qid,vrp.into_iter().map(|rp|rp.get_sendable()).collect()),
      KVStoreCommand::NotFound(qid) => KVStoreCommandSend::NotFound(qid),
      KVStoreCommand::StoreLocally(rp,qp,oaqid) => KVStoreCommandSend::StoreLocally(rp.get_sendable(),qp,oaqid),
    }
  }
}
 
impl<P : Peer, PR : Ref<P>, V : KeyVal, VR : Ref<V>> SToRef<KVStoreCommand<P,PR,V,VR>> for KVStoreCommandSend<P,PR,V,VR> {
  fn to_ref(self) -> KVStoreCommand<P,PR,V,VR> {
    match self {
      KVStoreCommandSend::Start => KVStoreCommand::Start,
      KVStoreCommandSend::CommitStore => KVStoreCommand::CommitStore,
      KVStoreCommandSend::Subset(nb,f) => KVStoreCommand::Subset(nb,f),
      KVStoreCommandSend::WithLocalValue(k,f) => KVStoreCommand::WithLocalValue(k,f),
      KVStoreCommandSend::Find(qm, k,oaqid) => KVStoreCommand::Find(qm,k,oaqid),
      KVStoreCommandSend::FindLocally(k,oaqid) => KVStoreCommand::FindLocally(k,oaqid),
      KVStoreCommandSend::Store(qid,vrp) => KVStoreCommand::Store(qid,vrp.into_iter().map(|rp|rp.to_ref()).collect()),
      KVStoreCommandSend::NotFound(qid) => KVStoreCommand::NotFound(qid),
      KVStoreCommandSend::StoreLocally(rp,qp,oaqid) => KVStoreCommand::StoreLocally(rp.to_ref(),qp,oaqid),
    }
  }
}
 
  


//type GlobalServiceCommand : ApiQueriable + OptInto<Self::ProtoMsg> + OptInto<KVStoreCommand<Self::Peer,Self::Peer,Self::PeerRef>> + Clone;// = GlobalCommand<Self>;


#[derive(Clone)]
pub enum KVStoreReply<VR> {
  FoundApi(Option<VR>,ApiQueryId),
  FoundApiMult(Vec<VR>,ApiQueryId),
  Done(ApiQueryId),
}

impl<VR> ApiRepliable for KVStoreReply<VR> {
  fn get_api_reply(&self) -> Option<ApiQueryId> {
    match *self {
      KVStoreReply::FoundApi(_,ref a)
      | KVStoreReply::FoundApiMult(_,ref a)
      | KVStoreReply::Done(ref a)
        => Some(a.clone()),
    }
  }
}

pub enum KVStoreReplySend<VR : SRef> {
  FoundApi(Option<<VR as SRef>::Send>,ApiQueryId),
  FoundApiMult(Vec<<VR as SRef>::Send>,ApiQueryId),
  Done(ApiQueryId),
}

impl<VR : SRef> SRef for KVStoreReply<VR> {
  type Send = KVStoreReplySend<VR>;
  fn get_sendable(self) -> Self::Send {
    match self {
      KVStoreReply::FoundApi(ovr,aqid) => KVStoreReplySend::FoundApi(ovr.map(|v|v.get_sendable()),aqid),
      KVStoreReply::FoundApiMult(vrs,aqid) => KVStoreReplySend::FoundApiMult(vrs.into_iter().map(|v|v.get_sendable()).collect(),aqid),
      KVStoreReply::Done(aqid) => KVStoreReplySend::Done(aqid),
    }
  }
}
impl<VR : SRef> SToRef<KVStoreReply<VR>> for KVStoreReplySend<VR> {
  fn to_ref(self) -> KVStoreReply<VR> {
    match self {
      KVStoreReplySend::FoundApi(ovr,aqid) => KVStoreReply::FoundApi(ovr.map(|v|v.to_ref()),aqid),
      KVStoreReplySend::FoundApiMult(vrs,aqid) => KVStoreReply::FoundApiMult(vrs.into_iter().map(|v|v.to_ref()).collect(),aqid),
      KVStoreReplySend::Done(aqid) => KVStoreReply::Done(aqid),
    }
  }
}
 
/*pub enum GlobalReply<P : Peer,PR,GSC,GSR> {
  /// forward command to list of peers or/and to nb peers from route
  Forward(Option<Vec<PR>>,Option<Vec<(<P as KeyVal>::Key,<P as Peer>::Address)>>,usize,GSC),
  /// reply to api
  Api(GSR),
  /// no rep
  NoRep,
  Mult(Vec<GlobalReply<P,PR,GSC,GSR>>),
}*/


impl<
  P : Peer,
  RP : Ref<P> + Clone,
  V : KeyVal, 
  VR : Ref<V>,
  S : KVStore<V>, 
  DH : DHTRules,
  QC : QueryCache<P,VR,RP>,
  > Service for KVStoreService<P,RP,V,VR,S,DH,QC> {
  type CommandIn = GlobalCommand<RP,KVStoreCommand<P,RP,V,VR>>;
  type CommandOut = GlobalReply<P,RP,KVStoreCommand<P,RP,V,VR>,KVStoreReply<VR>>;
  //    KVStoreReply<P,V,RP>;

  fn call<Y : SpawnerYield>(&mut self, req: Self::CommandIn, _async_yield : &mut Y) -> Result<Self::CommandOut> {
    if self.store.is_none() {
      self.store = Some(self.init_store.call(())?);
    }
    if self.query_cache.is_none() {
      self.query_cache = Some(self.init_cache.call(())?);
    }

    let store = self.store.as_mut().unwrap();
    let query_cache = self.query_cache.as_mut().unwrap();
    let (_is_local,owith,req) = match req {
      GlobalCommand::Local(r) => (true,None,r), 
      GlobalCommand::Distant(ow,r) => (false,ow,r), 
    };
    match req {
      KVStoreCommand::Start => (),
      KVStoreCommand::Subset(nb,f) => {
        match store.get_next_vals(nb) {
          Some(vals) => {
            debug!("Nb store next vals for subset : {:?}",vals.len());
//            let cmds = vals.into_iter().map(|v|f(v)).collect();
 //           return Ok(GlobalReply::Mult(cmds));
            let cmd = f(vals);
            return Ok(cmd);
          },
          None => {
            panic!("KVStore use as backend does not allow subset");
          },
        }
      },
      KVStoreCommand::WithLocalValue(k,f) => {
        let oval = store.get_val(&k);
        let cmd = f(oval);
        return Ok(cmd);
      },

      KVStoreCommand::CommitStore => {
        if !store.commit_store() {
          panic!("Store commit failure, ending");
        }
      },
      KVStoreCommand::Store(qid,mut vs) => {

        debug!("A store kv command received with id {:?}",qid);
        let removereply = match query_cache.query_get_mut(&qid) {
         Some(query) => {
           match *query {
             Query(_, QReply::Local(_,ref nb_res,ref mut vres,_,ref qp), _) => {
               let (ds,cp) = self.dht_rules.do_store(true, qp.clone());
               if ds {
                 for v in vs.iter() {
                   store.add_val(v.borrow().clone(),cp);
                 }
               }
               vres.append(&mut vs);
               if *nb_res == vres.len() {
                 true
               } else {
                 return Ok(GlobalReply::NoRep);
               }
             },
             Query(_, QReply::Dist(ref old_mode_info,ref owith,nb_res,ref mut vres,_), _) => {
               // query prio dist to 0
               let (ds,cp) = self.dht_rules.do_store(false, 0);
               if ds {
                 for v in vs.iter() {
                   store.add_val(v.borrow().clone(),cp);
                 }
               }

               if !self.dht_rules.notfoundreply(&old_mode_info.get_mode()) {
                 // clone could be removed
                 let (odpr,odka,qid) = old_mode_info.clone().fwd_dests(&owith);
                 return Ok(GlobalReply::Forward(odpr,odka,FWConf{ nb_for : 0, discover : true },KVStoreCommand::Store(qid,vs)));
               } else {
                 vres.append(&mut vs);
                 if nb_res == vres.len() {
                   true
                 } else {
                   return Ok(GlobalReply::NoRep);
                 }
               }

             },
           }
         },
         None => {
           // TODO log probably timeout before
           return Ok(GlobalReply::NoRep);
         },
       };
       if removereply {
         let query = query_cache.query_remove(&qid).unwrap();
         match query.1 {
           QReply::Local(apiqid,_,vres,_,_) => {
             return Ok(GlobalReply::Api(KVStoreReply::FoundApiMult(vres, apiqid)));
           },
           QReply::Dist(old_mode_info,owith,_,vres,_) => {
             let (odpr,odka,qid) = old_mode_info.fwd_dests(&owith);
             return Ok(GlobalReply::Forward(odpr,odka,FWConf{ nb_for : 0, discover : true },KVStoreCommand::Store(qid,vres)));
           },
         }
       }

      },
      KVStoreCommand::StoreLocally(v,qprio,o_api_queryid) => {
        let (ds,cp) = self.dht_rules.do_store(true, qprio);
        if ds {
          // TODO new method on Ref trait (here double clone on Clone on send type!!
          store.add_val(v.borrow().clone(),cp);
        }
        if let Some(api_queryid) = o_api_queryid {
          return Ok(GlobalReply::Api(KVStoreReply::Done(api_queryid)));
        }
      },
      KVStoreCommand::NotFound(qid) => {
        debug!("A store not found command received with id : {:?}",qid);
        let remove = match query_cache.query_get_mut(&qid) {
         Some(query) => {
          match *query {
             Query(_, QReply::Local(_,_,_,ref mut nb_not_found,_), _) |
             Query(_, QReply::Dist(_,_,_,_,ref mut nb_not_found), _) => {
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
         let query = query_cache.query_remove(&qid).unwrap();
         match query.1 {
           QReply::Local(apiqid,_,vres,_,_) => {
             return Ok(GlobalReply::Api(KVStoreReply::FoundApiMult(vres, apiqid)));
           },
           QReply::Dist(old_mode_info,owith,_,vres,_) => {
             if vres.len() > 0 {
               let (odpr,odka,qid) = old_mode_info.fwd_dests(&owith);
               return Ok(GlobalReply::Forward(odpr,odka,FWConf{nb_for : 0, discover : true},KVStoreCommand::Store(qid,vres)));
             } else {
               if self.dht_rules.notfoundreply(&old_mode_info.get_mode()) {
                 let (odpr,odka,qid) = old_mode_info.fwd_dests(&owith);
                 return Ok(GlobalReply::Forward(odpr,odka,FWConf{nb_for : 0, discover : true},KVStoreCommand::NotFound(qid)));
               } else {
                 return Ok(GlobalReply::NoRep);
               }
             }
           },
         }
        }
        return Ok(GlobalReply::NoRep);
      },

      KVStoreCommand::Find(mut querymess, key,o_api_queryid) => {
        debug!("Find command received is_dist : {}",owith.is_some());
        // test here use o_api_query_id considering it implies next statement
        assert!(o_api_queryid.is_some() != owith.is_some());
        let oval = store.get_val(&key); 
        if oval.is_some() {
          querymess.nb_res -= 1;
        }
        // early exit when no need to forward
        if querymess.rem_hop == 0 || querymess.nb_res == 0 {
          if let Some(val) = oval {
            match o_api_queryid {
              Some(api_queryid) => {
                return Ok(GlobalReply::Api(KVStoreReply::FoundApi(Some(<VR as Ref<V>>::new(val)),api_queryid)));
              },
              None => {
                // reply
                let (odpr,odka,qid) = querymess.mode_info.fwd_dests(&owith);
                return Ok(GlobalReply::Forward(odpr,odka,FWConf{nb_for : 0, discover : true},KVStoreCommand::Store(qid,vec![<VR as Ref<V>>::new(val)])));
              },
            }
          }
          if self.dht_rules.notfoundreply(&querymess.mode_info.get_mode()) {
            match o_api_queryid {
              Some(api_queryid) => {
                return Ok(GlobalReply::Api(KVStoreReply::FoundApi(None,api_queryid)));
              },
              None => {
                let (odpr,odka,qid) = querymess.mode_info.fwd_dests(&owith);
                return Ok(GlobalReply::Forward(odpr,odka,FWConf{nb_for : 0, discover : true},KVStoreCommand::NotFound(qid)));
              },
            }
          } else {
            return Ok(GlobalReply::NoRep);
          }
        }
        //let do_store = querymess.mode_info.do_store() && querymess.rem_hop > 0;
        let (qid,st) = if o_api_queryid.is_some() || querymess.mode_info.do_store_on_forward() {
          (query_cache.new_id(),true)
        } else {
          (querymess.get_query_id(),false)
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
        let oval = oval.map(|val|<VR as Ref<V>>::new(val)); 
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

        if let Some(apiqid) = o_api_queryid {
          let query = Query(qid, QReply::Local(apiqid,querymess.nb_res,vres,nb_not_found,querymess.prio), Some(expire));
          query_cache.query_add(qid, query);
        } else {
          if st {
            // clone on owith could be removed
            let query = Query(qid, QReply::Dist(old_mode_info.clone(),owith.clone(),querymess.nb_res,vres,nb_not_found), Some(expire));
            query_cache.query_add(qid, query);
          }
        };
        if oval.is_some() && !do_reply_not_found {
          let (odpr,odka,oqid) = old_mode_info.fwd_dests(&owith);
          let found = GlobalReply::Forward(odpr,odka,FWConf{nb_for : 0, discover : true},KVStoreCommand::Store(oqid,vec![oval.unwrap()]));
          let pquery = GlobalReply::Forward(None,None,FWConf{ nb_for : querymess.nb_forw as usize, discover : self.discover},KVStoreCommand::Find(querymess,key,None));
          return Ok(GlobalReply::Mult(vec![found,pquery]));
        }
        return Ok(GlobalReply::Forward(None,None,FWConf{ nb_for : querymess.nb_forw as usize, discover : self.discover},KVStoreCommand::Find(querymess,key,None)));
      },
      KVStoreCommand::FindLocally(key,apiqueryid) => {
        let o_val = store.get_val(&key).map(|v|<VR as Ref<V>>::new(v));
        return Ok(GlobalReply::Api(KVStoreReply::FoundApi(o_val,apiqueryid)));
      },
    }
    Ok(GlobalReply::NoRep)
  }
}


// adapter to forward peer message into global dest
//pub struct OptPeerGlobalDest<MC : MyDHTConf> (pub GlobalDest<MC>);
pub struct OptPeerGlobalDest<MC : MyDHTConf> {
  pub mainloop : MainLoopSendIn<MC>,
  pub api : Option<ApiHandleSend<MC>>,
  // issue with recursive resolution of send for local (evaluating trait recurse on Send
  // constraint)
  // For now use a Fat Pointer, which is maybe a good idea to leverage the initialization
  // difficulty
  pub globalservice : Option<Box<SpawnSendWithHandle<GlobalCommand<MC::PeerRef,MC::GlobalServiceCommand>> + Send>>,
  pub globalservice_build : (bool,Arc<Mutex<Option<Box<SpawnSendWithHandle<GlobalCommand<MC::PeerRef,MC::GlobalServiceCommand>> + Send>>>>),
//  pub globalservice : Option<GlobalHandleSend<MC>>,
}

impl<MC : MyDHTConf> SRef for OptPeerGlobalDest<MC> where
  MainLoopSendIn<MC> : Send,
  ApiWeakSend<MC> : Send,
  ApiWeakUnyield<MC> : Send,
//  GlobalWeakSend<MC> : Send,
//  GlobalWeakUnyield<MC> : Send,
  {
  type Send = Self;
  #[inline]
  fn get_sendable(self) -> Self::Send {
    self
  }
}

impl<MC : MyDHTConf> SToRef<OptPeerGlobalDest<MC>> for OptPeerGlobalDest<MC> where
  MainLoopSendIn<MC> : Send,
  ApiWeakSend<MC> : Send,
  ApiWeakUnyield<MC> : Send,
//  GlobalWeakSend<MC> : Send,
//  GlobalWeakUnyield<MC> : Send,
  {
  #[inline]
  fn to_ref(self) -> OptPeerGlobalDest<MC> {
    self
  }
}




impl<MC : MyDHTConf> SpawnSend<GlobalReply<MC::Peer,MC::PeerRef,KVStoreCommand<MC::Peer,MC::PeerRef,MC::Peer,MC::PeerRef>,KVStoreReply<MC::PeerRef>>> for OptPeerGlobalDest<MC> {
  const CAN_SEND : bool = true;
  fn send(&mut self, r : GlobalReply<MC::Peer,MC::PeerRef,KVStoreCommand<MC::Peer,MC::PeerRef,MC::Peer,MC::PeerRef>,KVStoreReply<MC::PeerRef>>) -> Result<()> {
    match r {
      GlobalReply::Mult(cmds) => {
        for cmd in cmds.into_iter() {
          self.send(cmd)?;
        }
      },
      GlobalReply::MainLoop(mlc) => {
        self.mainloop.send(MainLoopCommand::SubCommand(mlc))?;
      },
      GlobalReply::PeerApi(c) => {
        // TODO weak send macro of inline function!!!
        if c.get_api_reply().is_some() {
          let cml =  match self.api {
            Some(ref mut api_weak) => {
              api_weak.send_with_handle(ApiCommand::ServiceReply(MCReply::PeerStore(c)))?.map(|c|
                  if let ApiCommand::ServiceReply(MCReply::PeerStore(c)) = c {c} else {unreachable!()})
            },
            None => {
              Some(c)
            },
          };
          if let Some(c) = cml {
            self.api = None;
            self.mainloop.send(MainLoopCommand::ProxyApiReply(MCReply::PeerStore(c)))?;
          }
        }
      },
      GlobalReply::Api(c) => {
        self.send(GlobalReply::PeerApi(c))?;
      },
      GlobalReply::PeerForward(opr,okad,nb_for,gsc) => {
        self.mainloop.send(MainLoopCommand::ForwardService(opr,okad,nb_for,MCCommand::PeerStore(gsc)))?;
      },
      GlobalReply::Forward(opr,okad,nb_for,gsc) => {
        self.send(GlobalReply::PeerForward(opr,okad,nb_for,gsc))?;
      },
      GlobalReply::ForwardOnce(ok,oad,fwconf,gsc) => {
        // never use here (storeprop uses standard forward)
        unreachable!()
      },
      GlobalReply::PeerStore(..) => {
        unreachable!()
      },
      GlobalReply::GlobalSendPeer(op) => {
        // lazy init
        if self.globalservice_build.0 == true {
          if self.globalservice_build.0 {
          if let Ok(mut g) = self.globalservice_build.1.lock() {
            if g.is_some() {
              self.globalservice = replace(&mut g,None);
              self.globalservice_build.0 = false;
            }
          }
          }
        }
        let o_listen_global = <MC::GlobalServiceCommand as PeerStatusListener<MC::PeerRef>>::build_command(PeerStatusCommand::PeerQuery(op));
        if let Some(k) = o_listen_global {
          let od = match self.globalservice {
            Some(ref mut ps_weak) => {
                ps_weak.send_with_handle(GlobalCommand::Local(k))?.map(|c|
                    if let GlobalCommand::Local(k) = c {k} else {unreachable!()})
            },
            None => {
              Some(k)
            },
          };
          if let Some(k) = od {
           self.api = None;
           self.mainloop.send(MainLoopCommand::ProxyGlobal(GlobalCommand::Local(k)))?;
          }
        } else {
          warn!("Peer query for global was issued but global command does not allow it (see PeerStatusListener implementation for this command)");
        }
      },
      GlobalReply::NoRep => (),
    }
    Ok(())
  }
}

 
/// peer in kvstore is trusted and in case of noauth mode it could be forward to global service (if
/// auth mode peer_ping will forward it after auth (needed for example in default no auth mydht
/// tunnel).
pub fn trusted_peer_ping<P : Peer,PR>(v : Vec<P>) -> GlobalReply<P,PR,KVStoreCommand<P,PR,P,PR>,KVStoreReply<PR>> {
  if v.len() == 0 {
    GlobalReply::NoRep
  } else {
    let cmds = v.into_iter().map(|p|GlobalReply::MainLoop(MainLoopSubCommand::TrustedTryConnect(p.clone()))).collect();
    GlobalReply::Mult(cmds)
  }
}

pub fn peer_ping<P : Peer,PR>(v : Vec<P>) -> GlobalReply<P,PR,KVStoreCommand<P,PR,P,PR>,KVStoreReply<PR>> {
  if v.len() == 0 {
    GlobalReply::NoRep
  } else {
    let cmds = v.into_iter().map(|p|GlobalReply::MainLoop(MainLoopSubCommand::TryConnect(p.get_key(),p.get_address().clone()))).collect();
    GlobalReply::Mult(cmds)
  }
}
pub fn peer_discover<P : Peer,PR>(v : Vec<P>) -> GlobalReply<P,PR,KVStoreCommand<P,PR,P,PR>,KVStoreReply<PR>> {
  if v.len() == 0 {
    GlobalReply::NoRep
  } else {
    let dests = v.into_iter().map(|p|(p.get_key(),p.get_address().clone())).collect();
    GlobalReply::MainLoop(MainLoopSubCommand::Discover(dests))
  }
}
#[inline]
pub fn peer_forward_ka<P : Peer,PR>(op : Option<P>) -> GlobalReply<P,PR,KVStoreCommand<P,PR,P,PR>,KVStoreReply<PR>> {
  GlobalReply::MainLoop(MainLoopSubCommand::FoundAddressForKey(op.map(|p|p.get_address().clone())))
}

#[inline]
pub fn send_user_to_global<P : Peer,RP : Ref<P>> (res_q_peer : Option<P>) -> GlobalReply<P,RP,KVStoreCommand<P,RP,P,RP>,KVStoreReply<RP>> {
  GlobalReply::GlobalSendPeer(res_q_peer.map(|p|<RP as Ref<P>>::new(p)))
}

/*pub fn peer_discover<MC : MyDHTConf>(v : Vec<MC::Peer>) -> GlobalReply<MC::Peer,MC::PeerRef,KVStoreCommand<MC::Peer,MC::PeerRef,MC::Peer,MC::PeerRef>,KVStoreReply<MC::PeerRef>> {
  GlobalReply::ForwardMainLoop(MainLoopSubCommand::TryConnect(p.get_key(),p.get_address().clone()))).collect();
}*/



