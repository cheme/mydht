use procs::mesgs::{PeerMgmtMessage,ClientMessage,ClientMessageIx,QueryMgmtMessage,PeerMgmtInitMessage};
use peer::{PeerMgmtMeths, PeerPriority,PeerState,PeerStateChange};
use std::sync::mpsc::{Receiver};
use procs::{client,ArcRunningContext,RunningProcesses,RunningTypes};
use std::collections::{VecDeque};
use std::sync::{Arc,Semaphore,Mutex};
use query::{LastSent,QueryMsg,QueryHandle,QueryModeMsg};
use route::Route;
use std::sync::mpsc::channel;
use time::Duration;
use std::thread;
use transport::{Transport};
use peer::Peer;
use utils::{self,OneResult,Either};
use keyval::{KeyVal};
use num::traits::ToPrimitive;
use procs::ClientMode;
use rules::DHTRules;
use route::{ClientInfo,ServerInfo};
use procs::server::{resolve_server_mode,serverinfo_from_handle};
use mydhtresult::Result as MydhtResult;
use route::{ClientSender};
use std::sync::mpsc::{Sender};
use procs::server::start_listener;
use procs::sphandler_res;
use utils::TransientOption;
use mydhtresult::{Error,ErrorKind};
use mydhtresult::Result as MDHTResult;
use bit_vec::BitVec;


/// TODO expect problem if pool of server managed by peermgmt & client local to peermgmt : client
/// create will wait for a reply by its same thread : Pool of server handle is needed in start
/// client param (direct if client local, sender if client threaded)
/// This type is close to either but it is deconstructed before use (so we do not have to implement
/// send or sync for local handle.
/// ClientPool does not require such handle as client is always started from peer manager
/// TODO Need a option server pool as parameter of start client. And a start distant fn wrapper
/// without this (could not be send to process).
/// TODO replace resend of message to ourselve by fn call  TODO rc.me.get_key() in a variable
/// (lotacall)
pub enum ServerPoolHandle<'a> {
  /// TODO replace by ServerPool type
  Local(&'a String),
  /// pool message in peermgmt for now, so use of sender of running process TODO see if othe pool process
  Threaded,
}


// peermanager manage communication with the storage : add remove update. The storage is therefore
// not shared, we use message passing. 
/// Start a new peermanager process
/// Return error on broken channel from rp (condition to kill dht).
pub fn start<RT : RunningTypes, 
  T : Route<RT::A,RT::P,RT::V,RT::T>,
  F : FnOnce() -> Option<T> + Send + 'static,
  >
 (rc : ArcRunningContext<RT>,
  routei : F, 
  r : &Receiver<PeerMgmtMessage<RT::P,RT::V,<RT::T as Transport>::ReadStream,<RT::T as Transport>::WriteStream>>, 
  rp : RunningProcesses<RT>
  ) -> MydhtResult<()> {
  let mut res = Ok(());
  let mut route = match routei() {
    Some(s) => s,
    None => {
      error!("route initialization failed");
      return Err(Error::new_from("route init failure".to_string(), ErrorKind::StoreError, res.err()));
    },
  };


  let clientmode = rc.rules.client_mode();
  let mut threadpool = ((0, BitVec::new()),Vec::new());
  let local = *clientmode == ClientMode::Local(true) || *clientmode == ClientMode::Local(false);
  let localspawn = *clientmode == ClientMode::Local(true);
  let servermode = resolve_server_mode (&rc);
  let heavyaccept = rc.rules.is_accept_heavy();
  loop {
    // TODO plug is auth false later (globally)
    let isauth = rc.rules.is_authenticated();
    let (peerheavy,queryheavy,poolheavy) = rc.rules.is_routing_heavy();
 
    match r.recv() {
      Ok(m) => {
        match peermanager_internal (&rc, &mut route, &mut threadpool, &rp, clientmode, heavyaccept, local, localspawn, m) {
          Ok(true) => (),
          Ok(false) => {
            break;
          },
          Err(e) => {
            res = Err(e);
            break;
          },
        }
      },
      Err(e) => {
        res = Err(Error::from(e));
        break;
      }
    };
  };
  if !route.commit_store() {
    res = Err(Error::new_from("peer store commit failure".to_string(), ErrorKind::StoreError, res.err()));
  };
  res
}

