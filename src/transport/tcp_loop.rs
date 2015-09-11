//! 
//! Status : In developpement
//!
//!   - testing connection in threaded mode ok
//!
//!
//! TODO Coroutine spawn failure : due to closure restriction : move coroutine start to server and
//! return handle (closure should return handle), need new localcoroutine mode TODO bef some
//! testing C
//!
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
//!
//!  Coroutine mode is simplier (one coroutine per connection and that is all : no attempt to
//!  reduce the number of coroutine like in threaded mode).
//!  
//!  TODO allow spawn non managed for coroutine mode (similar to spawn udp) ??? do some testing
//!  : usefull (avoid blocked server process : should be default)?
//!
//!  TODO coroutine could maybe run in managed mode -> useless 
//!  
//!  NOTTODO channel mode : Reader only contain a receiver over Vec<u8>, and  stream read is done in
//!  tcp_loop process only -> run in managed mode -> a thread per reader : useless (should run
//!  normal tcp) 
//!
//!
//!  TODO coroutine disconnect : currently nothing done : coroutine exit and no token map update...
//!  (handle and ended corouting kept : need a end message send at end of every coroutine
//!  spawn).!!!! here might get coroutine stuck (read on error does not sched)
//!  TODO coroutine timeout !!!
//!  TODO test
//! 
//!
//!  TODO need testing on disconnect
//!



extern crate byteorder;
extern crate vec_map;
extern crate mio;
use std::sync::mpsc;
use std::sync::mpsc::{Sender};
use std::result::Result as StdResult;
use std::mem;
//use self::byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Result as IoResult;
use mydhtresult::Result;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::io::Write;
use std::io::Read;
use time::Duration;
use super::{Transport,ReadTransportStream,WriteTransportStream,SpawnRecMode,ReaderHandle};
use std::net::SocketAddr;
//use self::mio::tcp::TcpSocket;
use self::mio::tcp::TcpListener;
use std::fmt::Debug;
use self::mio::tcp::TcpStream as MioTcpStream;
//use self::mio::tcp;
use self::mio::Token;
use self::mio::Timeout;
use self::mio::EventSet;
use self::mio::EventLoop;
use self::mio::Sender as MioSender;
use self::mio::Handler;
use self::mio::PollOpt;
use self::mio::tcp::Shutdown;
//use super::Attachment;
use num::traits::ToPrimitive;
use self::vec_map::VecMap;
//use std::sync::Mutex;
//use std::sync::Arc;
//use std::sync::Condvar;
//use std::sync::PoisonError;
use std::error::Error;
use utils::{self,OneResult};
//use std::os::unix::io::AsRawFd;
//use std::os::unix::io::FromRawFd;
use coroutine::Coroutine;
use coroutine::Handle as CoHandle;

#[cfg(feature="with-extra-test")]
#[cfg(test)]
use transport::test::connect_rw_with_optional;
#[cfg(feature="with-extra-test")]
#[cfg(test)]
use transport::test::connect_rw_with_optional_non_managed;
#[cfg(feature="with-extra-test")]
#[cfg(test)]
use utils::{SocketAddrExt,sa4};
#[cfg(feature="with-extra-test")]
#[cfg(test)]
use std::net::Ipv4Addr;

const CONN_REC : usize = 0;

/// Tcp struct : two options, timeout for connect and time out when connected.
pub struct Tcp {
  keepalive : Option<Duration>,
  timeout : Duration,

  // choice of read model for stream (either coroutine in same thread or condvar synch reading with
  // other thread). Note that could be feature gate to avoid enum overhead. TODO actualy spawn is
  // used for that and threaded is used nowhere
  threaded : bool,
  spawn : bool,
  
  // usage of mutex is not so good yet it is only used on connection init
  // TODO may be able to remove option (new init)
  channel : OneResult<Option<MioSender<HandlerMessage>>>,
  listener : TcpListener,
}


impl Tcp {
  /// constructor.
  pub fn new (p : &SocketAddr,  keepalive : Option<Duration>, timeout : Duration, threaded : bool, spawn : bool) -> IoResult<Tcp> {

    let tcplistener = try!(TcpListener::bind(p));

    Ok(Tcp {
      keepalive : keepalive,
      timeout : timeout,
      threaded : threaded,
      spawn : spawn,
      channel : utils::new_oneresult(None),
      listener : tcplistener,
    })
  }
}

