//! Receiving process
//! It is the only process which receive messages (except proxy mode where we could wait for
//! messages in `client` process and ping / pong).
//! With connected transport, a thread is used to receive queries from peers, with non connected
//! transport a thread receive globally spawning short lives process to run server job.
//! TODO clienthandle should be more used, and especially added to query when possible
//! TODO bool to know if auth ok (do not run find or store if not auth (pb with asynch : need to
//! send message after ping and not at the same time)



use procs::mesgs::{PeerMgmtInitMessage,PeerMgmtMessage,KVStoreMgmtMessage,QueryMgmtMessage,ServerPoolMessage,ClientMessage};
use msgenc::{ProtoMessage};
use procs::{ArcRunningContext,RunningProcesses,RunningTypes};
use peer::{Peer,PeerMgmtMeths,PeerPriority,PeerStateChange};
use transport::ReadTransportStream;
use transport::Transport;
use std::sync::{Mutex,Arc};
use std::sync::mpsc::{Sender};
use std::sync::mpsc;
use std::thread;
use query::{QueryMsg,QueryModeMsg};
use rules::DHTRules;
use time::Duration;
use query;
//use query::{QueryID};
//use utils;
use keyval::{SettableAttachment};
//use keyval::{Attachment,SettableAttachment};
use keyval::{KeyVal,Attachment};
use utils::{receive_msg};
use num::traits::ToPrimitive;
use std::io::Result as IoResult;
use mydhtresult::Result as MDHTResult;
use procs::ServerMode;
use procs::ClientHandle;
use route::ServerInfo;

/// either we created a permanent thread for server, or it is managed otherwhise (for instance udp
/// single reception loop or tcp evented loop).
pub enum ServerHandle<P : Peer, TR : ReadTransportStream> {
  /// loop model without message listener
  ThreadedOne(Arc<Mutex<bool>>),
  /// pool mode with some poll on message listener
  Mult(usize,Sender<ServerPoolMessage<P,TR>>),
  /// Local no loop nothing to do
  Local,
}
impl<P : Peer, T : ReadTransportStream> Clone for ServerHandle<P,T> {
  fn clone(&self) ->  ServerHandle<P,T> {
    match self {
     &ServerHandle::ThreadedOne(ref amut) => ServerHandle::ThreadedOne(amut.clone()),
     &ServerHandle::Mult(ref ix, ref sndr) => ServerHandle::Mult(ix.clone(),sndr.clone()),
     &ServerHandle::Local => ServerHandle::Local,
    }
  }
}
pub fn serverinfo_from_handle<P : Peer, TR : ReadTransportStream> (h : &ServerHandle<P,TR>) -> ServerInfo {
  match *h {
    ServerHandle::Local => ServerInfo::TransportManaged,
    ServerHandle::ThreadedOne(ref amut) => ServerInfo::Threaded(amut.clone()),
    ServerHandle::Mult(_,_) => panic!("multiplecx server not yet implemeted"),
  }
}

// all update of query prio and nbquery and else are not done every where : it is a security issue
/// all update of query conf when receiving something, this is a filter to avoid invalid or network
/// dangerous queries (we apply local conf other requested conf)
pub fn update_query_conf<P : Peer, R : DHTRules> (qconf  : &mut QueryMsg<P>, r : &R) {
  // ensure number of query is not changed
  qconf.nb_forw = r.nbquery(qconf.nb_forw); // TODO special function for when hoping?? with initial nbquer in param : eg less and less??
    // TODO add a treshold for remhop
  //  let newremhop = remhop;
    // TODO a treshold for number of res
 //   let newrnbres = rnbres;
    // TODO priority lower function (no need to lower nbquer : only treshold and lower prio)
//    let newqp = qp;
}

