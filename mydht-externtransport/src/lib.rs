// TODO giveup on generic approach and be webrtc sepcific ?
#![feature(type_ascription)]
extern crate slab;
extern crate mydht_base;
extern crate mydht_userpoll;
extern crate serde;
#[macro_use] extern crate serde_derive;

use std::slice;
use std::cell::RefCell;
use std::cell::Cell;
use slab::Slab;
use std::collections::VecDeque;
use std::borrow::Borrow;
use std::ffi::{
  CStr,
  CString,
};
use std::mem;
use std::io::Write;
use std::io::Read;
use std::io::Result as IoResult;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::rc::Rc;
use mydht_base::mydhtresult::{
  Result,
  Error,
  ErrorKind,
};

use mydht_base::utils::{
  Proto,
};
use mydht_base::service::{
  Service,
  ServiceRestartable,
  Spawner,
  SpawnerYield,
  RestartOrError,
  RestartSameThread,
  NoSend,
  DefaultRecv,
  YieldReturn,
  NoRecv,
  LocalRc,
  SpawnChannel,
  LocalRcChannel,
};
use mydht_base::transport::{
  Token,
  Ready,
  Registerable,
  TriggerReady,
  Poll,
  Events,
  Event,
  WriteTransportStream,
  ReadTransportStream,
  Transport,
  Address,
};

use mydht_userpoll::{
  UserPoll,
  UserEvents,
  UserRegistration,
  UserSetReadiness,
  poll_reg,
};

use std::os::raw::{
  c_char,
  c_void,
};
use std::marker::PhantomData;

#[cfg(feature = "jstest")]
#[derive(Clone,Debug)]
pub enum TestCommand {
  ConnectWith(Vec<u8>),
  None,
}

impl Proto for TestCommand {
  fn get_new(&self) -> Self { self.clone() }
}

