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


//! TODO refactor all, even reader start is not done (even register token(0) for inc connect)
//! plus remove timeout


extern crate byteorder;
extern crate mio;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver,Sender};
use std::thread;
use std::mem;
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
use self::mio::Sender as MioSender;
use self::mio::Handler;
use self::mio::PollOpt;
use self::mio::tcp::Shutdown;
use super::Attachment;
use num::traits::ToPrimitive;
use std::collections::VecMap;
use std::sync::Mutex;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::PoisonError;
use std::error::Error;
use utils::{OneResult,new_oneresult,ret_one_result,clone_wait_one_result,clone_wait_one_result_ifneq_timeout_ms,clone_wait_one_result_timeout_ms,one_result_val_clone,change_one_result,one_result_spurious};
use std::os::unix::io::AsRawFd;
use std::os::unix::io::FromRawFd;
#[cfg(feature="with-extra-test")]
#[cfg(test)]
use transport::test::connect_rw_with_optional;
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
  // other thread). Note that could be feature gate to avoid enum overhead.
  spawn : bool,
  
  // usage of mutex is not so good yet it is only used on connection init
  // TODO may be able to remove option (new init)
  channel : OneResult<Option<MioSender<HandlerMessage>>>,
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
      channel : new_oneresult(None),
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


/// tread state : true if something to read (true of spurious wakeout)
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
pub enum ReadTcpStream {
  Threaded(Option<MioTcpStream>,Token, OneResult<ThreadState>,MioSender<HandlerMessage>,u32),
  CoRout(MioTcpStream,Token,MioSender<HandlerMessage>),
}

impl ReadTcpStream {
  /// end stream returning a new with handle of tcpstream (the one who was registerd
   fn end_clone(&mut self) -> Self {
     let st = match self {
       &mut ReadTcpStream::Threaded(ref mut st, _, _,_,_) => {
         mem::replace (st, None)
       },
       &mut ReadTcpStream::CoRout(..) => {
         panic!("TODO");
       },
     };
     match self {
       &mut ReadTcpStream::Threaded(_, ref tok, ref or, ref sm, ref to) => {
         ReadTcpStream::Threaded(st, tok.clone(), or.clone(), sm.clone(), to.clone())
       },
       &mut ReadTcpStream::CoRout( ref st, ref tok, ref sm) => {
         panic!("TODO");
       },
     }
 
   }
}