fn peermanager_internal<RT : RunningTypes, 
  T : Route<RT::A,RT::P,RT::V,RT::T>,
  >
 (rc : &ArcRunningContext<RT>,
  route : &mut T,
  threadpool : &mut ThreadPool<RT>,
  rp : &RunningProcesses<RT>,
  clientmode : &ClientMode,
  heavyaccept : bool,
  local : bool,
  localspawn : bool,
  r : PeerMgmtMessage<RT::P,RT::V,<RT::T as Transport>::ReadStream,<RT::T as Transport>::WriteStream>,
  ) -> MydhtResult<bool> {
  match r {
    PeerMgmtMessage::PeerAuth(node,sign) => {
      let k = node.get_key();
      // pong received in server
      let onenewprio = match route.get_node(&k) {
        Some(ref p) => {
          match p.1 {
            PeerState::Ping(ref chal, ref ores, ref prio) => {
              // check auth with peer challenge
              if rc.peerrules.checkmsg(&(*p.0),&chal[..],&sign) {
                if heavyaccept {
                // spawn accept if it is heavy accept to get prio (prio stored in ping is
                // unchecked)
                  assert!(*prio == PeerPriority::Unchecked);
                  let rcthread = rc.clone();
                  let rpthread = rp.clone();
                  let oresth = ores.0.clone();
                  thread::spawn(move ||{
                    let oprio = rcthread.peerrules.accept(&node, &rpthread, &rcthread);
                    let afrom = Arc::new(node);
                    match oprio {
                      Some(prio) => {
                        oresth.map(|res|utils::change_one_result(&res, true)).is_some();
                        sphandler_res(rpthread.peers.send(PeerMgmtMessage::PeerUpdatePrio(afrom.clone(),PeerState::Online(prio))));
                      },
                      None => {
                        // from client & from server to close both TODO a message for both?? (or
                        // shared message)
                        sphandler_res(rpthread.peers.send(PeerMgmtMessage::PeerRemFromClient(afrom.clone(), PeerStateChange::Refused)));
                        sphandler_res(rpthread.peers.send(PeerMgmtMessage::PeerRemFromServer(afrom.clone(), PeerStateChange::Refused)));
                        // useless : already in drop of peerping state
                        // oresthred.0.map(|ref res|utils::ret_one_result(&res, false)).is_some();
                      },
                    };
                    // hook
                    rcthread.peerrules.for_accept_ping(&afrom, &rpthread, &rcthread);
                  });
                  None
                } else {
                  // set result to true (do not notify done at drop of peerping (when prio is
                  // updated))
                  ores.0.as_ref().map(|res|utils::change_one_result(res, true)).is_some();
                  Some((PeerState::Online(prio.clone()),false))
                }
              } else {
                // useless : already in drop !!! 
                // ores.0.as_ref().map(|ref res|utils::ret_one_result(&res, false)).is_some();
                
                // wrong message : auth failure
                Some((PeerState::Blocked(prio.clone()),true))
              }
            },
            _ => {
              // bad peer state
              warn!("receive pong for non stored ping, ignoring");
              None
            }
          }
        },
        None => {
          // no peer
          warn!("receive pong for non existent peer, retrying ping");
          try!(rp.peers.send(PeerMgmtMessage::PeerPing(Arc::new(node),None)));
          None
        },
      };
      onenewprio.map(|(prio, rm)|{
        if rm {
          rem_peer(route, &k,rc,threadpool,clientmode,Some(prio),None);
        } else {
          route.update_priority(&k,Some(prio),None);
        };
      }).is_none();
    },
    PeerMgmtMessage::ClientMsg(msg, nodeid) => {
      match clisend(Either::Right(&nodeid), rc, route, rp, msg, localspawn, heavyaccept, local, clientmode,threadpool) {
        Ok(true) => debug!("cli msg proxied ok in peerman"),
        Ok(false) => warn!("cli msg proxied failure in peerman (missing peer)"),
        Err(e) => error!("send error in proxied : {}",e),
      };
    },
    PeerMgmtMessage::PeerRemFromClient(p,pschange) => {
      let nodeid = p.get_key().clone();
      // close all info
      rem_peer(route,&nodeid,rc,threadpool,clientmode,None,Some(pschange));
    },
    PeerMgmtMessage::PeerRemFromServer(p,pschange) => {
      let nodeid = p.get_key().clone();
      // close all info
      rem_peer(route,&nodeid,rc,threadpool,clientmode,None,Some(pschange));
    },
    PeerMgmtMessage::PeerAddOffline(p) => {
      info!("Adding offline peer : {:?}", p);
      let nodeid = p.get_key().clone();
      let hasnode = route.has_node(&nodeid);
      if !hasnode {
        route.add_node((p,PeerState::Offline(PeerPriority::Unchecked),(None,None)));
      } else {
        route.update_priority(&nodeid,None,Some(PeerStateChange::Offline));
      };
    },
    PeerMgmtMessage::PeerPong(p,prio,chal,ows) => {
      if ows.is_some() {
        // add it to peer or init peer with it (this should be an asym transport
        panic!("write stream on pong not implemented (very special transport case)");
      };
      let nodeid = p.get_key();
      if p.get_key() != rc.me.get_key() {
        // a ping has been received we need to reply with pong : this may also init the peer
        let hasnode = route.has_node(&nodeid);
        let upd = !hasnode || match route.get_node(&nodeid) {
        //  is offline TODO blocked??
          Some(&(_,PeerState::Offline(_), _)) => true,
          _ => false,
        };
        let ok = if upd {
          peer_ping(rc,rp,route,&p,None,prio,heavyaccept,localspawn,local,clientmode,threadpool).is_ok()
        } else {
          true
        };

        // send pong to cli
        let mess = ClientMessage::PeerPong(chal);
        if ok && !local {
          if !route.get_client_info(&nodeid).and_then(|ci|
            ci.send_climsg(mess)).is_ok() {
               warn!("closed connection detected");
               rem_peer(route,&nodeid,rc,threadpool,clientmode,None,Some(PeerStateChange::Offline));
             
          };
 
        } else if ok {
          if !send_local::<RT,T>(&p,rc,route,rp,mess,localspawn,clientmode,threadpool) {
            error!("send_local_pong_reply failure from peerman");
          };
        };
      } else {
        error!("Trying to ping ourselve");
      }
    },
    PeerMgmtMessage::PeerAddFromServer(p,prio,ows,ssend,servinfo) => {
      if p.get_key() != rc.me.get_key() {
        info!("Adding peer : {:?}, {:?} from {:?}", p, prio, rc.me);
        let nodeid = p.get_key().clone();
        let hasnode = route.has_node(&nodeid);
        debug!("-Update peer prio : {:?}, {:?} from {:?}", p, prio, rc.me);
        if !hasnode {
          debug!("-init up");
          // TODO send peerping??? and put state ping instead plus get client handle for add_node
          let (ocliinfo,serinfo) = if local || localspawn {
            // No init of local since their is no added value to establish connection before
            // sending first frame (return clihandle does not shortcut peermanager).
            // paninc on none (but test avoid it)
            (init_local(&clientmode, ows),None)
          } else {
            match init_client_info(&p,&rc,&rp,ows,clientmode,threadpool) {
              Ok(r) => (Some(r.0),r.1),
              Err(e) => {
                error!("cannot connect client connection (a server connection was connected but without send handle, putting peer offline : {}", e);
                rem_peer(route,&nodeid,rc,threadpool,clientmode,None,Some(PeerStateChange::Offline));

                (None,None)
              },

            }
          };
          // if serifo is not none, we got a loose listening stream, this should not happen
          //assert!(serinfo == None);
          if !serinfo.is_none() {
            panic!("inconsistent transport stream and client mode configuration : a reading stream was created with write stream but another reading stream was created before withou a write stream");
          };
          ocliinfo.map(|cliinfo|{
            let clihandler = cliinfo.new_handle();
            route.add_node((p.clone(),PeerState::Offline(prio.clone()),(Some(servinfo),Some(cliinfo))));
            // ssend current cliinfo !!! (server is blocked from it)
            let okserhand = ssend.send(clihandler).is_ok();
            // on send of new clihandler to server (either new peer or connection lost to server
            // and reinit, start a replying ping (pong is sent directly)
            if peer_ping(rc,rp,route,&p,None,prio,heavyaccept,localspawn,local,clientmode,threadpool).is_err() || !okserhand {
              if okserhand {
                 warn!("closed server connection detected");
              } else {
                 warn!("closed connection detected");
              };
              rem_peer(route,&nodeid,rc,threadpool,clientmode,None,Some(PeerStateChange::Offline));
            };
          });
        } else {
          // else do nothing, accept value is not updated and state not updated to
          // TODO change prio to online?? pb skip block...
          debug!("Readdnode nothing done");
          // TODO ssend current cliinfo
        };
      } else {
        error!("Trying to add ourselves as a peer");
      }
    },
    PeerMgmtMessage::ServerInfoFromClient(p,serinfo) => {
      // used to add readinfo when connectwith of transport is used in another thread
      if let &ClientMode::Local(_) = clientmode {
        panic!("received read stream from local thread, this should not be possible");
      };
      // update peer with server info
      let serinfosp = serinfo.clone(); // ugly clone
      match route.update_infos(&p.get_key(), |ref mut sercli|{sercli.0 = Some(serinfosp);Ok(())}) {
        Ok(false) => {
          // close serverhandle
          warn!("Race on peer removal : a peer has been removed before its server handle has been received");
          try!(serinfo.shutdown(&rc.transport, &(*p)));
        },
        Err(e) => {
          // TODO switch to panic??
          error!("Error on updating a client in route {}",e);
        },
        _ => (),
      };
    },
    PeerMgmtMessage::PeerChangeState(p,prio) => {
      debug!("Change peer state : {:?}, {:?} from {:?}", p.get_key(), prio, rc.me.get_key());
      route.update_priority(&p.get_key(),None,Some(prio));
    },
    PeerMgmtMessage::PeerUpdatePrio(p,prio) => {
      debug!("Update peer prio : {:?}, {:?} from {:?}", p.get_key(), prio, rc.me.get_key());
      route.update_priority(&p.get_key(),Some(prio),None);
    },
    PeerMgmtMessage::PeerQueryPlus(p) => {
      debug!("Adding peer query ");
      route.query_count_inc(&p.get_key());
    },
    PeerMgmtMessage::PeerQueryMinus(p,_) => {
      debug!("Adding peer query ");
      route.query_count_dec(&p.get_key());
    },
    PeerMgmtMessage::PeerPing(p, ores) => {
      if peer_ping(rc,rp,route,&p,ores,PeerPriority::Unchecked,heavyaccept,localspawn,local,clientmode,threadpool).is_err() {
        let nodeid = p.get_key();
        // this will drop peerping state and ores will return.
        rem_peer(route,&nodeid,rc,threadpool,clientmode,None,Some(PeerStateChange::Offline));
      };
    },
    PeerMgmtMessage::KVFind(key, queryconf) => {
      let remhop = queryconf.rem_hop;
      let nbquery = queryconf.nb_forw;
//        let qp = queryconf.prio;
      let mut ok = remhop > 0;
      if ok {
        // get closest node to query
        // if no result launch (spawn) discovery processing
        debug!("!!!in peer find of procman bef get closest");
        let peers = match queryconf.hop_hist {
          None => route.get_closest_for_query(&key, nbquery, &VecDeque::new()),
          Some(LastSent::LastSentPeer(_, ref filter)) | Some(LastSent::LastSentHop(_, ref filter)) => route.get_closest_for_query(&key, nbquery, filter),
        };
        let mut newqueryconf = update_lastsent_conf ( &queryconf , &peers , nbquery);
        let rsize = peers.len();
        // update number of result to expect for each proxyied request
        let rnbres = newqueryconf.nb_res;
        let newrnbres = if rsize == 0 {
          rnbres
        } else {
          let newrnbres_round = rnbres / rsize;
          if (newrnbres_round * rsize) < rnbres {
            // we prefer to send to much query than the other way (could be a lot more)
            newrnbres_round + 1
          } else {
            newrnbres_round
          }
        };
        newqueryconf.nb_res = newrnbres;
        if rsize == 0 {
          ok = false;
        } else {
          let qh = if let QueryModeMsg::Asynch(..) = queryconf.modeinfo {
              QueryHandle::NoHandle
           } else {
              QueryHandle::QueryManager(queryconf.get_query_id())
           };
           let mut nbfail = 0;
           for p in peers.iter() {
             let mess =  ClientMessage::KVFind(key.clone(),qh.clone(), newqueryconf.clone()); // TODO queryconf arc?? + remove newqueryconf.clone() already cloned why the second (bug or the fact that we send)
             if !local {
               let nodeid = p.get_key();
               if !route.get_client_info(&nodeid).and_then(|ci|
                 ci.send_climsg(mess)).is_ok() {
                 warn!("closed connection detected");
                 rem_peer(route,&nodeid,rc,threadpool,clientmode,None,Some(PeerStateChange::Offline));
                 nbfail = nbfail + 1;
               };
             } else {
               if !send_local::<RT, T> (p, rc , route, rp, mess, localspawn,clientmode,threadpool) {
                 // peer rem in send_local method
                 nbfail = nbfail + 1;
               };
             };
           };
           if nbfail >= rsize {
             ok = false;
           } else {
           if nbfail > 0 {

             try!(rp.queries.send(QueryMgmtMessage::Lessen(queryconf.get_query_id(),nbfail))); // TODO asynch mult???
           }};
        };
      };
      if !ok {
        debug!("forwarding kvfind to no peers, releasing query");
        try!(rp.queries.send(QueryMgmtMessage::Release(queryconf.get_query_id())));
      };
    },
    PeerMgmtMessage::PeerFind(nid, mut oquery, queryconf, in_cache) => {
      let remhop = queryconf.rem_hop;
      let nbquery = queryconf.nb_forw;
//        let qp = queryconf.prio;
 
      // query ourselve if here and local // no ping at this point : trust current
      // status, proxyto is either right with destination or left with a possible result in
      // parameter
      // async
      let asyncproxy = !in_cache && oquery.is_none();
      let proxyto = 
      {
      let rnode = if in_cache {
        None // already done once
      } else {
        route.get_node(&nid)
      };

        match rnode {
        // blocked peer are also blocked for transmission (this is likely to make them
        // disapear) TODO might not be a nice idea
        Some(&(_,PeerState::Blocked(_), _)) => Either::Left(None),
        // no ping at this point : trust current status
        Some(&(ref ap,PeerState::Online(_), _)) => Either::Left(Some((*ap).clone())),
        // None or offline (offline may mean we need updated info for peer
        _ => {
          if remhop > 0 {
            // get closest node to query
            // if no result launch (spawn) discovery processing
            let peers = match queryconf.hop_hist {
              None => route.get_closest_for_node(&nid, nbquery, &VecDeque::new()),
              Some(LastSent::LastSentPeer(_, ref filter)) | Some(LastSent::LastSentHop(_, ref filter))  => 
                route.get_closest_for_node(&nid, nbquery, filter), // this get_closest could be skip by sending to querymanager directly
            };
            let newqueryconf = update_lastsent_conf ( &queryconf , &peers , nbquery);

            let rsize = peers.len();
            if rsize == 0 {
                 Either::Left(None)
            } else {
              Either::Right((newqueryconf,peers))
            }
          } else {
            Either::Left(None)
          }
        },
      }};
      match proxyto {
        // result local due to not proxied (none) or result found localy
        Either::Left(r) => {
          match oquery {
            Some(mut query) => {
              // put result !! in a spawn (do not want to manipulate mutex here (even
              let ssp = rp.peers.clone();
              let srp = rp.store.clone();
              // TODO remove this spawn even if dealing with mutex (too costy?? )
              thread::spawn(move || {
                debug!("!!!!found not sent unlock semaphore");
                if r == None {
                  // No update of result so will reply none
                  query.release_query(&ssp);
                } else {
                  if query.set_query_result(Either::Left(r),&srp) {
                    query.release_query(&ssp);
                  } else {
                    if query.lessen_query(1, &ssp) {
                      query.release_query(&ssp);
                    }
                  }
                };
              });
            },
            None => {
              // Here no query object created
              // eg Async hop reply directly
              // asynch)
              if asyncproxy {
                  debug!("!!!AsyncResult returning {:?}", r); 
               match queryconf.modeinfo.get_rec_node().map(|r|r.clone()) {
                Some (ref recnode) => {
                  let mess = ClientMessage::StoreNode(queryconf.modeinfo.to_qid(), r);
                  let nodeid = recnode.get_key();
                  let upd = !route.has_node(&nodeid) || match route.get_node(&nodeid) {
                    Some(&(_,PeerState::Offline(_), _)) => true,
                    _ => false,
                  };
                  let ok = if upd {
                    peer_ping(rc,rp,route,recnode,None,PeerPriority::Unchecked,heavyaccept,localspawn,local,clientmode,threadpool).is_ok() // TODO actually this only tell if route will contain peer in Ping status, and start an auth, sending cli_messg should be done after auth : TODO evo to add message to Ping state !!!
                  } else {
                    true
                  };
                  if ok && !local {
                    // send result directly
                    if !route.get_client_info(&nodeid).and_then(|ci|
                      ci.send_climsg(mess)).is_ok() {
                        warn!("closed connection detected");
                        rem_peer(route,&nodeid,rc,threadpool,clientmode,None,Some(PeerStateChange::Offline));
                    };
 // TODO borderline as no auth : just proxy (a ping has been send but without checking of pong : we need to trust query from : implement a condition to send message on peerauth (add msg to peerping state)
                  } else if ok {
                     send_local::<RT, T> (recnode, rc , route, rp, mess, localspawn,clientmode,threadpool);
                  };
                },
                _ => {error!("None query in none asynch peerfind");},
              }

              } else {
                  debug!("None return to queryman, no proxy, releasing");
                  if r.is_some() {
                    try!(rp.queries.send(QueryMgmtMessage::NewReply(queryconf.get_query_id(),(r,None))));
                  } else {
                    try!(rp.queries.send(QueryMgmtMessage::Release(queryconf.get_query_id())));
                  };
              };
            },
          };
        },
        Either::Right((newqueryconf,peers)) => {
          let rsize = peers.len();
          assert!(rsize != 0); // should have been left case
          let (rel, qh) = 
            match oquery {
              Some(mut query) => {
                // TODO nf treashold in queryconf?? TODO check this with fail in kvfind (unclear,
                // overcomplicated)
                let nftresh = rc.rules.notfoundtreshold(nbquery, remhop, &newqueryconf.modeinfo.get_mode());
                let nbless = calc_adj(nbquery, rsize, nftresh);
                // Note that it doesnot prevent unresponsive client
                if query.lessen_query(nbless,&rp.peers) {
                  query.release_query(&rp.peers);
                  (true, QueryHandle::NoHandle) // here qh not use
                } else {
                  //let qh = query.get_handle();
                  try!(rp.queries.send(QueryMgmtMessage::NewQuery(query, PeerMgmtInitMessage::PeerFind(nid.clone(),queryconf.clone()))));
                    debug!("peerfind send to queryman");
                    (true, QueryHandle::NoHandle) // here qh not use
                   // (true, qh)
               }
 
               },
               None => {
                if asyncproxy {
                  (false, QueryHandle::NoHandle)
                } else {
                  let qh = QueryHandle::QueryManager(queryconf.get_query_id());
                  (false, qh)
                }
              },
          };
          if !rel {
           let mut nb_fail = 0;
           for p in peers.iter() {
             let mess = ClientMessage::PeerFind(nid.clone(),qh.clone(), newqueryconf.clone()); // TODO queryconf arc??
             if !local {
               // get connection
               let nodeid = p.get_key();
               if !route.get_client_info(&nodeid).and_then(|ci|
                 ci.send_climsg(mess)).is_ok() {
                   warn!("closed connection detected");
                   rem_peer(route,&nodeid,rc,threadpool,clientmode,None,Some(PeerStateChange::Offline));
                   nb_fail = nb_fail + 1;
               };
             } else {
               if !send_local::<RT, T> (p, rc , route, rp, mess, localspawn,clientmode,threadpool) {
                 nb_fail = nb_fail + 1;
               };
             };
           };
           if nb_fail > 0 {
             try!(rp.queries.send(QueryMgmtMessage::Lessen(queryconf.get_query_id(),nb_fail))); // TODO asynch mult???
           }};


          },
         };
     },
     PeerMgmtMessage::Refresh(max) => {
       info!("Refreshing connection pool");
       let torefresh = route.get_pool_nodes(max);
       for n in torefresh.iter(){
         if local {
           send_local_ping::<RT, T>(n, rc, route, rp,localspawn,clientmode,threadpool,None);
         } else {
           // normal process
           peer_ping(rc,rp,route,n,None,PeerPriority::Unchecked,heavyaccept,localspawn,local,clientmode,threadpool).is_ok();
         }
       }
     },
     PeerMgmtMessage::ShutDown => {
       info!("Shudown receive");
       return Ok(false);
     },
     PeerMgmtMessage::StoreNode(qconf, result) => {
       match qconf.modeinfo.get_rec_node().map(|r|r.clone()) {
         Some (rec)  => {
           let nodeid = rec.get_key();
           if nodeid != rc.me.get_key() {
             let mess = ClientMessage::StoreNode(qconf.modeinfo.to_qid(), result);
             // TODO this upd check should be only for asynch modes
             let upd = !route.has_node(&nodeid) || match route.get_node(&nodeid) {
               Some(&(_,PeerState::Offline(_), _)) => true,
               _ => false,
             };
             let ok = if upd {
               peer_ping(rc,rp,route,&rec,None,PeerPriority::Unchecked,heavyaccept,localspawn,local,clientmode,threadpool).is_ok()
             } else {
               true
             };
             if ok && !local {
               if !route.get_client_info(&nodeid).and_then(|ci|
                 ci.send_climsg(mess)).is_ok() {
                   warn!("closed connection detected");
                   rem_peer(route,&nodeid,rc,threadpool,clientmode,None,Some(PeerStateChange::Offline));
                 };
               // TODO do something for not having to create an arc here eg arc in qconf + previous qconf clone
             } else if ok {
               send_local::<RT, T> (&rec, rc, route, rp, mess, localspawn,clientmode,threadpool);
             };
           } else {
             error!("local loop detected for store node");
           }
         },
         None => {
           error!("Cannot proxied received store node to originator for conf {:?} ",qconf.modeinfo);
         },
       }
     },
     PeerMgmtMessage::StoreKV(qconf, result) => {
       // (query sync over clients(the query contains nothing)) // TODO factorize a fun
       // for proxied msg 
       match qconf.modeinfo.get_rec_node().map(|r|r.clone()) {
         Some (rec) => {
           let nodeid = rec.get_key();
           if nodeid != rc.me.get_key() {
             let mess = ClientMessage::StoreKV(qconf.modeinfo.to_qid(), qconf.chunk, result);
             // TODO this upd check should be only for asynch modes
             let upd = !route.has_node(&nodeid) || match route.get_node(&nodeid) {
               Some(&(_,PeerState::Offline(_), _)) => true,
               _ => false,
             };
             let ok = if upd {
               peer_ping(rc,rp,route,&rec,None,PeerPriority::Unchecked,heavyaccept,localspawn,local,clientmode,threadpool).is_ok()
             } else {
               true
             };
             if ok && !local {
               if !route.get_client_info(&nodeid).and_then(|ci|
                 ci.send_climsg(mess)).is_ok() {
                   warn!("closed connection detected");
                   rem_peer(route,&nodeid,rc,threadpool,clientmode,None,Some(PeerStateChange::Offline));
               }

             } else if ok {
               send_local::<RT, T> (&rec, rc , route, rp, mess, localspawn,clientmode,threadpool);
             };
           } else {
             error!("local loop detected for store kv {:?}", result);
           }
         },
         None => {
           error!("Cannot proxied received store value to originator for conf {:?} ",qconf.modeinfo);
         },
       }
     },
  };
  Ok(true)
}