#[cfg(feature = "jstest")]
pub type LocalRcC = LocalRc<TestCommand>;
#[cfg(feature = "jstest")]
pub type RecMain = DefaultRecv<TestCommand,LocalRcC>;
// reexport on main when targetting wasm seems ko so using a macro
#[macro_export]
macro_rules! extern_func (() => {
use std::mem;
use std::slice;
use mydht_base::transport::{
  Ready,
  TriggerReady,
};
use mydht_base::service::{
  Service,
  ServiceRestartable,
  Spawner,
  SpawnerYield,
  RestartOrError,
  RestartSameThread,
  NoSend,
  DefaultRecv,
  YieldReturn,
  NoRecv,
  SpawnUnyield,
  SpawnSend,
};
use mydht_userpoll::{ 
  UserSetReadiness,
};
use std::rc::Rc;
use std::cell::RefCell;
use std::cell::Cell;
use std::cell::Ref;
use std::borrow::BorrowMut;
use std::borrow::Borrow;

#[cfg(feature = "jstest")]
#[no_mangle]
pub extern "C" fn restore(handle : *mut c_void) {
  let mut typed_handle =
   unsafe { Box::from_raw(
      handle as *mut RestartSameThread<TestService, NoSend, RestartOrError, RecMain>
    )};
  typed_handle.unyield().unwrap(); // TODO correct erro mgmt with a test if is_finished before
  mem::forget(typed_handle);
}
/*alternate impl for alloc dealloc kept for testing : issue when using cipher
 * : memory corruption : TODO try to identify -> only after deciphering successfully a pic and
 * changing password (not using dalloc do no solve it)
#[no_mangle]
pub extern "C" fn alloc(size: usize) -> *mut u8 {
  unsafe {
    let layout = Layout::from_size_align(size, mem::align_of::<u8>()).unwrap();
    Heap.alloc(layout).unwrap()
  }
}

#[no_mangle]
pub extern "C" fn dealloc(ptr: *mut u8, size: usize) {
  unsafe  {
    let layout = Layout::from_size_align(size, mem::align_of::<u8>()).unwrap();
    Heap.dealloc(ptr, layout);
  }
}
*/
/* 
*/
#[no_mangle]
pub extern "C" fn alloc(size: usize) -> *mut c_void {
  let mut buf = Vec::with_capacity(size);
  let ptr = buf.as_mut_ptr();
  mem::forget(buf);
  return ptr as *mut c_void;
}

#[no_mangle]
pub extern "C" fn dealloc(ptr: *mut c_void, cap: usize) {
  unsafe  {
    let _buf = Vec::from_raw_parts(ptr, 0, cap);
  }
}
#[cfg(feature = "jstest")]
#[no_mangle]
pub extern "C" fn start(id : *mut u8, idlen : usize) {
 start_service(id,idlen);
}

#[cfg(feature = "jstest")]
#[no_mangle]
pub extern "C" fn test_connect_to(sender : *mut c_void, id : *mut u8, idlen : usize) {
  let destid = unsafe {
    slice::from_raw_parts(id, idlen)
  }.to_vec();

  log_js(format!("connect to send : {:?}", destid), LogType::Log);
  let mut sendchan =
   unsafe { Box::from_raw(
      sender as *mut LocalRcC
    )};
  sendchan.send(TestCommand::ConnectWith(destid.to_vec())).unwrap();
  mem::forget(sendchan);
}



/// reply to a query_listener call
/// Note that this function should be redefine
#[no_mangle]
pub extern "C" fn start_with_listener(_id : *mut u8, _idlen : usize, _listener_set_readiness : *mut c_void) -> *const u8 {
  unimplemented!("a start depending on our tests : should move in main.rs")
}


/// TODO seems pretty useless (js side)
#[no_mangle]
pub extern "C" fn transport_ready(tr_trigger : *mut c_void, tr_state : *mut c_void) {
  change_state_and_trigger(tr_trigger,tr_state, TransportState::Connected);
}

#[no_mangle]
pub extern "C" fn connect_success(chan_trigger : *mut c_void, chan_state : *mut c_void, chan_counter : *mut c_void, chan_id : u16) {
  // set counter
  let t_counter : ChannelId = unsafe { Box::from_raw(chan_counter as *mut Rc<Cell<u16>>) };
  {
     let t_href : &Cell<u16> = (*t_counter).borrow();
     t_href.set(chan_id);
  }
  mem::forget(t_counter);
  // set state and trigger ready read and write
  change_state_and_trigger(chan_trigger,chan_state,TransportState::Connected);
}

#[no_mangle]
pub extern "C" fn connect_fail(chan_trigger : *mut c_void, chan_state : *mut c_void) {
  // trigger ready read and write to error the next read / write (stuck otherwhise)
  change_state_and_trigger(chan_trigger,chan_state,TransportState::CouldNotConnect);
}
#[no_mangle]
pub extern "C" fn connect_close(chan_trigger : *mut c_void, chan_state : *mut c_void) {
  log_js("connect closed".to_string(), LogType::Log);
  // trigger ready read and write to error the next read / write (stuck otherwhise)
  change_state_and_trigger(chan_trigger,chan_state,TransportState::Closed);
}


fn change_state_and_trigger(trigger : *mut c_void, state : *mut c_void, new_state : TransportState) {
  // set state
  let t_state : TState = unsafe { Box::from_raw(state as *mut Rc<Cell<TransportState>>) };
  {
     let t_href : &Cell<TransportState> = (*t_state).borrow();
     t_href.set(new_state);
  }
  mem::forget(t_state);
  // trigger ready read and write to error the next read / write (stuck otherwhise)
  let t_trig = unsafe { Box::from_raw(trigger as *mut UserSetReadiness) };
  t_trig.set_readiness(Ready::Readable);
  t_trig.set_readiness(Ready::Writable);

  mem::forget(t_trig);
}

#[no_mangle]
pub extern "C" fn receive_connect(trigger : *mut c_void) {
  let t_trig = unsafe { Box::from_raw(trigger as *mut UserSetReadiness) };
  t_trig.set_readiness(Ready::Readable);

  mem::forget(t_trig);
}

#[no_mangle]
pub extern "C" fn write_success(_id : *mut u8, _idlen : usize, _write_set_readiness : *mut c_void, _chan_pt : *mut c_void) {
  unimplemented!("set ready")
}
#[no_mangle]
pub extern "C" fn write_error(_id : *mut u8, _idlen : usize, _write_set_readiness : *mut c_void, _chan_pt : *mut c_void) {
  unimplemented!("set ready + change state to error")
}


#[no_mangle]
pub extern "C" fn trigger_read(_set_readiness : *mut c_void, _eventlooptoken : usize) {
  unimplemented!()
}
#[no_mangle]
pub extern "C" fn forget_readiness(_set_readiness : *mut c_void) {
  unimplemented!("rc -1 on the poll by dropping object");
}

#[no_mangle]
pub extern "C" fn trigger_write(_set_readiness : *mut c_void, _eventlooptoken : usize) {
  unimplemented!("TODO cast to UserSetReadiness and trigger")
}

});