struct MyHandler<'a, C>
    where C : Fn(ReadTcpStream,Option<WriteTcpStream>) -> IoResult<()> {

  keepalive : Option<u32>,
  /// info about stream (for unlocking read)
  tokens : VecMap<StreamInfo>,
  /// sockets between read msg (added on end_read_msg
  socks : VecMap<ReadTcpStream>,
  timeout : u32,
  listener : &'a TcpListener,
  sender : MioSender<HandlerMessage>,
  readhandler : C,
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


  /// Message for handling a timeout or adding a connection
  fn notify(&mut self, event_loop: &mut EventLoop<Self>, msg: HandlerMessage) {
    match msg {
      HandlerMessage::EndRead(tok, rs) => {
        match self.tokens.get(&tok.0) {
          Some(&StreamInfo::Threaded(ref state)) => {
            let cont = one_result_spurious(state);
            if cont.unwrap() {
              (self.readhandler)(rs, None);
            } else {
              self.socks.insert(tok.0,rs);
            }
          },
          Some(&StreamInfo::CoRout(..)) => {
            panic!("TODO impl");
          },
          None => (),
        }
      },
      HandlerMessage::NewConnTh(r, readst) => {
        let tok = self.new_token();
        // we use only spurious wakeout bool
        let sync = new_oneresult(());
        // TODO error management
        event_loop.register_opt(&readst, tok, EventSet::readable(), PollOpt::edge());
        let si = StreamInfo::Threaded(sync.clone());
        self.tokens.insert(tok.as_usize(), si.clone());
        // TODO add read stream in ret result
        r.send((tok,si,readst));
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
            Ok(Some(mut s))  => {
              debug!("Initiating socket exchange : ");
              debug!("  - From {:?}", s.local_addr());
              debug!("  - From {:?}", s.peer_addr());
              debug!("  - With {:?}", s.peer_addr());
              let tok = self.new_token();
              //let sync = Arc::new((Mutex::new(ThreadState::Init),Condvar::new()));
              let state = new_oneresult(());
              // TODO error management
              match event_loop.register_opt(&s, tok, EventSet::readable(), PollOpt::edge()) {
                Err(e) => {
                  error!("tcp loop register of socket failed when connecting {:?}",&e);
                },
                Ok(()) => {
                  let si = StreamInfo::Threaded(state.clone());
                  self.tokens.insert(tok.as_usize(), si.clone());
                },
              };
              let sw = s.try_clone().unwrap();
              let ows = Some(WriteTcpStream(sw));
              // TODO error mgmt
              s.set_keepalive (self.keepalive);
              // start a receive even if no message : necessary to send ows (warn likely to timeout (or not generally auth right after connect TODO when noauth a ping frame must be send))
              let rs = ReadTcpStream::Threaded(Some(s), tok, state,self.sender.clone(),self.timeout.clone());
              (self.readhandler)(rs, ows);
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
            (self.readhandler)(rs, None);
          },
          None => {
            ret_one_result(state, ());
          },
        };
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
  NewConnTh(Sender<(Token,StreamInfo,MioTcpStream)>, MioTcpStream),
  NewConnCo(OneResult<Option<(Token,StreamInfo)>>),
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
  fn start<C> (&self, readhandler : C) -> IoResult<()>
    where C : Fn(Self::ReadStream,Option<Self::WriteStream>) -> IoResult<()> {
//    tcplistener.0.set_write_timeout_ms(self.timeout.num_milliseconds());


    // default event loop TODO maybe config in Tcp ??
    // No timeout to
    let mut eventloop = try!(EventLoop::new());
    let sender = eventloop.channel();
    ret_one_result(&self.channel, Some(sender.clone()));
    try!(eventloop.register(&self.listener, Token(CONN_REC)));
    eventloop.run(&mut MyHandler{
      keepalive : self.keepalive.map(|d|d.num_seconds().to_u32().unwrap()),
      tokens : VecMap::new(),
      socks : VecMap::new(),
      timeout : self.timeout.num_milliseconds().to_u32().unwrap(),
      listener : &self.listener,
      sender : sender, 
      readhandler : readhandler,
      })
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
      Ok(mut guard) => {
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
      Err(poisoned) => {error!("poisonned mutex on one res");
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
          if self.spawn {
            ch.send(HandlerMessage::NewConnTh(sch, s));
            ch.clone()
          } else {
            let sync = new_oneresult(None);
            ch.send(HandlerMessage::NewConnCo(sync.clone()));
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
    let tok = si.0;
    match si.1 {
      StreamInfo::Threaded(state) => {
        Ok((WriteTcpStream(ws),
        // TODO get tcpstream
          Some(ReadTcpStream::Threaded(Some(si.2), tok, state,ch,self.timeout.num_milliseconds().to_u32().unwrap())
        )))
      },
      StreamInfo::CoRout(_,_) => {
        // TODO same mechanism as threaded?? or separate match globally (s is consume in previous
        // match)
        let s = ws.try_clone().unwrap();
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
              if (r.0).1 {
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
    Err(poisoned) => {error!("poisonned mutex on one res"); (false,0)},
  }  ;
  if !retry && nb == 0 {
        ch.send(HandlerMessage::Timeout(tok.clone()));
        Err(IoError::new(IoErrorKind::Other, "time out on transport reception"))
  } else {
    if retry {
      s.read(buf)
    } else {
      Ok(nb)
    }
  }

  } else {
    panic!("trying to read on closed read tcp stream : this is a bug");
  }
  }
  fn read_co(buf: &mut [u8], s : &mut MioTcpStream, tok : &Token, ch : &MioSender<HandlerMessage>) -> IoResult<usize> {
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

  fn end_read_msg(&mut self) -> () {
    // send reader back to loop
     let rs = self.end_clone();
     match self {
      &mut ReadTcpStream::Threaded(_,ref tok, _, ref ch, _) => {
        ch.send(HandlerMessage::EndRead(tok.clone(), rs));
      },
      _ => panic!("TODO"),
     }
    
  }
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
    // TODO change state??? : no done in end_read_msg TODO mode where we read n messages...
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

#[cfg(feature="with-extra-test")]
#[test]
fn connect_rw () {
  let start_port = 40000;

  let a1 = SocketAddrExt(sa4(Ipv4Addr::new(127,0,0,1), start_port));
  let a2 = SocketAddrExt(sa4(Ipv4Addr::new(127,0,0,1), start_port+1));
  let tcp_transport_1 : Tcp = Tcp::new (&a1, Some(Duration::seconds(5)), Duration::seconds(5), true).unwrap();
  let tcp_transport_2 : Tcp = Tcp::new (&a2, Some(Duration::seconds(5)), Duration::seconds(5), true).unwrap();

  connect_rw_with_optional(tcp_transport_1,tcp_transport_2,&a1,&a2,true);
}

