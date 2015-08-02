//! Tcp transport using system loop instead of multiple threads.
//! Implementation is therefore looping over receive method while stream objects are just shared reference
//! to token/client id...
//! TODO mapping token Filedesc value.
//! TODO test that, and make it work.
//!
//!  TODO handler is never started : store it in MyHandler and run handler when trigger on a socket
//!  fd (from not waiting to something else) !!! do not start on incoming connection but on read
//!  start : Use handler modified to run once, and add post action of handler to state init
//!  With new transport interface, check end_loop would reset state to init, and received content
//!  on init state does start handler (otherwhise condvar unlock).
//!  The condition on stop loop, is no more read : state not Something then end and Init state
//!  othewhise loop and read (may fail : read error break the loop but as next condition will got
//!  NoWaiting state)
//!  All this thread state stuff is pretty racy...
//!  Need handler fn in MyHandlefn r
//!  
//!  Note that spawn mode is not the same as usual transport because thread do not persist with the
//!  peermanager clientinfo : it allow a spawn (spawn occurs in transport (and end
//!  of loop is automatic)) but not its loop. // TODO may scoped thread to access TcpStream without Fd unsafty. see
//!  ScopedKey (rather not as scoped threads looks deprecated).
//!
//!
//!  TODO for the threaded mode it could be better to have already spawn threads for multiple
//!  connection : see client definition. A loop event start a read for a thread : we need to
//!  add  functionalities to handler message to manage pool and send msg with token for read (then run once
//!  thread). We might need another conf for dht rules (not transport (neutral)).
//!  That is more a mater of creating a new recv handler managing pool of readstream.
//!  (avoid all those spawn in run once).

extern crate byteorder;
extern crate mio;
use self::byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Result as IoResult;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::io::Write;
use std::io::Read;
use time::Duration;
use super::{Transport,ReadTransportStream,WriteTransportStream};
use std::net::SocketAddr;
use self::mio::tcp::TcpSocket;
use self::mio::tcp::TcpListener;
//use self::mio::Socket;
use self::mio::tcp::TcpStream as MioTcpStream;
use self::mio::tcp;
use self::mio::Token;
use self::mio::Timeout;
use self::mio::EventSet;
use self::mio::EventLoop;
use self::mio::Sender;
use self::mio::Handler;
use self::mio::tcp::Shutdown;
use super::Attachment;
use num::traits::ToPrimitive;
use std::collections::VecMap;
use std::sync::Mutex;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::PoisonError;
use std::error::Error;
use utils::{OneResult,ret_one_result,clone_wait_one_result,clone_wait_one_result_timeout_ms,one_result_val_clone,change_one_result};
use std::os::unix::io::AsRawFd;
use std::os::unix::io::FromRawFd;

const CONN_REC : usize = 0;

/// Tcp struct : two options, timeout for connect and time out when connected.
pub struct Tcp {
  keepalive : Option<Duration>,
  timeout : Duration,
  // choice of read model for stream (either coroutine in same thread or condvar synch reading with
  // other thread). Note that could be feature gate to avoid enum overhead.
  spawn : bool,
  // TODO maybe third choice where we include first read message in event loop but still spawn
  // thread for further processing (heavy validation or other but short message reading).
  
  // usage of mutex is not so good yet it is only used on connection init
  // TODO may be able to remove option (new init)
  channel : Mutex<Option<Sender<HandlerMessage>>>,
  listener : TcpListener,
}


impl Tcp {
  /// constructor.
  pub fn new (p : &SocketAddr,  keepalive : Option<Duration>, timeout : Duration, spawn : bool) -> IoResult<Tcp> {

    let tcplistener = try!(TcpListener::bind(p));

    Ok(Tcp {
      keepalive : keepalive,
      timeout : timeout,
      spawn : spawn,
      channel : Mutex::new(None),
      listener : tcplistener,
    })
  }
}

#[derive(Clone)]
pub enum StreamInfo {
  Threaded(OneResult<ThreadState>),
  CoRout(Token, Option<Timeout>),
}