#[inline]
// TODO all bool should be client mode?? -> also for peerping (moreover cannot run distinct mode
// per peer TODO use this to avoid duplicated code
/// When peer is missing, if sending with full peer reference, connect and ping will be try, if sending with only node id
/// false is returned directly.
fn clisend<RT : RunningTypes, T : Route<RT::A,RT::P,RT::V,RT::T>> 
 (
  ep : Either<& Arc<RT::P>, & <RT::P as KeyVal>::Key>,
  rc : & ArcRunningContext<RT>, 
  route : & mut T , 
  rp : & RunningProcesses<RT>,
  mess : ClientMessage<RT::P,RT::V,<RT::T as Transport>::WriteStream>,
  localspawn : bool,
  heavyaccept : bool,
  local : bool,
  clientmode : &ClientMode,
  threadpool : &mut ThreadPool<RT>,
 ) -> MydhtResult<bool> {
  let (op,oi) = ep.to_options();
  let onodeid = op.map(|p|p.get_key());
  let nodeid = match onodeid.as_ref() {
    Some(ni) => ni,
    None => oi.unwrap(), // ok to panic
  };
  if *nodeid != rc.me.get_key() {
    // a ping has been received we need to reply with pong : this may also init the peer
    let hasnode = route.has_node(nodeid);
    let (upd, prio, oep) = if !hasnode {
      (true, PeerPriority::Unchecked, None)
    } else { match route.get_node(nodeid) {
      //  is offline TODO blocked??
      Some(&(ref p,PeerState::Offline(ref prio), _)) => {
        let oep = if op.is_none() {
          Some((*p).clone())
        } else { None };
        (true,(*prio).clone(),oep)
      },
      _ => (false, PeerPriority::Unchecked, None),
    }};
    let ok = if upd {
      if op.is_none() {
        return Ok(false);
      } else {
        let p = match oep.as_ref() {
          Some(p) => p,
          None => op.unwrap(),
        };
        peer_ping(rc,rp,route,p,None,prio,heavyaccept,localspawn,local,clientmode,threadpool).is_ok()
      }
    } else {
      true
    };
    // send mess to cli
    if !ok {
      Ok(ok)
    } else if !local {
      let res = route.get_client_info(nodeid).and_then(|ci| ci.send_climsg(mess));
      if res.is_err() {
        warn!("closed connection detected");
        rem_peer(route,nodeid,rc,threadpool,clientmode,None,Some(PeerStateChange::Offline));
        res.map(|_|true)
      } else {
        Ok(true)
      }
    } else {
      if !send_local::<RT,T>(op.unwrap(),rc,route,rp,mess,localspawn,clientmode,threadpool) {
        error!("send message failure from peerman");
        Err(Error("local send error TODO send from send_local fn".to_string(), ErrorKind::RouteError,None))
      } else {
        Ok(true)
      }
    }
  } else {
    Err(Error("Trying to send message to ourselve".to_string(), ErrorKind::RouteError,None))
  }
}