#[derive(Clone)]
pub enum StreamInfo {
  Threaded(OneResult<ThreadState>),
  CoRout(CoHandle),
}
/*
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
}*/


/// tread state : true if something to read (true of spurious wakeout) TODO remove condvar is
/// enough for threaded and coroutine has its internal state.
pub type ThreadState = ();
/*
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
}*/

pub struct WriteTcpStream (MioTcpStream);

/// cotain the read stream, token reference 
pub enum ReadTcpStream {
  Threaded(Option<MioTcpStream>,Token,OneResult<ThreadState>,MioSender<HandlerMessage>,u32),
  CoRout(MioTcpStream,Token,MioSender<HandlerMessage>,u32), // plus the fact that it is use in context of coroutine (readhandler is started in a coroutine)
}

impl ReadTcpStream {
  /// end stream returning a new with handle of tcpstream (the one who was registerd).
  /// Coroutine being clonable it is useless.
   fn end_clone(&mut self) -> Self {
     match self {
       &mut ReadTcpStream::Threaded(ref mut st, ref tok, ref or, ref sm, ref to) => {
         let st = mem::replace (st, None);
         ReadTcpStream::Threaded(st, tok.clone(), or.clone(), sm.clone(), to.clone())
       },
       &mut ReadTcpStream::CoRout(..) => panic!("end clone mechanism is only to remove stream from threaded mode, it should not be use with coroutin mode"),
     }
   }
}

struct MyHandler<'a, C>
    where C : Fn(ReadTcpStream,Option<WriteTcpStream>) -> Result<ReaderHandle> {

  keepalive : Option<u32>,
  /// info about stream (for unlocking read)
  tokens : VecMap<StreamInfo>,
  /// sockets between read msg (added on end_read_msg
  socks : VecMap<ReadTcpStream>,
  timeout : u32,
  listener : &'a TcpListener,
  sender : MioSender<HandlerMessage>,
  readhandler : C,
  threaded : bool,
  spawn : bool,
 
}