impl StreamInfo {
  pub fn set_timeout(&mut self, to : Option<Timeout>) {
    match self {
      &mut StreamInfo::Threaded(_) => (),
      &mut StreamInfo::CoRout(_, ref mut top) =>  *top = to,
    };
  }
  pub fn get_timeout(&self) -> Option<Timeout> {
    match self {
      &StreamInfo::Threaded(_) => None,
      &StreamInfo::CoRout(_, ref to) => to.clone(), 
    }
  }
}

#[derive(PartialEq,Eq,Clone)]
/// TODO state must be simplier to avoid race over oneresult :
/// just trigger between something and waiting and relase condvar on every something
/// and primitive on read which block only if not Something (here primitive block anyway)
/// that is for threaded : the condvar lock must be used the states are indicative (except
/// Something when setting a condvar)
pub enum ThreadState {
  // Handler not started
  Init,
  // evloop triggered on socket
  Something,
  // end of read
  NoWaiting,
  // a read thread is waiting
  Waiting,
  // timeout for read thread or other error
  Timeout,
}
pub struct WriteTcpStream (MioTcpStream);
pub enum ReadTcpStream {
  Threaded(MioTcpStream,Token, OneResult<ThreadState>,Sender<HandlerMessage>,u32),
  CoRout(MioTcpStream,Token,Sender<HandlerMessage>),
}

struct MyHandler<'a, C>
    where C : Fn(ReadTcpStream,Option<WriteTcpStream>) -> IoResult<()> {

  /// info about stream (for unlocking read)
  tokens : VecMap<StreamInfo>,
  timeout : u64,
  listener : &'a TcpListener,
  readHandler : C,
}

impl<'a,C> MyHandler<'a,C>
    where C : Fn(ReadTcpStream,Option<WriteTcpStream>) -> IoResult<()> {


  // TODO maybe map token on RawFd of socket values (no need for tokens persistence for
  // coroutine??) - > then deregister is easier (fromfd of token val). !!!!!
  fn new_token(&self) -> Token {
    // start at 1 (0 is reserved for connection receiption)
    let mut r = 1;
    // need keys in ascending order!! see vecmap doc
    for i in self.tokens.keys() {
      // no need for max usize break, it will be 0
      if i != r {
        break;
      } else {
        r += 1;
      }
    };
    if r == 0 {
      panic!("Token map full for tcp eventloop transport!!!");
    };
    Token(r)
  }
}