#[inline]
// could not initiate client info of route, it is considered that we need to reping
// TODO return result TODO fuse with clisend
fn send_local<RT : RunningTypes, T : Route<RT::A,RT::P,RT::V,RT::T>>
 (p : & Arc<RT::P> , 
  rc : & ArcRunningContext<RT>, 
  route : & mut T , 
  rp : & RunningProcesses<RT>,
  mess : ClientMessage<RT::P,RT::V,<RT::T as Transport>::WriteStream>,
  localspawn : bool,
  clientmode : &ClientMode,
  threadpool : &mut ThreadPool<RT>,
 )
 -> bool {
   // TODO first route.updateinfo  (and send_mesg_local on ci), then if Err with kind no Cliinfo in
   // route : create cliinfo by transport connect (even for spawn true : cannot connect in new
   // thread or we may connect numerous times).
 
 let key = p.get_key();
 // TODO skip those check after fn called only from clisend
 let (olspawn, res) = match route.get_node(&key) {
   Some(&(_,PeerState::Refused,_)) => {
      info!("#####refused : client do not send");
      (None,false)
   },
   Some(&(_,PeerState::Blocked(_),_)) => {
      // if existing consider ping true : normaly if offline or block , no channel
      info!("#####blocked : client do not send");
      (None,false)
   },
   Some(&(_,_,ref secli)) => {
     if localspawn {
       (Some(secli.1.as_ref().and_then(|cli|cli.get_clone_sender())), true)
     } else {
       (None,true)
     }
   },
   None => (None,false),
 };
 if res {
   let (rem,rec) = match olspawn {
     // localspawn
     Some(mut mutws) => {
       let rpsp = rp.clone();
       let rcsp = rc.clone();
       let psp = p.clone();
       let erpeer = rpsp.peers.clone();
       thread::spawn (move || {
         if client::start_local::<RT>(&psp, &rcsp, &rpsp, Either::Right((mutws.as_mut(),mess))).is_err() {
           // TODO try reconnect or add it to client directly
           sphandler_res(erpeer.send(PeerMgmtMessage::PeerRemFromClient(psp, PeerStateChange::Offline)));
         }
       });
       (false,false)
     },
     // local
     None => {
       let mut rem = false;
       let mut rec = false;
       match route.update_infos(&key,|mut sercli| {
           match sercli.1 {
           Some(ref mut cli) => {
             let osend = cli.get_mut_sender();
             assert!(osend.is_some());
             if !try!(client::start_local::<RT>(&p, &rc, &rp, Either::Right((osend,mess)))) {
               rem = true;
               rec = true;
             }
           },
           None => {
             let (cli, oser) = try!(init_client_info(&p, &rc, &rp, None, &ClientMode::Local(false),threadpool));
             sercli.1 = Some(cli);
             if let Some(ref mut clii) = sercli.1 {
               let osend = clii.get_mut_sender();
               assert!(osend.is_some());
             
               if !try!(client::start_local::<RT>(&p, &rc, &rp, Either::Right((osend,mess)))) {
                 // no reconnnect
                 rem = true;
               };
               assert!(!(sercli.0.is_some() && oser.is_some()));
               sercli.0 = oser;
             } else {
               panic!("see pervious lines")
             };
           },
         };
         Ok(())
       }) {
         Ok(false) => {
           debug!("message not send, no peer in peermanager");
           (false,false)
         },
         Ok(true) => (rem,rec),
         Err(_) => (true,false),
       }
     },
   };
   if rem {
     if rec {
     // TODO try reconnect 
     };
     warn!("Closed connection detected");
     rem_peer(route,&key,rc,threadpool,clientmode,None,Some(PeerStateChange::Offline));
   }
 };
 res
}