impl<'a,C> MyHandler<'a,C>
    where C : Fn(ReadTcpStream,Option<WriteTcpStream>) -> Result<ReaderHandle> {


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
    where C : Fn(ReadTcpStream,Option<WriteTcpStream>) -> Result<ReaderHandle> {
  type Timeout = Token;
  type Message = HandlerMessage;


  /// Message for handling a timeout or adding a connection
  fn notify(&mut self, event_loop: &mut EventLoop<Self>, msg: HandlerMessage) {
    match msg {
      HandlerMessage::EndRead(tok, rs) => {
        match self.tokens.get(&tok.0) {
          Some(&StreamInfo::Threaded(ref state)) => {
            let cont = utils::one_result_spurious(state);
            if cont.unwrap() {
              match (self.readhandler)(rs, None) {
                Ok(ReaderHandle::LocalTh(_)) => (),
                Ok(_) => warn!("Unmatched Readerhandle"),
                Err(e) => warn!("Readhandler error in tcp loop notify handler : {}", e),
              };
            } else {
              self.socks.insert(tok.0,rs);
            }
          },
          Some(&StreamInfo::CoRout(..)) => {
            panic!("Coroutine mode do not use EndRead message");
          },
          None => (),
        }
      },
      HandlerMessage::NewConnTh(r, readst) => {
        let tok = self.new_token();
        // we use only spurious wakeout bool
        let sync = utils::new_oneresult(());
        // TODO error management
        match event_loop.register_opt(&readst, tok, EventSet::readable(), PollOpt::edge()) {
           Err(e) => {
           error!("tcp loop register of socket failed when connecting {:?}",&e);
           return
           },
           Ok(()) => (),
        };

        let si = StreamInfo::Threaded(sync.clone());
        self.tokens.insert(tok.as_usize(), si.clone());
        if r.send(Some((tok,si,readst))).map_err(|e|error!("transport send on newconnth broken : {}", e)).is_err() {
          self.tokens.remove(&tok.0);
        };
      },
      HandlerMessage::NewConnCo(r, readst) => {
        let tok = self.new_token();
        match event_loop.register_opt(&readst, tok, EventSet::readable(), PollOpt::edge()) {
           Err(e) => {
           error!("tcp loop register of socket failed when connecting {:?}",&e);
           return
           },
           Ok(()) => (),
        };

        // this result from connect with and write stream is return from connect with (not used in
        // this read handler
        let rts = ReadTcpStream::CoRout(readst,tok,self.sender.clone(),self.timeout.clone());
        let cohandle = match (self.readhandler)(rts, None) {
          Ok(ReaderHandle::Coroutine(ch)) => ch,
          Ok(_) => {
            error!("Readerhandle mismatch for coroutine");
            return
          },
          Err(e) => {
            error!("Readhandler error in tcp loop notify handler : {}", e);
            return
          },
        };
        // run server up to first read where we get back to returning the write stream
        cohandle.resume().ok().expect("Failed to resume coroutine in start of conn to");
        let si = StreamInfo::CoRout(cohandle);
        self.tokens.insert(tok.as_usize(), si);
        if r.send(None).map_err(|e|error!("transport send on newconnth broken : {}", e)).is_err() {
          self.tokens.remove(&tok.0);
        };
 
      },
      HandlerMessage::Timeout(tok) => {
/*        if let Some(si) = self.tokens.get(&tok.0) {
          event_loop.deregister(si.get_stream());
        };*/
        // TODO deregister : may need to add stream in timeout msg (use end_clone).
        self.tokens.remove(&tok.0);
        self.socks.remove(&tok.0);
      },
      /*
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
      },*/
    }
  }

  // on timeout, remove token after return with error state (so linked read will return a timeout error)
  /*
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
           *          let st = utils::one_result_val_clone(state);
          if st == Some(ThreadState::Waiting) {
            utils::ret_one_result(state, ThreadState::Timeout);
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
  }*/

  /// on read get token and change its state to something to read, 
  fn ready(&mut self, event_loop: &mut EventLoop<Self>, token: Token, es: EventSet) {
    if token == Token(CONN_REC) {
      // Incoming connection we loop to empty all possible stacked con
      loop {
        let socket = self.listener.accept();
        match socket {
            Err(e) => {error!("Socket acceptor error : {:?}", e);}
            Ok(None)  => {
              break;
            },
            Ok(Some(s))  => {
              debug!("Initiating socket exchange : ");
              debug!("  - From {:?}", s.local_addr());
              debug!("  - From {:?}", s.peer_addr());
              debug!("  - With {:?}", s.peer_addr());
              let tok = self.new_token();
              //let sync = Arc::new((Mutex::new(ThreadState::Init),Condvar::new()));
              let state = utils::new_oneresult(());
              // TODO error management
              match event_loop.register_opt(&s, tok, EventSet::readable(), PollOpt::edge()) {
                Err(e) => {
                  error!("tcp loop register of socket failed when connecting {:?}",&e);
                  // TODO deregister ??
                  return
                },
                Ok(()) => {
                },
              };



              let sw = s.try_clone().unwrap();
              let ows = Some(WriteTcpStream(sw));
              if s.set_keepalive (self.keepalive).map_err(|e|error!("cannot set keepalif for new socket : {}",e)).is_err() {
                // TODO deregister ??
                return
              };
              // start a receive even if no message : necessary to send ows (warn likely to timeout (or not generally auth right after connect TODO when noauth a ping frame must be send))
              if self.threaded {
                let rs = ReadTcpStream::Threaded(Some(s), tok, state.clone(),self.sender.clone(),self.timeout.clone());

                let si = StreamInfo::Threaded(state.clone());
                self.tokens.insert(tok.as_usize(), si.clone());

                // start only after insert!!
                match (self.readhandler)(rs, ows) {
                  Ok(ReaderHandle::LocalTh(_)) => (),
                  Ok(_) => warn!("Unmatched Readerhandle"),
                  Err(e) => warn!("Readhandler error in tcp loop ready handler : {}", e),
                };

              } else {

                let rs = ReadTcpStream::CoRout(s, tok, self.sender.clone(),self.timeout.clone());
                let cohandle = match (self.readhandler)(rs, ows) {
                  Ok(ReaderHandle::Coroutine(ch)) => ch,
                  Ok(_) => {
                    error!("Readerhandle mismatch for coroutine");
                    return
                  },
                  Err(e) => {
                    error!("Readhandler error in tcp loop notify handler : {}", e);
                    return
                  },
                };
 
                // start server up to read wheer we get back to init stream info
                cohandle.resume().ok().expect("Failed to resume coroutine in start of receiv conn");
                let si = StreamInfo::CoRout(cohandle);
                self.tokens.insert(tok.as_usize(), si);

              }
            }
        }
      }
    } else {
      // a msg for a reader
    // TODO fuse that in next get (not prio) (only for corout) TODO remove if corout do not use it
/* // reinit timeout
      if let Some(si) = self.tokens.get_mut(&token.0) {
        si.get_timeout().map(|to|event_loop.clear_timeout(to)).unwrap_or(false);
        si.set_timeout(None);
      };
*/ 
      match self.tokens.get(&token.0) {
        Some(&StreamInfo::Threaded(ref state)) => {
          // TODO if socket back in sock 
          match self.socks.remove(&token.0) {
            Some(rs) => {
              match (self.readhandler)(rs, None) {
                Ok(ReaderHandle::LocalTh(_)) => (),
                Ok(_) => warn!("Unmatched Readerhandle"),
                Err(e) => warn!("Readhandler error in tcp loop ready handler : {}", e),
              };
            },
            None => {
              utils::ret_one_result(state, ());
            },
          };
        },
        Some(&StreamInfo::CoRout(ref handle)) => {
          // unlock read of coroutine
          handle.resume().ok().expect("Failed to resume coroutine when receiving content from tcp loop");
        },
        //if not in map log error
        None => error!("receive content for disconnected stream, timeout may be to short"),
      }
    }
  }

}

pub enum HandlerMessage {
  NewConnTh(Sender<Option<(Token,StreamInfo,MioTcpStream)>>, MioTcpStream),
  NewConnCo(Sender<Option<(Token,StreamInfo,MioTcpStream)>>, MioTcpStream),
  Timeout(Token),
  // get back the socket
  EndRead(Token,ReadTcpStream),
  //StartTimeout(Token),
}

impl ReadTcpStream {
/*  fn get_stream(&mut self) -> &mut MioTcpStream {
    match self {
      &mut ReadTcpStream::Threaded(ref mut s,_,_,_,_) => s,
      &mut ReadTcpStream::CoRout(ref mut s,_,_) => s,
    }
  }*/
}

impl Transport for Tcp {
  type ReadStream = ReadTcpStream;
  type WriteStream = WriteTcpStream;
  type Address = SocketAddr;

  /// if spawn we do not loop (otherwhise we should not event loop at all), the thread is just for
  /// one message.
  fn do_spawn_rec(&self) -> SpawnRecMode {
    if self.threaded {
      if self.spawn {
        SpawnRecMode::LocalSpawn
      } else {
        SpawnRecMode::Local
      }
    } else {
      SpawnRecMode::Coroutine
    }
  }

  fn start<C> (&self, readhandler : C) -> Result<()>
    where C : Fn(ReadTcpStream,Option<WriteTcpStream>) -> Result<ReaderHandle> {
//    tcplistener.0.set_write_timeout_ms(self.timeout.num_milliseconds());


    // default event loop TODO maybe config in Tcp ??
    // No timeout to
    let mut eventloop = try!(EventLoop::new());
    let sender = eventloop.channel();
    utils::ret_one_result(&self.channel, Some(sender.clone()));
    try!(eventloop.register(&self.listener, Token(CONN_REC)));
    try!(eventloop.run(&mut MyHandler {
      keepalive : self.keepalive.map(|d|d.num_seconds().to_u32().unwrap()),
      tokens : VecMap::new(),
      socks : VecMap::new(),
      timeout : self.timeout.num_milliseconds().to_u32().unwrap(),
      listener : &self.listener,
      sender : sender, 
      readhandler : readhandler,
      threaded : self.threaded,
      spawn : self.spawn,
      }));
    Ok(())
  }

  fn connectwith(&self,  p : &SocketAddr, timeout : Duration) -> IoResult<(Self::WriteStream, Option<Self::ReadStream>)> {
    let s = try!(MioTcpStream::connect(p));
    try!(s.set_keepalive (self.keepalive.map(|d|d.num_seconds().to_u32().unwrap())));
    let ws = try!(s.try_clone());
//    try!((s.0).0.set_write_timeout_ms(self.timeout.num_milliseconds().to_usize().unwrap()));
    //try!(s.0.set_read_timeout_ms(self.timeout.num_milliseconds().to_usize().unwrap()));
    
    // get token from event loop!!
   let (sch,sync) = mpsc::channel();
   let ch = {
    let r = match self.channel.0.lock() {
      Ok(guard) => {
        if guard.0.is_none() {
          let mut rguard = guard;
          loop {
            match self.channel.1.wait(rguard) {
              Ok(mut r) => {
                if r.1 {
                  r.1 = false;
                  rguard = r;
                  break;
                } else {
                  debug!("spurious wakeout");
                  rguard = r;
                };
              },
              Err(_) => {error!("Condvar issue for return res"); 
                return Err(IoError::new(IoErrorKind::Other, "condvar issue in connect"));
              },
            }
          };
          rguard
        } else {
          guard
        }
      },
      Err(e) => {error!("poisonned mutex on one res {}", e);
        return Err(IoError::new(IoErrorKind::Other, "condvar issue in connect"));
      },
   };
   match r.0 {
        None => {
          // not initialized event loop,
          let msg = "Event loop of tcp loop not initialized, server process may not have start properly";
          error!("{}",msg);
          return Err(IoError::new(IoErrorKind::Other, msg));
        },
        Some(ref ch) => {
          if self.threaded {
            try!(to_iores(ch.send(HandlerMessage::NewConnTh(sch, s))));
            ch.clone()
          } else {
            try!(to_iores(ch.send(HandlerMessage::NewConnCo(sch, s))));
            ch.clone()
          }
        },
      }
    };
    // wait for token in condvar
    let oosi = sync.recv();
    let si = if oosi.is_err() {
      return Err(IoError::new(IoErrorKind::Other, "eventloop return no stream info, error occurs"));
    } else {
        oosi.unwrap()
    };
    match si {
      Some((tok,StreamInfo::Threaded(state),rs)) => {
        Ok((WriteTcpStream(ws),
        // TODO get tcpstream
          Some(ReadTcpStream::Threaded(Some(rs), tok, state,ch,self.timeout.num_milliseconds().to_u32().unwrap())
        )))
      },
      _ => {
        // no read stream returned : server already started as a coroutine by the transport
        // coroutine init ok
        Ok((WriteTcpStream(ws), None))
      },

    }
  }

  /// deregister the read stream for this address
  fn disconnect(&self, sock : &SocketAddr) -> IoResult<bool> {
    // TODO  deregister ReadStream : PB we do not have mapping address stream token -> TODO add
    // this mapping (costy...

    Ok(true)
  }

}


impl ReadTcpStream {
  fn read_th(buf: &mut [u8], s : &mut Option<MioTcpStream>, tok : &Token, or : &OneResult<ThreadState>, ch : &MioSender<HandlerMessage>, to : &u32) -> IoResult<usize> {
    // on read start if state is "nothing to read", start timeout and init condvar
  if let Some(mut s) = s.as_mut() {
  let (retry, nb) = match or.0.lock() {
    Ok(mut guard) => {
      let nb = match s.read(buf) {
        Ok(n) => n,
        Err(e) => {
          let rawerr = e.raw_os_error();
          // 11 as temporaryly unavalable
          if rawerr == Some(11) {
            0
          } else {
            return Err(e);
          }
        },
      };
      if nb == 0 {
        let mut res = (false,0);
        loop {
          guard = match or.1.wait_timeout_ms(guard, *to) {
            Ok(mut r) => {
              if !r.1 || (r.0).1 { // timeout or marker changed
                (r.0).1 = false;
                res = (r.1,0);
                break;
              } else {
                debug!("spurious wakeout");
              };
              r.0
            },
            Err(_) => {
              error!("Condvar issue for return res"); 
              break;
            },
          };
        };
        res
      } else {
        (false,nb)
      }
    },
    Err(e) => {error!("poisonned mutex on one res : {}",e); (false,0)},
  };
  if !retry && nb == 0 {
        try!(to_iores(ch.send(HandlerMessage::Timeout(tok.clone()))));
        Err(IoError::new(IoErrorKind::Other, "time out on transport reception"))
  } else {
    if retry {
      s.read(buf) // TODO useless?? retry from a read 0
    } else {
      Ok(nb)
    }
  }

  } else {
    panic!("trying to read on closed read tcp stream : this is a bug");
  }
  }

  fn read_co(buf: &mut [u8], s : &mut MioTcpStream, tok : &Token, ch : &MioSender<HandlerMessage>) -> IoResult<usize> {
    // TODO no timeout mechanism : coroutine could easily get stuck !!!!
    Coroutine::sched();
    Ok(try!(s.read(buf)))
 
  }

}

impl Read for ReadTcpStream {
  fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
    match self {
      &mut ReadTcpStream::Threaded(ref mut s,ref tok, ref or, ref ch, ref to) => ReadTcpStream::read_th(buf,s,tok,or,ch,to),
      &mut ReadTcpStream::CoRout(ref mut s,ref tok, ref ch, ref to) =>  ReadTcpStream::read_co(buf,s,tok,ch),
    }
  }
}
impl ReadTransportStream for ReadTcpStream {