impl<'a,C> Handler for MyHandler<'a,C>
    where C : Fn(ReadTcpStream,Option<WriteTcpStream>) -> IoResult<()> {
  type Timeout = Token;
  type Message = HandlerMessage;


  /// Message for setting a timeout or adding a connection
  fn notify(&mut self, event_loop: &mut EventLoop<Self>, msg: HandlerMessage) {
    match msg {
      HandlerMessage::NewConnTh(r, fd) => {
        let tok = self.new_token();
        let sync = Arc::new((Mutex::new(ThreadState::Init),Condvar::new()));
        // TODO find something else than unsafe
        let mstream : MioTcpStream = unsafe { FromRawFd::from_raw_fd(fd) };
        // TODO error management
        event_loop.register(&mstream, tok);
        let si = StreamInfo::Threaded(sync.clone());
        self.tokens.insert(tok.as_usize(), si.clone());
        ret_one_result(&r, Some((tok,si)));
      },
      HandlerMessage::NewConnCo(r) => {
        let tok = self.new_token();
        let si = StreamInfo::CoRout(tok.clone(), None);
        self.tokens.insert(tok.as_usize(), si.clone());
        ret_one_result(&r, Some((tok,si)));
      },
      HandlerMessage::Timeout(tok) => {
/*        if let Some(si) = self.tokens.get(&tok.0) {
          event_loop.deregister(si.get_stream());
        };*/
        self.tokens.remove(&tok.0);
      },
      HandlerMessage::StartTimeout(tok) => {
        if self.tokens.contains_key(&tok.0) {
          // start timeout
          match event_loop.timeout_ms(tok, self.timeout) {
            Ok(timout) => {
              // store in token
              if let Some(si) = self.tokens.get_mut(&tok.0) {
                si.set_timeout(Some(timout));
              }
            },
            Err(e) => {
              error!("Cannot set timeout in eventloop : {:?}",e);
            },
          }
        };
      },
    }
  }

  /// on timeout, remove token after return with error state (so linked read will return a timeout error)
  fn timeout(&mut self, event_loop: &mut EventLoop<Self>, timeout: Token) {
    let rem = self.tokens.get_mut(&timeout.0).map(|mut si|{
      match si {
        &mut StreamInfo::Threaded(_) => {
          false
        },
        &mut StreamInfo::CoRout(_,ref mut to) => {
          *to = None;
          // TODO
          /*
           *          let st = one_result_val_clone(state);
          if st == Some(ThreadState::Waiting) {
            ret_one_result(state, ThreadState::Timeout);
            true
          } else {
            false
          }
*/
          true
        },

      }
    }).unwrap_or(false);
    if rem {
/*      if let Some(si) = self.tokens.get(&timeout.0) {
        event_loop.deregister(si.get_stream());
      };*/
      self.tokens.remove(&timeout.0);
    };
  }

  /// on read get token and change its state to something to read, 
  fn ready(&mut self, event_loop: &mut EventLoop<Self>, token: Token, _: EventSet) {
    if token == Token(0) {
      // Incoming connection TODO !!!! (see tcp)
      loop {
        let socket = self.listener.accept();
        match socket {
            Err(e) => {error!("Socket acceptor error : {:?}", e);}
            Ok(None)  => {
              break;
            },
            Ok(Some(mut s))  => {
              debug!("Initiating socket exchange : ");
              debug!("  - From {:?}", s.local_addr());
              debug!("  - From {:?}", s.peer_addr());
              debug!("  - With {:?}", s.peer_addr());
              //s.0.set_keepalive (self.keepalive.map(|d|d.num_seconds().to_u32().unwrap())));
              //s.set_read_timeout(self.timeout.num_seconds().to_u64().map(StdDuration::from_secs));
              //s.set_write_timeout(self.timeout.num_seconds().to_u64().map(StdDuration::from_secs));

        let tok = self.new_token();
        let sync = Arc::new((Mutex::new(ThreadState::Init),Condvar::new()));
        // TODO error management
        event_loop.register(&s, tok);
        let si = StreamInfo::Threaded(sync.clone());
        self.tokens.insert(tok.as_usize(), si.clone());
 // TODO both handlers !!!!!
              //handler(s, None);
            }
        }
      }
    } else {
    // TODO fuse that in next get (not prio) (only for corout) TODO remove if corout do not use it
    if let Some(si) = self.tokens.get_mut(&token.0) {
      si.get_timeout().map(|to|event_loop.clear_timeout(to)).unwrap_or(false);
      si.set_timeout(None);
    };
 
    match self.tokens.get(&token.0) {
      Some(&StreamInfo::Threaded(ref state)) => {
        let st = one_result_val_clone(state);

        if st == Some(ThreadState::Waiting) {
          ret_one_result(state, ThreadState::Something);
        } /*else {
          // maybe useless : we directly read in socket
          change_one_result(state, ThreadState::Something);
        }*/
      },
      Some(&StreamInfo::CoRout(_,_)) => {
        // TODO
      },
      //if not in map log error
      None => error!("receive content for disconnected stream, timeout may be to short"),
    }
    }

  }

}

pub enum HandlerMessage {
  NewConnTh(OneResult<Option<(Token,StreamInfo)>>, i32),
  NewConnCo(OneResult<Option<(Token,StreamInfo)>>),
  Timeout(Token),
  StartTimeout(Token),
}

impl ReadTcpStream {
  fn get_stream(&mut self) -> &mut MioTcpStream {
    match self {
      &mut ReadTcpStream::Threaded(ref mut s,_,_,_,_) => s,
      &mut ReadTcpStream::CoRout(ref mut s,_,_) => s,
    }
  }
}