#[inline]
// could initiate client info of route (using connect with)
fn send_local_ping<RT : RunningTypes, T : Route<RT::A,RT::P,RT::V,RT::T>>
 (p : & Arc<RT::P> , 
  rc : & ArcRunningContext<RT>, 
  route : & mut T , 
  rp : & RunningProcesses<RT>,
  localspawn : bool,
  clientmode : &ClientMode,
  threadpool : &mut ThreadPool<RT>,
  ores : Option<OneResult<bool>>,
 )
 ->  bool {

   let chal = rc.peerrules.challenge(&(*p));
   let mess = ClientMessage::PeerPing(p.clone(), chal.clone());
   let r = send_local(p,rc,route,rp,mess,localspawn,clientmode,threadpool);
   if r {
     route.update_priority(&p.get_key(),None,Some(PeerStateChange::Ping(chal,TransientOption(ores))));
   };
   r
}

#[inline]
fn init_client_info<'a, RT : RunningTypes>
 (p : & Arc<RT::P>,
  rc : & ArcRunningContext<RT>,
  rp : & RunningProcesses<RT>,
  ows : Option<<RT::T as Transport>::WriteStream>, 
  cmode : & ClientMode,
  tp : &mut ThreadPool<RT>,
 )
 -> MydhtResult<(ClientInfo<RT::P,RT::V,RT::T>, Option<ServerInfo>)> {
  match cmode {
    &ClientMode::Local(dospawn) => match ows {
      Some (ws) => Ok((ClientInfo::Local(ClientSender::Local(ws)),None)),
      None => {
        // TODO duration in rules
        let (ws, ors) = try!(rc.transport.connectwith(&p.to_address(), Duration::seconds(5)));
        // Send back read stream an connected status to peermanager
        let osi = match ors {
          None => None,
          Some(rs) => {
            let sh = try!(start_listener(rs,&rc,&rp));
            Some(serverinfo_from_handle(&sh))
          },

        };
        if dospawn {
          Ok((ClientInfo::LocalSpawn(ClientSender::LocalSpawn(Arc::new(Mutex::new(ws)))),osi))
        } else {
          Ok((ClientInfo::Local(ClientSender::Local(ws)),osi))
        }
      },
    },
    &ClientMode::ThreadedOne => {
      let (tcl,rcl) = channel();
      let ci = ClientInfo::Threaded(tcl,0,0);
      let psp = p.clone();
      let rcsp = rc.clone();
      let rpsp = rp.clone();

      thread::spawn (move || {client::start::<RT>(&psp, rcl, rcsp, rpsp, ows.map(|ws|ClientSender::Threaded(ws)), false)});
      Ok((ci,None))
    },
    &ClientMode::ThreadedMax(_) | &ClientMode::ThreadPool(_) => {
      let (thix, pix) = try!(add_peer_to_pool (&mut tp.0, cmode));
      if is_lesseq_pool_thread(&mut tp.0,thix,1,cmode) {
        let (tcl,rcl) = channel();
        let ctcl = tcl.clone();
        let ci = ClientInfo::Threaded(tcl,pix,thix);
        let psp = p.clone();
        let rcsp = rc.clone();
        let rpsp = rp.clone();

        thread::spawn (move || {client::start::<RT>(&psp, rcl, rcsp, rpsp, ows.map(|ws|ClientSender::Threaded(ws)), true)});
        
        if thix < tp.1.len() {
          tp.1.get_mut(thix).map(|mut v|{
            if v.is_some() {
              error!("starting a new thread over an existing one");
            };
            *v = Some(ctcl);
          });
        } else {
          for i in tp.1.len()..thix {
            tp.1.push(None);
          };
          tp.1.push(Some(ctcl));
        };
        Ok((ci,None))
      } else {
        let ci = match tp.1.get(thix) {
          Some(&Some(ref tcl)) => {
            try!(tcl.send((ClientMessage::PeerAdd(p.clone(),ows),pix)));
            ClientInfo::Threaded((*tcl).clone(),pix,thix)
          },
          _ => {
            return Err(Error("missing reference to multiplexed thread".to_string(), ErrorKind::RouteError, None));
          },
        };
        Ok((ci,None))
      }
    },
  }
}
 