extern {

  pub fn wasm_log(_ : *const c_char, _ : LogType);
  /// ask for a listener. In our webrtc case it is an asynchronous process were we connect through
  /// websocket to the user server, publishing our connection id.
  /// token will be use latter for registration
  /// TODO if challenge with websocket server will need more parameters (currently no challenge and
  /// unsafe registering)
  pub fn query_listener(_send_channel : *mut c_void, _trig_transport : *mut c_void, _handle : *mut c_void, _status : *mut u8, _id : *mut u8, _idlen : usize);
//  pub fn yield_loop(_transport : *mut c_void);
  /// this should only be called when there is a ready event on the poll, it returns the port for the channel
  /// only first param is for input
  /// others params are for output
  /// return true if there is next, false otherwhise
  pub fn next_pending_connected(_send_channel : *mut c_void, _id_out : *mut *mut u8, _id_len_out : *mut usize, _chan_count : *mut u16) -> bool;

  /// connect to dest
  /// TODO callback on a set_ready with channel id see 'connect_success' and 'connect_fail' (chan
  /// pointer is for call back and to identify the chan at a js level)
  pub fn query_new_channel(_transport_id : *mut c_void, _dest_id : *mut u8, _idlen : usize, _chan_trigger : *mut c_void, _chan_state : *mut c_void, _chan_counter : *mut c_void);
  /// when rust go a new channel he send it back for set_readiness (here we do not build a
  /// connection, it already xist : we simply send the channel pointer
  pub fn read_channel_reg(_transport_id : *mut c_void, _dest_id : *mut u8, _idlen : usize, _chan_id : u16, _chan_trigger : *mut c_void, _chan_state : *mut c_void);
  // no call back here at some point need an association table between pointer chan and actuall
  // chanel
  pub fn close_channel(_chan : *mut c_void);

  /// TODO add returned possible error code
  /// call back with a set write ready when content send ( when buff full we return 0 from write
  /// meaning that rust should io not ready error on it)
  pub fn write(_chan : *mut c_void, _content : *mut u8, _contentlen : usize) -> usize;
  /// return possible error code as call back (same as write), flush the buffer!!
  pub fn flush_write(_chan : *mut c_void);
  /// read buff of on message receive in the corresponding channel persistence
  /// (copy array) -> TODO on js some congestion mechanism : check webrtc data if there is a way to
  /// tell sender to suspend send until ok (asynch send is good for it not sure about webrtc :
  /// could add a layer for it but not for now).
  pub fn read(_chan : *mut c_void, _buf : *mut u8, _buflen : usize) -> usize;
  
  /// fn to call to give context back (eg js without worker)
  pub fn suspend(_self : *mut c_void);
}