impl Transport for Tcp {
  type ReadStream = ReadTcpStream;
  type WriteStream = WriteTcpStream;
  type Address = SocketAddr;
  fn start<C> (&self, readHandler : C) -> IoResult<()>
    where C : Fn(Self::ReadStream,Option<Self::WriteStream>) -> IoResult<()> {
    /*let sock : TcpSocket = try!(tcp:match *p {
      SocketAddr::V4(..) => TcpSocket::v4(),
      SocketAddr::V6(..) => TcpSocket::v6(),
    });
    // Set SO_REUSEADDR
    try!(sock.set_reuseaddr(true));
    // Bind the socket
    try!(sock.bind(p));
    try!(sock.set_keepalive (self.keepalive.map(|d|d.num_seconds().to_u32().unwrap())));
    // listen
    let tcplistener = try!(sock.listen(1024));*/
//    tcplistener.0.set_write_timeout_ms(self.timeout.num_milliseconds());


    // default event loop TODO maybe config in Tcp ??
    // No timeout to
    let mut eventloop = try!(EventLoop::new());
    let sender = eventloop.channel();
    { // mutex section
      let mut mchannel = self.channel.lock().unwrap(); // TODO unsafe
      *mchannel = Some(sender);
    }
    try!(eventloop.register(&self.listener, Token(CONN_REC)));
    eventloop.run(&mut MyHandler{
      tokens : VecMap::new(),
      timeout : self.timeout.num_milliseconds().to_u64().unwrap(),
      listener : &self.listener,
      readHandler : readHandler,
      })
  }

  fn connectwith(&self,  p : &SocketAddr, timeout : Duration) -> IoResult<(Self::WriteStream, Option<Self::ReadStream>)> {
    let s = try!(MioTcpStream::connect(p));
    try!(s.set_keepalive (self.keepalive.map(|d|d.num_seconds().to_u32().unwrap())));
    let ws = try!(s.try_clone());
//    try!((s.0).0.set_write_timeout_ms(self.timeout.num_milliseconds().to_usize().unwrap()));
    //try!(s.0.set_read_timeout_ms(self.timeout.num_milliseconds().to_usize().unwrap()));
    
    // get token from event loop!!
    let (sync,ch) : (OneResult<Option<(Token,StreamInfo)>>, Sender<HandlerMessage>) = {
      // mutex section
      let mchan = self.channel.lock().unwrap(); //TODO unsafe
      match *mchan {
        None => {
          // not initialized event loop,
          let msg = "Event loop of tcp loop not initialized, server process may not have start properly";
          error!("{}",msg);
          return Err(IoError::new(IoErrorKind::Other, msg));
        },
        Some(ref ch) => {
          if self.spawn {
            let sync = Arc::new((Mutex::new(None),Condvar::new()));
            ch.send(HandlerMessage::NewConnTh(sync.clone(), s.as_raw_fd()));
            (sync, ch.clone())
          } else {
            let sync = Arc::new((Mutex::new(None),Condvar::new()));
            ch.send(HandlerMessage::NewConnCo(sync.clone()));
            (sync, ch.clone())
          }
        },
      }
    };
    // wait for token in condvar
    let oosi = clone_wait_one_result(&sync,None);
    let si = if oosi.is_none() {
      return Err(IoError::new(IoErrorKind::Other, "eventloop return no stream info, error occurs"));
    } else {
      let osi = oosi.unwrap();
      if osi.is_none() {
        return Err(IoError::new(IoErrorKind::Other, "eventloop return no stream infos"));
      } else {
        osi.unwrap()
      }
    };
    let tok = si.0;
    match si.1 {
      StreamInfo::Threaded(state) => {
        Ok((WriteTcpStream(ws),
          Some(ReadTcpStream::Threaded(s, tok, state,ch,self.timeout.num_milliseconds().to_u32().unwrap())
        )))
      },
      StreamInfo::CoRout(_,_) => {
        Ok((WriteTcpStream(ws),
        Some(ReadTcpStream::CoRout(s,tok,ch))))
      },

    }
  }
  /// if spawn we do not loop (otherwhise we should not event loop at all), the thread is just for
  /// one message.
  fn do_spawn_rec(&self) -> (bool,bool) {(self.spawn,false)}