#[inline]
fn update_lastsent_conf<P : Peer> ( queryconf : &QueryMsg<P>,  peers : &Vec<Arc<P>>, nbquery : u8) -> QueryMsg<P> {
  let mut newqueryconf = queryconf.clone();
  newqueryconf.hop_hist = match newqueryconf.hop_hist {
    Some(LastSent::LastSentPeer(maxnb,mut lpeers)) => {
      let totalnb = peers.len() + lpeers.len();
      if totalnb > maxnb {
        for _ in 0..(totalnb - maxnb) {
          lpeers.pop_front();
        };
      }else{};
      for p in peers.iter(){
        lpeers.push_back(p.get_key());
      };
      Some(LastSent::LastSentPeer(maxnb, lpeers))
    },
    Some(LastSent::LastSentHop(mut hop,mut lpeers)) => {
      if hop > 0 {
        hop -= 1;
      } else {
        for _ in 0..(nbquery) { // this is an approximation (could be less)
          lpeers.pop_front();
        };
      };
      for p in peers.iter(){
        lpeers.push_back(p.get_key());
      };
      Some(LastSent::LastSentHop(hop, lpeers))
    },
    None => None,
  };
  newqueryconf
}

#[inline]
pub fn init_local<P : Peer, V : KeyVal, T : Transport> (cm : &ClientMode, ows : Option<T::WriteStream>) -> Option<ClientInfo<P,V,T>> {
  match cm {
    &ClientMode::Local(false) => ows.map(|ws|ClientInfo::Local(ClientSender::Local(ws))),
    &ClientMode::Local(true) => ows.map(|ws|ClientInfo::LocalSpawn(ClientSender::LocalSpawn(Arc::new(Mutex::new(ws))))),
    _ => None,
  }
}

