extern crate mydht_base;

use std::usize::MAX as MAX_USIZE;
use mydht_base::transport::{
  Token,
  Ready,
  Registerable,
  TriggerReady,
  Poll,
  Events,
  Event,
  
};


use mydht_base::mydhtresult::{
  Result,
  Error,
  ErrorKind,
};

use std::time::Duration;
//use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::cell::RefCell;
use std::rc::Rc;

#[cfg(test)]
mod test;

// we do not want to manage a file descriptor pool so we got unique fd for userpoll to use in
// constructor. If two userpoll are build upon identic fd, the registration of a registrable will
// fail
#[derive(Clone)]
pub struct UserPoll(Rc<RefCell<UserPollInner>>,FD);

struct UserPollInner {
  // rbt (slab allocated) containing fd of file to poll -> using std::collection::BTreeMap probably over fD for now
  // A nice operation would be add useritem and return first FD available (then init fd in user
  // item)
  items : BTreeMap<FD,UserItem>,
  ev_queue : VecDeque<Event>,
  last_new_fd : FD,
}

/// info register in poll for something to poll
struct UserItem {
  token : FD,
  // Topics to listen to
  topics : Topics,
}
/*impl Ord for UserItem {
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
}*/
impl Poll for UserPoll {
  type Events = UserEvents;
  fn poll(&self, events: &mut Self::Events, _timeout: Option<Duration>) -> Result<usize> {
    let queue = &mut self.0.borrow_mut().ev_queue;
    let ql = queue.len();
    let nb_mov = std::cmp::max(events.size_limit, ql);
    events.content = queue.split_off(ql - nb_mov);
    Ok(events.content.len())
  }
}
impl UserPoll {
  // TODO could use service Yield, but means that we need to unyield accordingly from any call to
  // setreadiness (kind encumber that : mio impl suspend thread which is certainly similar as threadblock service
  // our use case is a js worker as service spawner and is not for initial use : TODO plug a some
  // similar code as threadblock in a wait function that we will pragma with something else for js
  // latter and even latter integrate with the current (mainloop) service yield)
  pub fn new(fd : FD) -> Self {
    UserPoll(Rc::new(RefCell::new(
      UserPollInner {
        items : BTreeMap::new(),
        ev_queue : VecDeque::new(), // TODO capacity init??
        last_new_fd : FD_UNDEFINED,
    })),fd)
  }
  fn ctl_add<E : UserEventable>(&self, e : &E, t : Token, r : Ready) -> Result<()> {
    let xistingfd = e.get_fd(self.1);
    if xistingfd == FD_UNDEFINED {
      // new fd
      let newfd = self.new_fd(t,r)?;
      e.put_fd(self.1, newfd, self);
    } else {
      self.0.borrow_mut().items.get_mut(&xistingfd).unwrap().topics.add(r);
    }
    Ok(())
  }
  fn new_fd(&self, token : Token, r : Ready) -> Result<usize> {
    let mut topics = Topics::empty();
    topics.add(r);
    let mut new_fd = self.0.borrow().last_new_fd.clone();
    let mut i = 0;
    while {
      new_fd = inc_l(new_fd);
      i += 1;
      if i == MAX_USIZE {
        return Err(
          Error("Full poll : reach max fd for this poll".to_string(), ErrorKind::IOError, None))
      }
      self.0.borrow().items.get(&new_fd).is_some()
    } {}
    self.0.borrow_mut().last_new_fd = new_fd;
    let ui = UserItem {
      token,
      topics,
    };

    self.0.borrow_mut().items.insert(new_fd,ui);
    Ok(new_fd)

  }
  fn ctl_rem<E : UserEventable>(&self, e : &E) -> Result<()> {
    let fdp = e.get_fd(self.1).clone();
    e.rem_fd(self.1);
    self.0.borrow_mut().items.remove(&fdp);
    Ok(())
  }
}
impl UserPollInner {
  fn trigger_event(&mut self, fd_eventable : &FD, ready : Ready) -> Result<()> {
    if let Some(item) = self.items.get(fd_eventable) {
      // check reg :
      if item.topics.contains(ready) {
        self.ev_queue.push_front(Event {
          kind : ready,
          token : item.token,
        })
      }
    }
    Ok(())

  }
}


#[inline]
fn inc_l(v : usize) -> usize {
  if v == MAX_USIZE {
    1
  } else {
    v + 1
  }
}
pub struct UserEvents {
  size_limit : usize,
  // pbably array of UserSetReadiness
  // copied from the poll Queue (need lock if multith, here currently monoth)
  // Note that if Ready are multiple on topics (guess not) we could have more events than capacity.
  content : VecDeque<Event>,
}

impl Events for UserEvents {
  fn with_capacity(s : usize) -> Self {
    UserEvents {
      size_limit : s,
      // no with capacity as its copyied from a split_off (could be optional) 
      content : VecDeque::new()

    }
  }
}