  /// deregister the read stream for this address
  fn disconnect(&self, sock : &SocketAddr) -> IoResult<bool> {
    // TODO  deregister ReadStream : PB we do not have mapping address stream token -> TODO add
    // this mapping (costy...

    Ok(true)
  }

}


impl ReadTcpStream {
  fn read_th(buf: &mut [u8], s : &mut MioTcpStream, tok : &Token, or : &OneResult<ThreadState>, ch : &Sender<HandlerMessage>, to : &u32) -> IoResult<usize> {
    // on read start if state is "nothing to read", start timeout and init condvar
    let nb = try!(s.read(buf));
    if nb == 0 {
      // waiting state
      change_one_result(or, ThreadState::Waiting);
      // OneResult clonewait fn (doable on condvar). -> do it later (Sender is still usefull to ask
      // for removal in loop). = no nedd to send timeout msg but rmv msg in case needed, no need to
      // do something when timeout on eventloop, and no need to clear timeout on notify
      let r = clone_wait_one_result_timeout_ms(or, Some(ThreadState::NoWaiting), *to);
      if r.is_none() {
        change_one_result(or, ThreadState::Timeout);
        ch.send(HandlerMessage::Timeout(tok.clone()));

        Err(IoError::new(IoErrorKind::Other, "time out on transport reception"))
      } else {
        // aka r == Threadstate::Something : read again, we trust the loop so no recursive call
        s.read(buf)
      }
    } else {
      Ok(nb)
    }

  }
  fn read_co(buf: &mut [u8], s : &mut MioTcpStream, tok : &Token, ch : &Sender<HandlerMessage>) -> IoResult<usize> {
    let nb = try!(s.read(buf));
    if nb == 0 {
      // waiting state
    /*  change_one_result(or, ThreadState::Waiting);
      // start a timeout
      ch.send(HandlerMessage::StartTimeout(tok.clone()));
      let r = clone_wait_one_result(or, Some(ThreadState::Init));
      let r = clone_wait_one_result_timeout_ms(or, Some(ThreadState::Init), to);
      if r.is_none() || r == Some(ThreadState::Timeout) {
        Err(IoError::new(IoErrorKind::Other, "time out on transport reception"))
      } else {
        // aka r == Threadstate::Something : read again, we trust the loop so no recursive call
        s.read(buf)
      }*/
    };
 
    Ok(0)
  }

}

impl Read for ReadTcpStream {
  fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
    match self {
      &mut ReadTcpStream::Threaded(ref mut s,ref tok, ref or, ref ch, ref to) => ReadTcpStream::read_th(buf,s,tok,or,ch,to),
      &mut ReadTcpStream::CoRout(ref mut s,ref tok, ref ch) =>  ReadTcpStream::read_co(buf,s,tok,ch),
    }
  }
}
impl ReadTransportStream for ReadTcpStream {

  fn disconnect(&mut self) -> IoResult<()> {
    match self {
      &mut ReadTcpStream::Threaded(ref mut s,ref tok, ref or, ref ch, ref to) => {
    // TODO send token (plus address for mapping table) Disconnect in channel and from msg deregister and remove infos
      },
      &mut ReadTcpStream::CoRout(ref mut s,ref tok, ref ch) =>  {
        //TODO howto ???
      },
    };
    Ok(())
  }
 
  /// no loop (already event loop)
  fn rec_end_condition(&self) -> bool {
    true
  }

}
/*
impl Write for WriteTcpStream {
  fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
    self.write(buf)
  }
  fn flush(&mut self) -> IoResult<()> {
    self.flush()
  }
}*/

impl WriteTransportStream for WriteTcpStream {

  fn disconnect(&mut self) -> IoResult<()> {
    self.0.shutdown(Shutdown::Write)
  }
 
}
impl Write for WriteTcpStream {
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
      self.0.write(buf)
    }
    fn flush(&mut self) -> IoResult<()> {
      self.0.flush()
    }
}