/// process to run for pinging a peer, for new peer, peer initialization is done
/// TODO return bool to have right cannot connect and wrong io error (for refused or unreachable)
fn peer_ping <RT : RunningTypes, 
  T : Route<RT::A,RT::P,RT::V,RT::T>,
  >
 (rc : &ArcRunningContext<RT>,
  rp : &RunningProcesses<RT>,
  route : &mut T,
  p : &Arc<RT::P>,
  ores : Option<OneResult<bool>>,
  iprio : PeerPriority,
  heavyaccept : bool,
  localspawn : bool,
  local : bool,
  clientmode : &ClientMode,
  threadpool : &mut ThreadPool<RT>,
  ) -> MDHTResult<()> {
  let nodeid = p.get_key();
  if nodeid != rc.me.get_key() {
    let (toinit, notref) = if !route.has_node(&nodeid) {
      let mut acc = true;
      // add node
      let prio = if !heavyaccept && iprio == PeerPriority::Unchecked {
        match rc.peerrules.accept(&p, &rp, &rc) {
          Some(p) => p,
          None => { 
            acc = false;
            PeerPriority::Unchecked
          },
        }
      } else {
        iprio
      };
      if acc {
        route.add_node((p.clone(),PeerState::Offline(prio.clone()),(None,None)));
        (Some((prio,false)),true)
      } else {
        route.add_node((p.clone(),PeerState::Refused,(None,None)));
        (None,false)
      }
    } else {
      // TODO add Blocked??
      match route.get_node(&nodeid) {
        Some(&(_,PeerState::Offline(ref pri), (ref s,None))) => {
          (Some((pri.clone(),s.is_some())),true)
        },
        Some(&(_,PeerState::Refused, _)) => (None,false),
        _ => (None,true)
      }
    };
    let ok = notref && toinit.map_or(true,|(pri,hasserv)|{
      init_client_info(&p,&rc,&rp,None,clientmode,threadpool).map(|ci|{
        assert!(!(ci.1.is_some() && hasserv));
        route.add_node((p.clone(),PeerState::Offline(pri),(ci.1,Some(ci.0))));
      }).is_ok()
    });
    debug!("Pinging peer : {:?}", p);
    if ok && !local {
      let chal = rc.peerrules.challenge(&(*p));
      route.update_priority(&nodeid,None,Some(PeerStateChange::Ping(chal.clone(),TransientOption(ores))));
      route.get_client_info(&nodeid).and_then(|ci|
        ci.send_climsg(ClientMessage::PeerPing(p.clone(),chal)))
    } else if ok {
      if !send_local_ping::<RT, T>(p, rc , route, rp,localspawn,clientmode,threadpool,ores) {
        error!("send_local_ping failure TODO get error");
        // TODO get errorr
        Err(Error("send_local_ping failure TODO get error".to_string(), ErrorKind::PingError,None))
      } else {Ok(())}
    } else {
      // TODO two different error (return error in code)
      Err(Error("refused peer or unreachable peer".to_string(), ErrorKind::PingError,None))
    }
  } else {
    error!("Trying to ping ourselves");
    Err(Error("Trying to ping ourselves".to_string(), ErrorKind::PingError,None))
  }
}

#[inline]
fn calc_adj(nbquery : u8, rsize : usize, nftresh : usize) -> usize {
  let unbq = nbquery.to_usize().unwrap();
  // adjust nb request
  let adj = unbq - rsize;
  if nftresh == unbq {
    adj
  } else {
    adj * nftresh / unbq
  }
}
/// next free position and is there a sender
type ThreadPoolInfo = (usize,BitVec);
/// Thread pool is only use for multiple peers in order to manage index of new peers and start of
/// new threads (a thread with no peers is designed to stop).
/// Synchro with peer thread is a push : we may have living thread with empty peers slot until
/// receiving peer removed message from client, but client are only added from this thread.
type ThreadPool<RT : RunningTypes> = (ThreadPoolInfo,Vec<Option<Sender<ClientMessageIx<RT::P,RT::V,<RT::T as Transport>::WriteStream>>>>); // TODO replace vec bool by bitvec

#[inline]
fn is_empty_pool_thread (tp : &mut ThreadPoolInfo, thix : usize, cmode : &ClientMode) -> bool {
  is_lesseq_pool_thread(tp,thix,0,cmode)
}

// TODO memoize size of each thix in threadpoolinfo
fn is_lesseq_pool_thread (tp : &mut ThreadPoolInfo, thix : usize, treshold : usize, cmode : &ClientMode) -> bool {
  let mut tot = 0;
  
  match cmode {
    &ClientMode::ThreadPool(ref poolsize) => {
      if thix >= *poolsize {
        return true;
      };
      let mut ix = thix;
      while let Some(v) = tp.1.get(ix) {
        if v {
          tot = tot + 1;
          if tot > treshold {
            return false;
          };
        };
        ix = ix + poolsize;
      }
    },
    &ClientMode::ThreadedMax(ref maxth) => {
      let ixstart = thix * maxth;
      for ix in ixstart..(ixstart + maxth) {
        if let Some(true) = tp.1.get(ix) {
          tot = tot + 1;
          if tot > treshold {
            return false;
          };
        };
      };
    },
    _ => {
      error!("wrong peer config for thread pool removal");
      return false;
    },
  };
  true
}
 