/// new query mode to use when proxying a query (not for proxy mode and asynch mode where nothing
/// change).
/// The query id is set to 0 (unset) and may need to be replace by querymanager
fn new_query_mode<P : Peer> (qm : &QueryModeMsg<P>, me : &Arc<P>) -> QueryModeMsg<P> {
   match qm {
     &QueryModeMsg::AMix(0, _, _)  => 
       QueryModeMsg::Asynch(me.clone(), 0),
     &QueryModeMsg::AMix(i, _, _)  => 
       QueryModeMsg::AMix(i - 1, me.clone(), 0),
     &QueryModeMsg::AProxy(_, _)  => 
       QueryModeMsg::AProxy(me.clone(), 0),
     ref a => {
       // should not happen
       error!("unexpected query mode has been received, forwarding : {:?}",a);
       (*a).clone()
     },
   }
}

/// fn to define server mode depending on what transport allows and DHTRules
#[inline]
pub fn resolve_server_mode <RT : RunningTypes> (rc : &ArcRunningContext<RT>) -> ServerMode {
  let (max, pool, timeoutmul, otimeout) = rc.rules.server_mode_conf();
  match rc.transport.do_spawn_rec() {
      // spawn and managed eg tcp
      (true,true) => {
        if max > 1 {
          ServerMode::ThreadedMax(max,timeoutmul)
        } else if pool > 1 {
          ServerMode::ThreadedPool(pool,timeoutmul)
        } else {
          ServerMode::ThreadedOne(otimeout)
        }
      },
      // spawn in non managed
      (true,false) => {
        if max > 1 {
          ServerMode::LocalMax(max)
        } else if pool > 1 {
          ServerMode::LocalPool(pool)
        } else {
          ServerMode::Local(true)
        }
      },
      // non managed no spawn
      (false,false) => {
        ServerMode::Local(false)
      },
      _ => panic!("Invalid transport definition"),
  }
}
// fn to start a server process out of transport reception (when connect with of transport return a
// reader to.
pub fn start_listener <RT : RunningTypes>
 (s : <RT::T as Transport>::ReadStream, 
  rc : &ArcRunningContext<RT>, 
  rp : &RunningProcesses<RT>,
 ) -> IoResult<ServerHandle<RT::P,<RT::T as Transport>::ReadStream>>  {
  let servermode = resolve_server_mode (&rc); 
  match servermode {
    ServerMode::ThreadedOne(otimeout) => {
      let rp_thread = rp.clone();
      let rc_thread = rc.clone();
      let amut = Arc::new(Mutex::new(false));
      let sh = ServerHandle::ThreadedOne(amut);
      let sh_thread = sh.clone();
      thread::spawn (move || {
        request_handler::<RT> (s, &rc_thread, &rp_thread, None, sh_thread, otimeout,false);
      });
      Ok(sh)
    },
    ServerMode::Local(_) => {
      error!("Local server process start from something else than transport is a library bug or incoherent transport");
      panic!("Local server process start from something else than transport is a library bug or incoherent transport");
    },
    _ => panic!("TODO impl mult"),
  }
}


/// Server loop
pub fn servloop <RT : RunningTypes>
 (rc : ArcRunningContext<RT>, 
  rp : RunningProcesses<RT>) -> MDHTResult<()> {

    let servermode = resolve_server_mode (&rc); 
 
    let spserv = |s, ows| {
      let mut res = ();
      match servermode {
        ServerMode::ThreadedOne(otimeout) => {
          let rp_thread = rp.clone();
          let rc_thread = rc.clone();
          thread::spawn (move || {
            let amut = Arc::new(Mutex::new(false));
            request_handler::<RT>(s, &rc_thread, &rp_thread, ows, ServerHandle::ThreadedOne(amut), otimeout,false);
          });
        },
        ServerMode::Local(true) => {
          let rp_thread = rp.clone();
          let rc_thread = rc.clone();
          thread::spawn (move || {
            request_handler::<RT>(s, &rc_thread, &rp_thread, ows, ServerHandle::Local, None, false);
          });
        },
        ServerMode::Local(false) => {
          res = try!(request_handler::<RT>(s, &rc, &rp, ows, ServerHandle::Local, None, false));
        },
        _ => panic!("Server Mult mode are unimplemented!!!"),//TODO mult thread mgmt
      };
      Ok(res)
    };
    // loop in transport receive function TODO if looping : start it from PeerManager thread
    rc.transport.start(spserv)
}


