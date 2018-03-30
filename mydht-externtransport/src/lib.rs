// TODO giveup on generic approach and be webrtc sepcific ?

extern crate mydht_base;
extern crate mydht_userpoll;
extern crate serde;
#[macro_use] extern crate serde_derive;

use std::mem;
use std::io::Write;
use std::io::Read;
use std::io::Result as IoResult;

use mydht_base::mydhtresult::{
  Result,
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
  UserRegistration,
  UserSetReadiness,
  poll_reg,
};

use std::os::raw::{
  c_char,
  c_void,
};
use std::marker::PhantomData;

extern {
  /// ask for a listener. In our webrtc case it is an asynchronous process were we connect through
  /// websocket to the user server, publishing our connection id.
  /// token will be use latter for registration
  /// TODO on call forget _poll pointer (should deallocate later)
  /// should callback on 'start_with_listener' or similar function
  /// TODO if challenge with websocket server will need more parameters (currently no challenge and
  /// unsafe registering)
  fn query_listener(_id : *mut u8, _idlen : usize, _transport : *mut c_void);
  /// this should only be called when there is a ready event on the poll, it returns the port for the channel
  fn next_pending_connected(_id : *mut u8, _idlen : usize) -> u16;

  /// connect to dest
  /// TODO callback on a set_ready with channel id see 'connect_success' and 'connect_fail' (chan
  /// pointer is for call back and to identify the chan at a js level)
  fn query_new_channel(_dest_id : *mut u8, _idlen : usize, _transport : *mut c_void, _chan : *mut c_void);
  /// when rust go a new channel he send it back for set_readiness (here we do not build a
  /// connection, it already xist : we simply send the channel pointer
  fn read_channel_reg(_dest_id : *mut u8, _idlen : usize, _chan_id : u16, _chan : *mut c_void);
  // no call back here at some point need an association table between pointer chan and actuall
  // chanel
  fn close_channel(_chan : *mut c_void);

  /// TODO add returned possible error code
  /// call back with a set write ready when content send ( when buff full we return 0 from write
  /// meaning that rust should io not ready error on it)
  fn write(_chan : *mut c_void, _content : *mut u8, _contentlen : usize) -> usize;
  /// return possible error code as call back (same as write), flush the buffer!!
  fn flush_write(_chan : *mut c_void);
  /// read buff of on message receive in the corresponding channel persistence
  /// (copy array) -> TODO on js some congestion mechanism : check webrtc data if there is a way to
  /// tell sender to suspend send until ok (asynch send is good for it not sure about webrtc :
  /// could add a layer for it but not for now).
  fn read(_chan : *mut c_void, _buf : *mut u8, _buflen : usize) -> usize;
  
  /// fn to call to give context back (eg js without worker)
  fn suspend(_self : *mut c_void);
}

#[no_mangle]
pub extern "C" fn restore(_self: *mut c_void) {
  unimplemented!("restore a suspended context");
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
/// reply to a query_listener call
/// Note that this function should be redefine
#[no_mangle]
pub extern "C" fn start_with_listener(_id : *mut u8, _idlen : usize, _listener_set_readiness : *mut c_void) -> *const u8 {
  unimplemented!("a start depending on our tests : should move in main.rs")
}
#[no_mangle]
pub extern "C" fn connect_success(_id : *mut u8, _idlen : usize, _transport : *mut c_void, _chan_pt : *mut c_void) {
  unimplemented!("trigger an event (setreadiness of transport listener) and communicate success through + register channel state")
}

#[no_mangle]
pub extern "C" fn connect_fail(_id : *mut u8, _idlen : usize, _transport : *mut c_void, _chan_pt : *mut c_void) {
  unimplemented!()
}
#[no_mangle]
pub extern "C" fn receive_connect(_id : *mut u8, _idlen : usize, _transport : *mut c_void, _chan_id : u16) {
  unimplemented!("trigger an event (setreadiness of transport listener) and communicate success through + register channel state")
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

// TODO P as poll might be a bad idea : direct use of UserPoll more correct??
// TODO make it registerable!!
pub struct ExtTransport {
  listener_id : Vec<u8>,
  mult : bool,
  reg : UserRegistration,
  state : TransportState,
}

// TODO make it registerable!! see userpoll
pub struct ExtChannel {
  dest_id : Vec<u8>,
  reg : UserRegistration,
  // no chan_id it can stay at js level js will keep association chanid with this struct pointer 
  //chan_id : u16, // 65k chan
  state : TransportState,
}

pub enum TransportState {
  Querying,
  CouldNotConnect,
  Connected,
  Closed,
  WriteError,
  ReadError,
}

impl ExtTransport {
  
  pub fn new(listener_id : Vec<u8>, mult : bool) -> (Self,UserSetReadiness) {


    let (usr,reg) = poll_reg();
    let t = ExtTransport {
      listener_id,
      mult,
      reg,
    };

    unimplemented!("TODO call query");

    (t,usr)
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
    // TODO if state == connection fail trigger an error could not listen
    unimplemented!("TODO use next_pending_connected and build both streams then send ptr to js through read_register");
/*    let (s,ad) = self.listener.accept()?;
    debug!("Initiating socket exchange : ");
    debug!("  - From {:?}", s.local_addr());
    debug!("  - With {:?}", s.peer_addr());
    debug!("  - At {:?}", ad);
    s.set_keepalive(self.keepalive)?;
//    try!(s.set_write_timeout(self.timeout.num_seconds().to_u64().map(Duration::from_secs)));
    if self.mult {
//            try!(s.set_keepalive (self.timeout.num_seconds().to_u32()));
        let rs = try!(s.try_clone());
        Ok((s,Some(rs)))
    } else {
        Ok((s,None))
    }*/
  }


  fn connectwith(&self,  p : &Self::Address) -> IoResult<(Self::WriteStream, Option<Self::ReadStream>)> {
    unimplemented!("TODO instantiate extchannel in querying state plus some poll events, no register (done by code mydht or other loop usage), and call query new channel method return this querying channel")
/*    let s = TcpStream::connect(&p.0)?;
    s.set_keepalive(self.keepalive)?;
    // TODO set nodelay and others!!
//    try!(s.set_keepalive (self.timeout.num_seconds().to_u32()));
    if self.mult {
      let rs = try!(s.try_clone());
      Ok((s,Some(rs)))
    } else {
      Ok((s,None))
    }
    */
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