// TODO P as poll might be a bad idea : direct use of UserPoll more correct??
// TODO make it registerable!!
pub struct ExtTransport {
  pub listener_id : usize,
  mult : bool,
  reg : UserRegistration,
  slab : Slab<SlabEntry>,
  state : TState,
}

/// currently channel id on 16 bits (0 to 65 k)
pub type ChannelId = Box<Rc<Cell<u16>>>;

#[derive(Clone)]
pub struct ExtChannel {
  listener_id : usize,
  dest_id : Vec<u8>,
  count : ChannelId,
  reg : UserRegistration,
  // no chan_id it can stay at js level js will keep association chanid with this struct pointer 
  //chan_id : u16, // 65k chan
  state : TState,
}

pub type TState = Box<Rc<Cell<TransportState>>>;

#[derive(Copy,Clone,Eq,PartialEq,Debug)]
#[repr(u8)]
pub enum TransportState {
  Querying = 0,
  CouldNotConnect = 1,
  Connected = 2,
  Closed = 3,
  WriteError = 4,
  ReadError = 5,
}

impl ExtTransport {
  
  pub fn new(listener_id : usize, slab : Slab<SlabEntry>, mult : bool) -> (Self,UserSetReadiness,TState) {

    let state = Box::new(Rc::new(Cell::new(TransportState::Querying)));

    let (usr,reg) = poll_reg();
    let t = ExtTransport {
      listener_id,
      mult,
      reg,
      slab,
      state : state.clone(),
    };

    (t,usr,state)
  }
}

#[derive(Serialize,Deserialize,Debug, PartialEq, Eq, Clone, Hash)]
pub struct ByteAddress(Vec<u8>);
impl Address for ByteAddress {}

impl Transport<UserPoll> for ExtTransport {
  type ReadStream = ExtChannel;
  type WriteStream = ExtChannel;
  type Address = ByteAddress;


  fn accept(&self) -> Result<(Self::ReadStream, Option<Self::WriteStream>)> {
    let void_id = self.listener_id as *mut c_void;
    let mut id_out_i : *mut u8 = &mut (0 as u8) as *mut u8;
    let id_out : *mut *mut u8 = &mut id_out_i as *mut *mut u8;
    let id_len_out : *mut usize = &mut (0 as usize) as *mut usize;
    let chan_out : *mut u16 = &mut (0 as u16) as *mut u16;
    if unsafe { next_pending_connected(void_id, id_out, id_len_out, chan_out) } {
      let dest_id = unsafe {
        slice::from_raw_parts(*id_out, *id_len_out)
      }.to_vec();

      let chan_count = unsafe { *chan_out };
      let (usr,reg) = poll_reg();
      let trig_ch = Box::new(usr);
      let state = Box::new(Rc::new(Cell::new(TransportState::Connected)));
      unsafe {
        let h_trig_ch = Box::into_raw(trig_ch) as *mut c_void;
        let h_state_ch = Box::into_raw(state.clone()) as *mut c_void;
        read_channel_reg(void_id,*id_out,*id_len_out,chan_count,h_trig_ch,h_state_ch);
      }
      
      let rs = ExtChannel {
        listener_id : self.listener_id.clone(),
        dest_id,
        reg,
        state,
        // init at 0, option would be redundant with state
        count : Box::new(Rc::new(Cell::new(chan_count))),
      };
      let ows = if self.mult {
        Some(rs.clone())
      } else {
        None
      };
      Ok((rs,ows))
    } else {
      // mydht would block
      Err(Error("".to_string(),ErrorKind::ExpectedError,None))
    }
  }



