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
  Error,
  ErrorKind,
};

use std::time::Duration;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::cell::RefCell;
use std::rc::Rc;

#[cfg(test)]
mod test;

pub struct UserPoll {
  // rbt (slab allocated) containing fd of file to poll -> using std::collection::BTreeMap probably over fD for now
  // TODO change Registerable to use &mut Poll and remove this RefCell!!
  // A nice operation would be add useritem and return first FD available (then init fd in user
  // item)
  items : RefCell<BTreeMap<FD,UserItem>>,
  // we do not want to manage a file descriptor pool so we got unique fd for userpoll to use in
  // constructor. If two userpoll are build upon identic fd, the registration of a registrable will
  // fail
  fd : FD,
  // TODO change if to rem ref cell or if not possible put in items refcell
  last_new_fd : RefCell<FD>,
}

/// info register in poll for something to poll
/// TODO compare on fd
struct UserItem {
  fd : FD, // TODO remove??
  token : FD,
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
  pub fn new(fd : FD) -> Self {
    UserPoll {
      items : RefCell::new(BTreeMap::new()),
      fd,
      last_new_fd : RefCell::new(FD_UNDEFINED),
    }
  }
  fn ctl_add<E : UserEventable>(&self, e : &E, t : Token, r : Ready) -> Result<()> {
    let mut xistingfd = e.get_fd(self.fd);
    if xistingfd == FD_UNDEFINED {
      // new fd
      let newfd = self.new_fd(t,r)?;
      e.put_fd(self.fd, newfd);
    } else {
      self.items.borrow_mut().get_mut(&xistingfd).unwrap().topics.add(r);
    }
    Ok(())
  }
  fn new_fd(&self, token : Token, r : Ready) -> Result<usize> {
    let mut topics = Topics::empty();
    topics.add(r);
    let mut new_fd = self.last_new_fd.borrow().clone();
    let mut i = 0;
    while {
      new_fd = inc_l(new_fd);
      i += 1; // panic on full by usize overflow TODO if i == max return full error to avoid optim
      if i == 10000 {// TODO rep max value
        return Err(
          Error("Full poll : reach max fd for this poll".to_string(), ErrorKind::IOError, None))
      }
      self.items.borrow().get(&new_fd).is_some()
    } {}
    *self.last_new_fd.borrow_mut() = new_fd;
    let ui = UserItem {
      fd : new_fd,
      token,
      topics,
    };

    self.items.borrow_mut().insert(new_fd,ui);
    Ok(new_fd)

  }
  fn ctl_rem<E : UserEventable>(&self, e : &E) -> Result<()> {
    let fdp = e.get_fd(self.fd).clone();
    e.rem_fd(self.fd);
    self.items.borrow_mut().remove(&fdp);
    Ok(())
  }
}
#[inline]
fn inc_l(v : usize) -> usize {
  if v == 10000 { // TODO find usize max when internet conn
    1
  } else {
    v + 1
  }
}
pub struct UserEvents {
  // pbably array of UserSetReadiness
  // copied from the poll Queue (need lock if multith, here currently monoth)
  // Note that if Ready are multiple on topics (guess not) we could have more events than capacity.
  content : Vec<Event>,
}

impl Events for UserEvents {
  fn with_capacity(s : usize) -> Self {
    UserEvents {
      content : Vec::with_capacity(s)
    }
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
    match t {
      Ready::Readable => self.0 |= 1,
      Ready::Writable => self.0 |= 2,
    }
  }
  #[inline]
  fn contains(&self,t : Ready) -> bool {
    match t {
      Ready::Readable => (self.0 | 1 == self.0),
      Ready::Writable => (self.0 | 2 == self.0),
    }
  }

  #[inline]
  fn remove(&mut self, t : Ready) {
    match t {
      Ready::Readable => self.0 &= !1,
      Ready::Writable => self.0 &= !2,
    }
  }
  #[inline]
  fn empty() -> Topics {
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
  fn put_fd(&self, FD, FD);
  //fn put_fd(&mut self, FD);
  fn get_fd(&self, FD) -> FD;
  fn rem_fd(&self, FD);

  /// change event state (note that we only look at one event : pragmatic limitation)
  /// looks like some costy rc cell usage to come (just a get mut to it would be better interface
  /// if that is the case).
  fn event(&mut self, e : Ready);
  fn poll_event(&mut self) -> Topics;
}

pub type UserRegistration = Rc<RefCell<UserRegistrationInner>>;
#[derive(Clone)]
pub struct UserRegistrationInner {
  // TODO Btree map bad
  fds : BTreeMap<FD,FD>,
}
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
  fn put_fd(&self, fd_poll : FD, fd_self : FD) {
    self.borrow_mut().fds.insert(fd_poll,fd_self);
  }
  #[inline]
  fn get_fd(&self,fd_poll : FD) -> FD {
    self.borrow().fds.get(&fd_poll).unwrap_or(&FD_UNDEFINED).clone()
  }
  #[inline]
  fn rem_fd(&self, fd_poll : FD) {
    self.borrow_mut().fds.remove(&fd_poll);
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
//    if self.reg.borrow(). TODO iter on registration and post ready to poll : TODO ref to poll to
//    add on register -> pbbly also some RcRefCell
    unimplemented!()
  }
}


