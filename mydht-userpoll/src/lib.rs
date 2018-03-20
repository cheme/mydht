extern crate mydht_base;


use mydht_base::transport::{
  Token,
  Ready,
  Registerable,
  TriggerReady,
  Poll,
  Events,
  Event,
  
};

use mydht_base::service::{
};

use mydht_base::mydhtresult::{
  Result,
};

use std::time::Duration;
use std::cmp::Ordering;
use std::collections::BTreeMap;

pub struct UserPoll {
  // rbt (slab allocated) containing fd of file to poll -> using std::collection::BTreeMap probably over fD for now
  items : BTreeMap<FD,UserItem>,
}

/// info register in poll for something to poll
/// TODO compare on fd
struct UserItem {
  fd : FD,
  // Topics to listen to
  topics : Topics,
}
impl Ord for UserItem {
  fn cmp(&self, other: &Self) -> Ordering {
    unimplemented!()
  }
}
impl PartialOrd<UserItem> for UserItem {
  fn partial_cmp(&self, other: &UserItem) -> Option<Ordering> {
    unimplemented!()
  }
}
impl Eq for UserItem {
}
impl PartialEq<UserItem> for UserItem {
  fn eq(&self, other: &UserItem) -> bool {
    unimplemented!()
  }
}
impl Poll for UserPoll {
  type Events = UserEvents;
  fn poll(&self, events: &mut Self::Events, timeout: Option<Duration>) -> Result<usize> {
    unimplemented!()
  }
}

impl UserPoll {
  // TODO could use service Yield, but means that we need to unyield accordingly from any call to
  // setreadiness (kind encumber that : mio impl suspend thread which is certainly similar as threadblock service
  // our use case is a js worker as service spawner and is not for initial use : TODO plug a some
  // similar code as threadblock in a wait function that we will pragma with something else for js
  // latter and even latter integrate with the current (mainloop) service yield)
  fn new() -> Self {
    unimplemented!()
  }
  fn ctl_add<E : UserEventable>(&mut self, e : &E) -> Result<()> {
    unimplemented!()
      // TODO first check xisting on current E FD (if not undefined) if xisting : errno -> should
      // be simplier depending if already register : succes or not : attribut fd so no err?
      // still err on no more fd


      // poll file on insert, TODO use a queue bef act insert?
  }
}

pub struct UserEvents {
  // pbably array of UserSetReadiness
  // copied from the poll Queue (need lock if multith, here currently monoth)
  // Note that if Ready are multiple on topics (guess not) we could have more events than capacity.
}

impl Events for UserEvents {
  fn with_capacity(s : usize) -> Self {
    unimplemented!()
  }
}

impl Iterator for UserEvents {
  type Item = Event;
  fn next(&mut self) -> Option<Self::Item> {
    unimplemented!()
  }
}

pub type FD = usize;

const FD_UNDEFINED : FD = 0;

#[derive(Clone)]
// currently only two Ready state so u8 totally enough
pub struct Topics(u8);

impl Topics {
  #[inline]
  fn add(&mut self,t : Ready) {
    unimplemented!()
  }
  #[inline]
  fn contains(&self,t : Ready) -> bool {
    unimplemented!()
  }

  #[inline]
  fn remove(&mut self, t : Ready) {
    unimplemented!()
  }
  #[inline]
  fn empty() {
    Topics(0);
  }
}

/// Trait to implement in order to join this kind of loop
pub trait UserEventable {
  /// when joining get the internal descriptor that userpoll gave us
  fn put_fd(&mut self, FD);
  fn get_fd(&self) -> FD;

  /// change event state (note that we only look at one event : pragmatic limitation)
  /// looks like some costy rc cell usage to come (just a get mut to it would be better interface
  /// if that is the case).
  fn event(&mut self, e : Ready);
  fn poll_event(&mut self) -> Topics;
}

#[derive(Clone)]
pub struct UserSetReadiness {
  fd : FD,
  state : Topics,// useless ?
}

impl UserEventable for UserSetReadiness {
  #[inline]
  fn put_fd(&mut self, fd : FD) {
    self.fd = fd
  }
  #[inline]
  fn get_fd(&self) -> FD {
    self.fd
  }
  #[inline]
  fn event(&mut self, e : Ready) {
    unimplemented!()
  }
  #[inline]
  fn poll_event(&mut self) -> Topics {
    unimplemented!()
    // TODO put state to init
  }

}

impl TriggerReady for UserSetReadiness {
  fn set_readiness(&self, ready: Ready) -> Result<()> {
    unimplemented!()
  }
}