  fn connectwith(&self,  p : &Self::Address) -> IoResult<(Self::WriteStream, Option<Self::ReadStream>)> {
/*    if (*self.state).borrow() : &TransportState == &TransportState::Querying {
      return Err(IoError::new(IoErrorKind::WouldBlock,""))
    }*/
    log_js(format!("connect with state : {:?}", self.state.get()), LogType::Log);
    if self.state.get() == TransportState::Closed {
      return Err(IoError::new(IoErrorKind::ConnectionAborted,"Disconected transport"))
    }

    let (usr,reg) = poll_reg();
    let stream = ExtChannel {
      listener_id : self.listener_id.clone(),
      dest_id : p.0.clone(),
      reg,
      state : Box::new(Rc::new(Cell::new(TransportState::Querying))),

      // init at 0, option would be redundant with state
      count : Box::new(Rc::new(Cell::new(0))),
    };

    let mut destid = p.0.clone();
    let destlen = destid.len();
    let busr = Box::new(usr);
    let bstate = stream.state.clone();
    let bcount = stream.count.clone();
    unsafe {
      let ptrid = destid.as_mut_ptr();
      let h_usr = Box::into_raw(busr) as *mut c_void;
      let h_count = Box::into_raw(bcount) as *mut c_void;
      let h_state = Box::into_raw(bstate) as *mut c_void;
      let h_transport = stream.listener_id.clone() as *mut c_void;
      query_new_channel(h_transport, ptrid as *mut u8, destlen, h_usr, h_state, h_count);
    }
    mem::forget(destid);
    //mem::forget(busr);
    //mem::forget(bcount);
    let orstream = if self.mult {
      Some(stream.clone())
    } else {
      None
    };
    Ok((stream,orstream))

  }
}



impl Registerable<UserPoll> for ExtTransport {
  #[inline]
  fn register(&self, poll : &UserPoll, token: Token, interest: Ready) -> Result<bool> {
    self.reg.register(poll,token,interest)
  }
  #[inline]
  fn reregister(&self, poll : &UserPoll, token: Token, interest: Ready) -> Result<bool> {
    self.reg.reregister(poll,token,interest)
  }
  #[inline]
  fn deregister(&self, poll : &UserPoll) -> Result<()> {
    self.reg.deregister(poll)
  }
}

impl Registerable<UserPoll> for ExtChannel {
  #[inline]
  fn register(&self, poll : &UserPoll, token: Token, interest: Ready) -> Result<bool> {
    self.reg.register(poll,token,interest)
  }
  #[inline]
  fn reregister(&self, poll : &UserPoll, token: Token, interest: Ready) -> Result<bool> {
    self.reg.reregister(poll,token,interest)
  }
  #[inline]
  fn deregister(&self, poll : &UserPoll) -> Result<()> {
    self.reg.deregister(poll)
  }
}

impl WriteTransportStream for ExtChannel {
  fn disconnect(&mut self) -> IoResult<()> {
    unimplemented!("TODO change state and call channel close"); 
  }
}
impl ReadTransportStream for ExtChannel {
  fn disconnect(&mut self) -> IoResult<()> {
    unimplemented!("TODO change state and call channel close"); 
  }
  /// TODO remove
  fn rec_end_condition(&self) -> bool {
    false
  }
}

impl Write for ExtChannel {
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
      unimplemented!("TODO with write ext, on 0 Io error should block (asynch tr and full) : only error when the channel got a error state (check state)");
    }
    fn flush(&mut self) -> IoResult<()> {
      unimplemented!("TODO nothing or flush js write buffer : currently not");
    }
}
impl Read for ExtChannel {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
      unimplemented!("TODO with read ext read from buffer of on message receive and ior should block if empty");
    }
}




#[cfg(feature = "jstest")]
pub struct TestService {
  transport : ExtTransport,
  count : usize,
  event_loop : UserPoll,
  events : UserEvents,
  con_queue : VecDeque<TestCommand>,
}