/// Spawn thread either for one message (non connected) or for one connected peer (loop on stream
/// receiver).
///
/// TODO add if managed as parameter and if managed send handle (mutex on close) to peermgmt
/// TODO add ows to send writer if we receive a message with a node (switch to none when sent),
/// peeradd msg with this ows.
fn request_handler <RT : RunningTypes>
 (mut s1 : <RT::T as Transport>::ReadStream, 
  rc : &ArcRunningContext<RT>, 
  rp : &RunningProcesses<RT>,
  mut ows : Option<<RT::T as Transport>::WriteStream>,
  shandle : ServerHandle<RT::P, <RT::T as Transport>::ReadStream>,
  otimeout : Option<Duration>,
  mult : bool,
 ) -> MDHTResult<()>  {

  let managed = match shandle {
    ServerHandle::ThreadedOne(_) => true,
    // TODO mult
    _ => false,
  };
  otimeout.map(|timeout| {
    error!("timeout in receiving thread is currently unimplemented {}", timeout);
    // TODO start thread with sleep over mutex to close !!!!
    ()
  });
  // current connected peer
  let mut op : Option<Arc<RT::P>> = None;
  // used to skip peermanager on established connections
  //let mut clihandles : Vec<ClientHandle<RT::P,RT::V>> = vec!();
  let mut chandle = None;
  let mut doloop = true;
  while doloop {
    // r is message and oa an optional attachmnet (in pair because a reference owhen disconnected
    // and not when connected).
    let s = if !mult {
      &mut s1
    } else {
      // TODO update op value from table
      panic!("multip server not implemented");
    };
    let (r, oa) = match receive_msg (s, &rc.msgenc) {
      None => {
        info!("Serving connection lost or malformed message");
        op.as_ref().map(|p|{
          debug!("Sending remove message to peermgmt");
          // TODO on malformed message send blocked instead!!
          //rp.peers.send(PeerMgmtMessage::PeerRemFromServer(p.clone(),PeerStateChange::Blocked));
          rp.peers.send(PeerMgmtMessage::PeerRemFromServer(p.clone(),PeerStateChange::Offline))
        }).is_none();

        // TODO different result : warn on malformed message 
        break;
      },
      Some(r) => r,
    };
    doloop = request_handler_internal(rc,rp,ows,&shandle,r,oa,&mut op, &mut chandle,managed).unwrap_or(false);
    ows = None;

    if !managed {
      doloop = false;
    } else {
      // TODO not for all mult
      match shandle {
        ServerHandle::ThreadedOne(ref mutstop) => {
          match mutstop.lock() {
            Ok(res) => if *res == true {
              doloop = false;
            },
            Err(_) => error!("poisoned mutex for ping result"),
          };
 
        },
        _ => (),
      }
    };
    // TODO if not oneonly (reader is managed and persistent) 
    // exit condition + exit mutex + send event to peermanager( subsequent close write ) +
    // exit on timeout
    // else local disconnect and through event to peermanager (close write). TODO if doloop false
    // and mult, simply rem current s and stop loop if no more s (depending on mode).
  };
  Ok(())
}