impl Iterator for UserEvents {
  type Item = Event;
  fn next(&mut self) -> Option<Self::Item> {
    self.content.pop_back()
  }
}

pub type FD = usize;

const FD_UNDEFINED : FD = 0;

#[derive(Clone)]
// currently only two Ready state so u8 totally enough
pub struct Topics(u8);

impl Topics {
  #[inline]
  pub fn add(&mut self,t : Ready) {
    match t {
      Ready::Readable => self.0 |= 1,
      Ready::Writable => self.0 |= 2,
    }
  }
  #[inline]
  pub fn contains(&self,t : Ready) -> bool {
    match t {
      Ready::Readable => (self.0 | 1 == self.0),
      Ready::Writable => (self.0 | 2 == self.0),
    }
  }

  #[inline]
  pub fn remove(&mut self, t : Ready) {
    match t {
      Ready::Readable => self.0 &= !1,
      Ready::Writable => self.0 &= !2,
    }
  }
  #[inline]
  pub fn empty() -> Topics {
    Topics(0)
  }
}

#[test]
fn topics_test () {
  let mut top = Topics::empty();
  assert!(!top.contains(Ready::Readable));
  assert!(!top.contains(Ready::Writable));
  top.remove(Ready::Writable);
  assert!(!top.contains(Ready::Readable));
  assert!(!top.contains(Ready::Writable));
  top.add(Ready::Writable);
  assert!(top.contains(Ready::Writable));
  assert!(!top.contains(Ready::Readable));
  top.add(Ready::Readable);
  assert!(top.contains(Ready::Writable));
  assert!(top.contains(Ready::Readable));
  top.remove(Ready::Writable);
  assert!(!top.contains(Ready::Writable));
  assert!(top.contains(Ready::Readable));
  top.remove(Ready::Readable);
  assert!(!top.contains(Ready::Writable));
  assert!(!top.contains(Ready::Readable));
}
/// Trait to implement in order to join this kind of loop
pub trait UserEventable {
  /// when joining get the internal descriptor that userpoll gave us
  fn put_fd(&self, FD, FD, &UserPoll);
  //fn put_fd(&mut self, FD);
  fn get_fd(&self, FD) -> FD;
  fn rem_fd(&self, FD);

}

pub type UserRegistration = Rc<RefCell<UserRegistrationInner>>;
#[derive(Clone)]
pub struct UserRegistrationInner {
  // TODO Btree map bad check operation when finish and switch (likely to be generally small)
  // plus first fd (userpoll fd seems unused
  fds : BTreeMap<FD,(FD,Rc<RefCell<UserPollInner>>)>,
}
// TODO change to single struct
#[derive(Clone)]
pub struct UserSetReadiness {
  // link to userregistration
  reg : UserRegistration,
}

pub fn poll_reg () -> (UserSetReadiness, UserRegistration) {
  let ur = Rc::new(RefCell::new(UserRegistrationInner {
    fds : BTreeMap::new(),
  }));
  (UserSetReadiness {
    reg : ur.clone(),
  }, ur)
}
impl UserEventable for UserRegistration {
  #[inline]
  fn put_fd(&self, fd_poll : FD, fd_self : FD, poll : &UserPoll) {
    self.borrow_mut().fds.insert(fd_poll,(fd_self,poll.0.clone()));
  }
  #[inline]
  fn get_fd(&self,fd_poll : FD) -> FD {
    self.borrow().fds.get(&fd_poll).map(|a|a.0.clone()).unwrap_or(FD_UNDEFINED)
  }
  #[inline]
  fn rem_fd(&self, fd_poll : FD) {
    self.borrow_mut().fds.remove(&fd_poll);
  }

}


impl Registerable<UserPoll> for UserRegistration {
  fn register(&self, poll : &UserPoll, t : Token, r : Ready) -> Result<bool> {
    poll.ctl_add(self, t, r)?;
    Ok(true)
  }
  fn reregister(&self, poll : &UserPoll, t : Token, r : Ready) -> Result<bool> {
    poll.ctl_add(self, t, r)?;
    Ok(true)
  }

  fn deregister(&self, poll: &UserPoll) -> Result<()> {
    poll.ctl_rem(self)
  }


}
/*impl Registerable<UserPoll> for UserEventable {
  fn register(&self, poll : &UserPoll, t : Token, r : Ready) -> Result<bool> {
    unimplemented!()
  }
  fn reregister(&self, poll : &UserPoll, t : Token, r : Ready) -> Result<bool> {
    unimplemented!()
  }

  fn deregister(&self, poll: &UserPoll) -> Result<()> {
    unimplemented!()
  }

}*/
impl TriggerReady for UserSetReadiness {
  fn set_readiness(&self, ready: Ready) -> Result<()> {
    for (_fd_poll,&(ref fd_self,ref poll_inner)) in self.reg.borrow().fds.iter() {
      poll_inner.borrow_mut().trigger_event(fd_self,ready)?;
    }
    Ok(())
  }
}