#[cfg(feature = "jstest")]
impl ServiceRestartable for TestService { }
#[cfg(feature = "jstest")]
impl Service for TestService {
  type CommandIn = TestCommand;
  type CommandOut = ();

  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, async_yield : &mut S) -> Result<Self::CommandOut> {
    match req {
      TestCommand::ConnectWith(destid) => {
        log_js(format!("got connect query to : {:?}", &destid), LogType::Log);
        let add = ByteAddress(destid);
        let (ws,ors) = self.transport.connectwith(&add)?;

        // TODO register and store
        return Ok(());
        /*match self.transport.connectwith(&add) {
          Ok((ws,ors)) => {
        // TODO register and store
        return Ok(());
          },
          Err(ref e) if e.kind() == IoErrorKind::WouldBlock => {
            log_js(format!("queuing connect to : {:?}", &add), LogType::Log);
            self.con_queue.push_back(TestCommand::ConnectWith(add.0));
            return Ok(());
          },
          Err(e) => return Err(e.into()),
        };*/
      },
      TestCommand::None => (),// poll
    }
    log_js(format!("service count : {}", self.count), LogType::Log);
    self.count += 1;
    loop {
      let nb = self.event_loop.poll(&mut self.events, None)?;
      if nb == 0 {
        if let YieldReturn::Return = async_yield.spawn_yield() {
          // yield
          return Err(Error("".to_string(), ErrorKind::ExpectedError, None));
        }
      }
      while let Some(event) = self.events.next() {
        log_js(format!("got event : {:?}", event), LogType::Log);
        // TODO on listener ready empty conn_queue!!
        match self.transport.slab.get(event.token) {
          Some(&SlabEntry::Listener) => {
            let (rs,owrs) = self.transport.accept()?;
            log_js(format!("receive a transport"), LogType::Log);
            // TODO register and store
          },
          None => log_js(format!("no slab entry for event : {:?}", event), LogType::Error),
        }
      }
    }
 
    Ok(())
  }

}

//const EVENTS_POLL_SIZE = 100;
const EVENTS_POLL_SIZE : usize = 5;

const POLL_FD : usize = 1;
#[cfg(feature = "jstest")]
pub enum SlabEntry {
  Listener,
}

#[cfg(feature = "jstest")]
pub fn start_service(id : *mut u8, idlen : usize) {

  let id_listen = unsafe {
    slice::from_raw_parts(id, idlen)
  }.to_vec();

  let (sc,sr) = LocalRcChannel.new().unwrap(); // this channel never fail
  let sc = Box::new(sc);
  let h_sc = Box::into_raw(sc) as *mut c_void;
  let hptr = h_sc as usize;
  let mut slabcache = Slab::new();
  let ttoken = slabcache.insert(SlabEntry::Listener);
  // use ptr on inp chan as js id (more compact than vec)
  let (transport, transport_trigger, transport_state) = ExtTransport::new(hptr, slabcache, true); 
  let event_loop = UserPoll::new(POLL_FD,true);
  assert!(transport.register(&event_loop, ttoken, Ready::Readable).unwrap());

  let service = TestService {
    transport,
    count : 0,
    event_loop,
    events : UserEvents::with_capacity(EVENTS_POLL_SIZE),
    con_queue : VecDeque::new(),
  };
  let handle = RestartOrError.spawn(
    service,
    NoSend,
    None,
    DefaultRecv(sr,TestCommand::None),
    0
  ).unwrap();

  let handle = Box::new(handle);
  let trig_tr = Box::new(transport_trigger);
  unsafe {
    let h_ptr = Box::into_raw(handle) as *mut c_void;
    let h_trig_tr = Box::into_raw(trig_tr) as *mut c_void;
    let h_state = Box::into_raw(transport_state) as *mut u8;
//   yield_loop(hptr);
    query_listener(h_sc,h_trig_tr,h_ptr,h_state,id, idlen);
  }

}

#[repr(u8)]
pub enum LogType { Log = 0, Error = 1, Alert = 2, }

pub fn log_js(m : String, t : LogType) {
  let m = CString::new(m.into_bytes()).unwrap(); // warn panic on internal \0
  unsafe {
    wasm_log(m.as_ptr(), t)
  }
}