  fn end_read_msg(&mut self) -> () {
    // send reader back to loop
     let rs = self.end_clone();
     match self {
      &mut ReadTcpStream::Threaded(_,ref tok, _, ref ch, _) => {
        match ch.send(HandlerMessage::EndRead(tok.clone(), rs)) {
          Ok(_) => (),
          Err(e) => error!("end read msg of a tcp loop failed due to channel issue : {:?}", e),
        };
      },
      _ => panic!("TODO"),
     }
    
  }
  fn disconnect(&mut self) -> IoResult<()> {
    match self {
      &mut ReadTcpStream::Threaded(ref mut s,ref tok, ref or, ref ch, ref to) => {
    // TODO send token (plus address for mapping table) Disconnect in channel and from msg deregister and remove infos
      },
      &mut ReadTcpStream::CoRout(ref mut s,ref tok, ref ch, ref to) =>  {
        //TODO howto ???
      },
    };
    Ok(())
  }
 
  /// no loop (already event loop)
  fn rec_end_condition(&self) -> bool {
    match self {
      &ReadTcpStream::Threaded(..) => true, // change state is done in end_read_msg TODO mode where we read n messages... bef exit
      &ReadTcpStream::CoRout(..) => false, // corout is kept
    }
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

#[cfg(feature="with-extra-test")]
#[test]
fn connect_rw () {
  let start_port = 40000;

  let a1 = SocketAddrExt(sa4(Ipv4Addr::new(127,0,0,1), start_port));
  let a2 = SocketAddrExt(sa4(Ipv4Addr::new(127,0,0,1), start_port+1));
  let tcp_transport_1 : Tcp = Tcp::new (&a1, Some(Duration::seconds(5)), Duration::seconds(5), true, false).unwrap();
  let tcp_transport_2 : Tcp = Tcp::new (&a2, Some(Duration::seconds(5)), Duration::seconds(5), true, false).unwrap();
  // TODO test with spawn

  connect_rw_with_optional(tcp_transport_1,tcp_transport_2,&a1,&a2,true);
}


#[cfg(feature="with-extra-test")]
#[test]
fn connect_rw_corout () {
  let start_port = 40000;

  let a1 = SocketAddrExt(sa4(Ipv4Addr::new(127,0,0,1), start_port));
  let a2 = SocketAddrExt(sa4(Ipv4Addr::new(127,0,0,1), start_port+1));
  let tcp_transport_1 : Tcp = Tcp::new (&a1, Some(Duration::seconds(5)), Duration::seconds(5), false, false).unwrap();
  let tcp_transport_2 : Tcp = Tcp::new (&a2, Some(Duration::seconds(5)), Duration::seconds(5), false, false).unwrap();
  // TODO need a test variant for coroutine (returning corouting handle and using no thread)

}


fn to_iores<A,Error : Debug>(e : StdResult<A,Error>) -> IoResult<A> {
  match e {
    Ok(a) => Ok(a),
    Err(e) => {
      let msg = format!("Io error from non Io error : {:?}",e);
      Err(IoError::new(IoErrorKind::Other, msg))
    },
  }
}