// return thread ix and peer index in thread 
fn add_peer_to_pool (tp : &mut ThreadPoolInfo, cmode : &ClientMode) -> MDHTResult<(usize, usize)> {
  let (thix, pix) = match cmode {
    &ClientMode::ThreadPool(ref poolsize) => {
      let pix = tp.0 / poolsize;
      (tp.0 - pix * poolsize, pix)
    },
    &ClientMode::ThreadedMax(ref maxth) => {
      let thix = tp.0 / maxth;
      (thix.clone(), tp.0 - thix * maxth)
    },
    _ => {
      error!("wrong peer config for thread pool removal");
      return Err(Error("Wrong peer config".to_string(), ErrorKind::RouteError, None));
    },
  };

  let is_last =  match tp.1.get(tp.0) {
    Some(val) => {
      if val {
        debug!("wrong *val index value, pool may be full");
        return Err(Error("Full pool of peers".to_string(), ErrorKind::RouteError, None));
      } else {
        false
      }
    },
    None => {
      true 
    },
  };
  if is_last {
    tp.1.push(true); 
    // here if thread limit push false instead (currently no limit)
    tp.0 = tp.0 + 1;
  } else  {
    tp.1.set(tp.0,true);
    tp.0 = tp.1.iter().position(|b|b==false).unwrap_or(tp.1.len());
  };
  Ok((thix, pix))
  
}

fn rem_peer_from_pool (tp : &mut ThreadPoolInfo, thix : usize, pix : usize, cmode : &ClientMode) {
  let ix = match cmode {
    &ClientMode::ThreadPool(ref poolsize) => pix * poolsize + thix,
    &ClientMode::ThreadedMax(ref maxth) => thix * maxth + pix,
    _ => {
      error!("wrong peer config for thread pool removal");
      return
    },
  };
  let rem = match tp.1.get(ix) {
    Some(val) => {
      if val {
        true
      } else {
        debug!("trying to remove peer that was already removed from pool");
        false
      }
    },
    None => {
      debug!("trying to remove peer but not define peer index in pool");
      false
    },
  };
  if rem {
    tp.1.set(ix,false);
    if ix < tp.0 {
    tp.0 = ix;
    };
  };
}

#[inline]
/// remove peer with new prio
fn rem_peer
<RT : RunningTypes, 
  T : Route<RT::A,RT::P,RT::V,RT::T>,
  >
(route : &mut T, 
 k : &<RT::P as KeyVal>::Key, 
 rc : &ArcRunningContext<RT>,
 tp : &mut ThreadPool<RT>,
 cm : &ClientMode,
 ons : Option<PeerState>,
 onsc : Option<PeerStateChange>) {
 match cm {
   &ClientMode::ThreadedMax(_) | &ClientMode::ThreadPool(_) => {
     match route.get_client_info(k) {
       Ok(&ClientInfo::Threaded(_,ref ix, ref thix)) => {
         rem_peer_from_pool(&mut tp.0, *thix, *ix, cm);
         if is_empty_pool_thread(&mut tp.0,*thix, cm) {
           tp.1.get_mut(*thix).map(|mut v|{
             if v.is_none() || v.as_ref().unwrap().send((ClientMessage::EndMult,usize::max_value())).is_err(){
               error!("could not send end thread message");
             };
             *v = None
           }); 
         };
       },
       _ => {warn!("mismatch client mode and client info, or missing client for removal");},
     }
   },
   _ => (),
 };
 route.update_priority(k,ons,onsc);
 route.remchan(k,&rc.transport);
}

#[test]
fn test_thread_max() {
  let cmode = ClientMode::ThreadedMax(2);
  let mut tp = (0,BitVec::new());

  assert!((0,0) == add_peer_to_pool (&mut tp, &cmode).unwrap());
  assert!((0,1) == add_peer_to_pool (&mut tp, &cmode).unwrap());
  assert!((1,0) == add_peer_to_pool (&mut tp, &cmode).unwrap());
  assert!((1,1) == add_peer_to_pool (&mut tp, &cmode).unwrap());
  assert!((2,0) == add_peer_to_pool (&mut tp, &cmode).unwrap());
  assert!(false == is_empty_pool_thread (&mut tp, 0, &cmode));
  assert!(false == is_empty_pool_thread (&mut tp, 1, &cmode));
  assert!(false == is_empty_pool_thread (&mut tp, 2, &cmode));
  assert!(true == is_empty_pool_thread (&mut tp, 3, &cmode));
  rem_peer_from_pool (&mut tp, 1, 0, &cmode);
  assert!(false == is_empty_pool_thread (&mut tp, 1, &cmode));
  rem_peer_from_pool (&mut tp, 1, 1, &cmode);
  assert!(true == is_empty_pool_thread (&mut tp, 1, &cmode));
  assert!((1,0) == add_peer_to_pool (&mut tp, &cmode).unwrap());
  assert!((1,1) == add_peer_to_pool (&mut tp, &cmode).unwrap());
  assert!((2,1) == add_peer_to_pool (&mut tp, &cmode).unwrap());
}
#[test]
fn test_thread_pool() {
  let cmode = ClientMode::ThreadPool(2);
  let mut tp = (0,BitVec::new());

  assert!((0,0) == add_peer_to_pool (&mut tp, &cmode).unwrap());
  assert!((1,0) == add_peer_to_pool (&mut tp, &cmode).unwrap());
  assert!((0,1) == add_peer_to_pool (&mut tp, &cmode).unwrap());
  assert!((1,1) == add_peer_to_pool (&mut tp, &cmode).unwrap());
  assert!((0,2) == add_peer_to_pool (&mut tp, &cmode).unwrap());
  assert!(false == is_empty_pool_thread (&mut tp, 0, &cmode));
  assert!(false == is_empty_pool_thread (&mut tp, 1, &cmode));
  assert!(true == is_empty_pool_thread (&mut tp, 2, &cmode));
  rem_peer_from_pool (&mut tp, 1, 0, &cmode);
  assert!(false == is_empty_pool_thread (&mut tp, 1, &cmode));
  rem_peer_from_pool (&mut tp, 1, 1, &cmode);
  assert!(true == is_empty_pool_thread (&mut tp, 1, &cmode));
  assert!((1,0) == add_peer_to_pool (&mut tp, &cmode).unwrap());
  assert!((1,1) == add_peer_to_pool (&mut tp, &cmode).unwrap());
  assert!((1,2) == add_peer_to_pool (&mut tp, &cmode).unwrap());
}