/// return io error of a message treatment,
/// and true if it should loop
fn request_handler_internal <RT : RunningTypes>
 (rc : &ArcRunningContext<RT>, 
  rp : &RunningProcesses<RT>,
  mut ows : Option<<RT::T as Transport>::WriteStream>,
  shandle : &ServerHandle<RT::P, <RT::T as Transport>::ReadStream>,
  r : ProtoMessage<RT::P,RT::V>,
  oa : Option<Attachment>,
  op : &mut Option<Arc<RT::P>>,
  chandle : &mut Option<ClientHandle<RT::P,RT::V>>,
  managed : bool,
 ) -> MDHTResult<bool>  {

  debug!("{:?} RECEIVED : {:?}", rc.me, r);
  match r {

    ProtoMessage::PING(from, chal, sig) => {
      // check ping authenticity
      if rc.peerrules.checkmsg(&from,&chal,&sig) {

        // TODO move after accept TODO heavy
        let do_accept = !rc.rules.is_accept_heavy();
        let accept = if do_accept {
          rc.peerrules.accept(&from, &rp, &rc)
        } else {
          Some(PeerPriority::Unchecked)
        };
        let afrom = Arc::new(from);
        match accept {
          None => {
            warn!("refused node {:?}",afrom);
            try!(rp.peers.send(PeerMgmtMessage::PeerRemFromServer(afrom, PeerStateChange::Refused)));
            return Ok(false);
          },
          Some(pri)=> {
            let mut newhandle = false;
            // init clihandle
            if chandle.is_none() && managed {
              let (tx,rx) = mpsc::sync_channel(1);
              rp.peers.send(PeerMgmtMessage::PeerAddFromServer(afrom.clone(), pri.clone(), ows, tx, serverinfo_from_handle(shandle)));
              ows = None; 
              *chandle = match rx.recv(){ // TODO !!! asynch chandle reception (message order ensure add is done where peerpong)
                Ok(ch) => {
                  newhandle = true;
                  Some(ch)
                },
                Err(_) => {
                  // error in cli then drop of sender
                  warn!("No cli handle for ping in server");
                  None
                },
              };
            };

            let sendinhandle = !newhandle && match chandle {
              &mut Some(ref h) => {
                // init for looping
//                if !mult {
                *op = Some(afrom.clone());
//                } else {
//                  panic!("mult serv not imp");
//                };
                // send pong
                let mess = ClientMessage::PeerPong(chal.clone());
                h.send_msg(mess).unwrap_or(false)
              },
              &mut None => false,
            };
            if !sendinhandle {
              // chandle is none and not managed
              rp.peers.send(PeerMgmtMessage::PeerPong(afrom.clone(),pri,chal,ows));
              ows = None;
            };

            // hook
            if do_accept {
              rc.peerrules.for_accept_ping(&afrom, &rp, &rc);
            };
          },
        };
      } else {
        warn!("invalid ping, closing connection");
        // TODO status change??
        if let Some(p) = (*op).as_ref() {
          debug!("Sending remove message to peermgmt");
          rp.peers.send(PeerMgmtMessage::PeerRemFromServer((*p).clone(),PeerStateChange::Blocked));
        };

        return Ok(false);
      }

    },
    ProtoMessage::PONG(node,sign) => {
      // check node is emitter for managed receive
      if let Some(p) = (*op).as_ref() {
        debug!("checking pong node");
        if p.get_key() != node.get_key() {
          rp.peers.send(PeerMgmtMessage::PeerRemFromServer(p.clone(),PeerStateChange::Blocked));
          return Ok(false);
        }
      };



      // challenge is stored in peermanager peer status.
      // So we just forward that to peermanager (we cannot check here).
      // Peermanager will also spawn accept if it is heavy accept
      // ows may be send in case where sending of ping does not establish a cli connection or
      // when this one should override previous connection for ping TODO check in server to
      // change state of connection and for heavy accept run a oneresult (state of serv is either
      // one result plus stack of waiting msg (in transport) or bool ...
      rp.peers.send(PeerMgmtMessage::PeerAuth(node,sign));
    },
    // asynch mode we do not need to keep trace of the query
    ProtoMessage::FIND_NODE(mut qconf@QueryMsg{ modeinfo : QueryModeMsg::Asynch(..),..}, nid) => {
      update_query_conf (&mut qconf ,&rc.rules);
      debug!("Asynch Find peer {:?}", nid);
      rp.peers.send(PeerMgmtMessage::PeerFind(nid,None,qconf));
    },
    // general case as asynch waiting for reply
    ProtoMessage::FIND_NODE(mut qconf, nid) => {
      let old_qconf = qconf.clone();
      update_query_conf (&mut qconf ,&rc.rules);
      let nbquer = qconf.nb_forw;
      let qp = qconf.prio;
      // TODO server query initialization is not really efficient it should be done after local
      debug!("Proxying Find peer {:?}", nid);
      let lifetime = rc.rules.lifetime(qp); // TODO special lifetime when hoping?
      qconf.modeinfo = new_query_mode (&qconf.modeinfo, &rc.me);
      // Warning here is old qmode stored in conf new mode is for proxied query
      let query : query::Query<RT::P,RT::V> = query::init_query(nbquer.to_usize().unwrap(), 1, lifetime, Some(old_qconf), None);
      debug!("Asynch Find peer {:?}", nid);
      // warn here is new qmode
      // it is managed : send to querycache (qid (init here to 0) and query cache)
      rp.queries.send(QueryMgmtMessage::NewQuery(query, PeerMgmtInitMessage::PeerFind(nid, qconf)));
    },
    // particular case for asynch where we skip node during reply process
    ProtoMessage::FIND_VALUE(mut qconf@QueryMsg{ modeinfo : QueryModeMsg::Asynch(..), ..}, nid) => {
      update_query_conf (&mut qconf ,&rc.rules);
      debug!("Asynch Find val {:?}", nid);
      rp.store.send(KVStoreMgmtMessage::KVFind(nid,None,qconf,false));
    },
    // general case as asynch waiting for reply
    ProtoMessage::FIND_VALUE(mut queryconf, nid) => {
      let old_qconf = queryconf.clone();
      let oldhop = queryconf.rem_hop; // Warn no set of this value
      let oldqp = queryconf.prio;
      update_query_conf (&mut queryconf ,&rc.rules);
      let qp = queryconf.prio;
      let sprio = queryconf.storage;
      let nb_req = queryconf.nb_res;
      debug!("Proxying Find value {:?}", nid);
      let lifetime = rc.rules.lifetime(qp); // TODO special lifetime when hoping
      // TODO same thing for prio that is 
      queryconf.modeinfo = new_query_mode (&queryconf.modeinfo, &rc.me);
      let esthop = (rc.rules.nbhop(oldqp) - oldhop).to_usize().unwrap();
      let store = rc.rules.do_store(false, qp, sprio, Some(esthop)); // first hop
      // Warning here is old qmode stored in conf new mode is for proxied query
      let query : query::Query<RT::P,RT::V> = query::init_query(queryconf.nb_forw.to_usize().unwrap(), nb_req, lifetime, Some(old_qconf), Some(store));

      debug!("Asynch Find val {:?}", nid);
      rp.store.send(KVStoreMgmtMessage::KVFind(nid,Some(query.clone()),queryconf,true));

    },
    // store node receive by server is asynch reply
    ProtoMessage::STORE_NODE(oqid, sre) => match oqid {
      Some(qid) => {
        let res = sre.map(|r|Arc::new(r.0));
        debug!("node store {:?} for {:?}", res, qid);

   // we update query, query then send new peer to peermanager with a ping request
   // (peeraddoffline)
            
            // TODO optional ping before query by sending queryid to peermanager to and peerman
            // send it back to query on pong
        rp.queries.send(QueryMgmtMessage::NewReply(qid, (res,None)));
      },
      None => {
        // TODO implement propagate!!
        error!("receive store node for non stored query mode : TODO implement propagate");
      },
    },
    ProtoMessage::STORE_VALUE_ATT(oqid, sre) => match oqid {
      Some(qid) => {
        let res = sre.map(|r|r.0);
        debug!("node store {:?} for {:?}", res, qid);
        rp.queries.send(QueryMgmtMessage::NewReply(qid, (None,res)));
      },
      None => {
        // TODO implement propagate!!
        error!("receive store node for non stored query mode : TODO implement propagate");
      },
    },
    ProtoMessage::STORE_VALUE(oqid, sre)=> match oqid {
      Some(qid) => {
        let res = match sre {
          Some(r) => {
            let mut res = r.0;
            // get attachment from transport response
            match oa {
              Some(ref at) => {
                if !res.set_attachment(at){
                  error!("error setting an attachment")
                };
              },
              _ => {
                error!("no attachment for store value att");
              },
            }
            Some(res)
          },
          None => {
            // TODO check unused attachment at least remove file
            None
          },
        };
        rp.queries.send(QueryMgmtMessage::NewReply(qid, (None,res)));
      },
      None => {
        // TODO implement propagate!!
        error!("receive store node for non stored query mode : TODO implement propagate");
      },
    },
    //u => error!("Unmanaged query to serving process : {:?}", u),
  };
  Ok(true)
}
