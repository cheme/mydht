use rustc_serialize::json;
use procs::mesgs::{self,PeerMgmtMessage,ClientMessage};
use std::io::Result as IoResult;
use peer::{PeerMgmtMeths, PeerPriority,PeerState,PeerStateChange};
use std::sync::mpsc::{Sender,Receiver};
use std::str::from_utf8;
use procs::{client,RunningContext,ArcRunningContext,RunningProcesses,RunningTypes};
use std::collections::{HashMap,BTreeSet,VecDeque};
use std::sync::{Arc,Semaphore,Condvar,Mutex};
use query::{self,QueryModeMsg,LastSent,QueryMsg};
use route::Route;
use std::sync::mpsc::channel;
use std::thread;
use transport::{Transport,TransportStream};
use peer::Peer;
use utils::{self,OneResult,Either};
use keyval::{KeyVal};
use msgenc::{MsgEnc};
use num::traits::ToPrimitive;
use procs::ClientMode;
use rules::DHTRules;
use route::{PeerInfo,ClientInfo,ServerInfo};
use procs::server::resolve_server_mode;

// peermanager manage communication with the storage : add remove update. The storage is therefore
// not shared, we use message passing. 
/// Start a new peermanager process
pub fn start<RT : RunningTypes, 
  T : Route<RT::P,RT::V,RT::T>,
  F : FnOnce() -> Option<T> + Send + 'static,
  >
 (rc : ArcRunningContext<RT>,
  mut routei : F, 
  r : &Receiver<PeerMgmtMessage<RT::P,RT::V,<RT::T as Transport>::ReadStream,<RT::T as Transport>::WriteStream>>, 
  rp : RunningProcesses<RT>, 
  sem : Arc<Semaphore>) {
  let mut route = routei().unwrap_or_else(||panic!("route initialization failed"));
  loop {

    let clientmode = rc.rules.client_mode();
    let servermode = resolve_server_mode (&rc);
    // TODO plug is auth false later (globally)
    let isauth = rc.rules.is_authenticated();
    let heavyaccept = rc.rules.is_accept_heavy();
    let (peerheavy,queryheavy,poolheavy) = rc.rules.is_routing_heavy();
 
    let local = *clientmode == ClientMode::Local(true) || *clientmode == ClientMode::Local(false);
    match r.recv() {
      Ok(PeerMgmtMessage::PeerAuth(node,sign)) => {
        let k = node.get_key();
        // pong received in server
        let onenewprio = match route.get_node(&k) {
          Some(ref p) => {
            match p.1 {
              PeerState::Ping(ref chal, ref prio) => {
                // check auth with peer challenge
                if rc.peerrules.checkmsg(&(*p.0),&chal,&sign) {
                  if heavyaccept {
                  // spawn accept if it is heavy accept to get prio (prio stored in ping is
                  // unchecked)
                    assert!(*prio == PeerPriority::Unchecked);
                    let rcthread = rc.clone();
                    let rpthread = rp.clone();
                    thread::scoped(move ||{
                      let oprio = rcthread.peerrules.accept(&node, &rpthread, &rcthread);
                      match oprio {
                        Some(prio) => {
                          rpthread.peers.send(PeerMgmtMessage::PeerUpdatePrio(Arc::new(node),PeerState::Online(prio)));
                        },
                        None => {
                          // from client & from server to close both TODO a message for both?? (or
                          // shared message)
                          let afrom = Arc::new(node);
                          rpthread.peers.send(PeerMgmtMessage::PeerRemFromClient(afrom.clone(), PeerStateChange::Refused));
                          rpthread.peers.send(PeerMgmtMessage::PeerRemFromServer(afrom, PeerStateChange::Refused));
                        },
                      }
                    });
                    None
                  } else {
                    Some((PeerState::Online(prio.clone()),false))
                  }
                } else {
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
            rp.peers.send(PeerMgmtMessage::PeerPing(Arc::new(node),None));
            None
          },
        };
        onenewprio.map(|(prio, rm)|{
          route.update_priority(&k,prio);
          if rm {
            // close connection
            route.remchan(&k);
          };
        }).is_none();
      },
      Ok(PeerMgmtMessage::ClientMsg(msg, key)) => {
        // TODO get from route and send with client info (either direct or with channel)
      },
      Ok(PeerMgmtMessage::PeerRemFromClient(p,pschange))  => {
        // TODO if needed close server (remchan : plus bool to close either both handle plus state
        // to block if do block (instead of offline)
        // opt (none means remove totally))
        let nodeid = p.get_key().clone();
        route.remchan(&nodeid);
        // TODO update prio if blocked keep peer
      },
      Ok(PeerMgmtMessage::PeerRemFromServer(p,pschange))  => {
        // TODO if needed close Clinet
        let nodeid = p.get_key().clone();
        route.remchan(&nodeid);
        // TODO update prio to offline with old accept result or oprio
      },
 
      Ok(PeerMgmtMessage::PeerAddOffline(p,tryconnect)) => {
        info!("Adding offline peer : {:?}", p);
        let nodeid = p.get_key().clone(); // TODO to avoid this clone create a route.addnode which has optional priority
        let (hasnode, pri) = match route.get_node(&nodeid) { // TODO new function route.hasP
          Some(p) => {(true,p.1.get_priority())},
          None => {(false,PeerPriority::Unchecked)},
        };
        if(!hasnode){
          route.add_node((p,PeerState::Offline(pri),None,None));
        } else {
          route.update_priority(&nodeid,PeerState::Offline(pri));
        }
        if tryconnect {
          // TODO tryconnect by sending ping...
        };
 
 
      },
      Ok(PeerMgmtMessage::PeerPong(_,_,_))  => {
        //TODO!!!
      },
      Ok(PeerMgmtMessage::PeerAddFromClient(p,prio,ors))  => {
        //TODO!!!
      },
      Ok(PeerMgmtMessage::PeerAddFromServer(p,prio,ows,ssend,servinfo))  => {
        if(p.get_key() != rc.me.get_key()) {
        info!("Adding peer : {:?}, {:?} from {:?}", p, prio, rc.me);
          // TODO  when test writen (change of prio) try to change with get_mut
          // TODO when offline or blocked remove s
          let nodeid = p.get_key().clone(); // TODO to avoid this clone create a route.addnode which has optional priority
          let hasnode = match route.get_node(&nodeid) { // TODO new function route.hasP
            Some(_) => {true},
            None => {false},
          };
          debug!("-Update peer prio : {:?}, {:?} from {:?}", p, prio, rc.me);
          // TODO manage pri = unchecked meaning we got a ping from server but we are heavy accept
          // so we may pong and so we p
          let pstate = if prio == PeerPriority::Unchecked {
            // TODO send peerping??? and put state ping instead
            PeerState::Offline(prio)
          } else {
            PeerState::Online(prio)
          };
          if(!hasnode){
            debug!("-init up");
            route.add_node((p,pstate,None,None));
          } else {
            debug!("-actual up");
            route.update_priority(&nodeid,pstate);
          }
        } else {
          error!("Trying to add ourselves as a peer");
        }
      },
      Ok(PeerMgmtMessage::PeerUpdatePrio(p,prio)) => {
        debug!("Update peer prio : {:?}, {:?} from {:?}", p.get_key(), prio, rc.me.get_key());
        route.update_priority(&p.get_key(),prio);
      },
      Ok(PeerMgmtMessage::PeerQueryPlus(p)) => {
        debug!("Adding peer query ");
        route.query_count_inc(&p.get_key());
      },
      Ok(PeerMgmtMessage::PeerQueryMinus(p)) => {
        debug!("Adding peer query ");
        route.query_count_dec(&p.get_key());
      },
      Ok(PeerMgmtMessage::PeerPing(p, ores)) => {
        if(p.get_key() != rc.me.get_key()) {
          debug!("Pinging peer : {:?}", p);
          if(!local){  
            get_or_init_client_connection::<RT, T>(&p, & rc , & mut route, & rp, true, ores);
          } else {
            // TODO  ores non supported (no ping query cache - TODO )
            send_nonconnected_ping::<RT, T>(&p, & rc , & mut route, & rp);
          };
        } else {
          error!("Trying to ping ourselves");
        }
      },
      Ok(PeerMgmtMessage::KVFind(key, oquery, queryconf))  => {
        let remhop = queryconf.rem_hop;
        let nbquery = queryconf.nb_forw;
        let qp = queryconf.prio;
        if remhop > 0 {
          // get closest node to query
          // if no result launch (spawn) discovery processing
          debug!("!!!in peer find of procman bef get closest");
          let peers = match queryconf.hop_hist {
            None => route.get_closest_for_query(&key, nbquery, &VecDeque::new()),
            Some(LastSent::LastSentPeer(_, ref filter)) | Some(LastSent::LastSentHop(_, ref filter))  => route.get_closest_for_query(&key, nbquery, filter),
          };
          let mut newqueryconf = update_lastsent_conf ( &queryconf , &peers , nbquery);
          let rsize = peers.len();
          // update number of result to expect for each proxyied request
          let nbqus = rsize.to_usize().unwrap();
          let rnbres = newqueryconf.nb_res;
          let newrnbres = if nbqus == 0 {
            rnbres
          } else {
            let newrnbres_round = rnbres / nbqus;
            if (newrnbres_round * nbqus) < rnbres {
              // we prefer to send to much query than the other way (could be a lot more)
              newrnbres_round + 1
            } else {
              newrnbres_round
            }
          };
          newqueryconf.nb_res = newrnbres;
          match oquery {
            Some(ref query) => {
              // adjust nb request
              query.lessen_query((nbquery.to_usize().unwrap() - rsize.to_usize().unwrap()),&rp.peers);
            },
            None => {
              if rsize == 0 {
                // no proxy should reply None (send to ourselve)
                rp.peers.send(PeerMgmtMessage::StoreKV(queryconf.clone(), None));
              };
            }, 
          };
 
          for p in peers.iter(){
            let mess =  ClientMessage::KVFind(key.clone(),oquery.clone(), newqueryconf.clone()); // TODO queryconf arc?? + remove newqueryconf.clone() already cloned why the second (bug or the fact that we send)
            if(!local) {
              // get connection
              let s = get_or_init_client_connection::<RT, T>(p, & rc , & mut route, & rp, false, None);
              s.send(mess);
            } else {
              send_nonconnected::<RT, T> (p, & rc , & mut route, & rp, mess);
            };
          };
        } else {
          oquery.map(|q|q.release_query(&rp.peers));
        }
      },
      Ok(PeerMgmtMessage::PeerFind(nid, oquery, queryconf))  => {
        let remhop = queryconf.rem_hop;
        let nbquery = queryconf.nb_forw;
        let qp = queryconf.prio;
   
        // query ourselve if here and local // no ping at this point : trust current
        // status, proxyto is either right with destination or left with a possible result in
        // parameter
        let proxyto = match route.get_node(&nid) {
          // blocked peer are also blocked for transmission (this is likely to make them
          // disapear) TODO might not be a nice idea
          Some(&(ref ap,PeerState::Blocked(_), ref s,_)) => Either::Left(None),
          // no ping at this point : trust current status
          Some(&(ref ap,PeerState::Online(_), ref s,_)) => Either::Left(Some((*ap).clone())),
          // None or offline (offline may mean we need updated info for peer
          _ => {
            if remhop > 0 {
              // get closest node to query
              // if no result launch (spawn) discovery processing
              let peers = match queryconf.hop_hist {
                None => route.get_closest_for_node(&nid, nbquery, &VecDeque::new()),
                Some(LastSent::LastSentPeer(_, ref filter)) | Some(LastSent::LastSentHop(_, ref filter))  => 
                  route.get_closest_for_node(&nid, nbquery, filter),
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
        };
        match proxyto {
          Either::Left(r) => {
            match oquery {
              Some(query) => {
                // put result !! in a spawn (do not want to manipulate mutex here (even
                let querysp = query.clone();
                let ssp = rp.peers.clone();
                let srp = rp.store.clone();
                // TODO remove this spawn even if dealing with mutex (costy?? )
                thread::scoped(move || {
                  println!("!!!!found not sent unlock semaphore");
                  if (r != None) {
                    if querysp.set_query_result(Either::Left(r),&srp) {
                      querysp.release_query(& ssp);
                    } else {
                      querysp.lessen_query(1, & ssp);
                    }
                  };
                });
              },
              None => {
                // Here no query object created
                // eg Async hop reply directly
                debug!("!!!AsyncResult returning {:?}", r);
                match queryconf.modeinfo.get_rec_node().map(|r|r.clone()) {
                  Some (ref recnode) => {
                    let mess = ClientMessage::StoreNode(queryconf.modeinfo.to_qid(), r);
                    if (!local){
                      // send result directly
                      let s = get_or_init_client_connection::<RT, T>(&recnode.clone(), & rc , & mut route, & rp, false, None);
                      s.send(mess);
                    } else {
                       send_nonconnected::<RT, T> (recnode, & rc , & mut route, & rp, mess);
                    };
                  },
                  _ => {error!("None query in none asynch peerfind");},
                 }
               },
            };
          },
          Either::Right((newqueryconf,peers)) => {
            match oquery {
              Some(ref query) => {
                let rsize = peers.len();
                 // adjust nb request
                 // Note that it doesnot prevent unresponsive client
                 query.lessen_query((nbquery.to_usize().unwrap() - rsize.to_usize().unwrap()),&rp.peers);
               },
               _ => (), 
             };
             for p in peers.iter() {
               let mess = ClientMessage::PeerFind(nid.clone(),oquery.clone(), newqueryconf.clone()); // TODO queryconf arc??
               if (!local) {
                 // get connection
                 let s = get_or_init_client_connection::<RT, T>(p, & rc , & mut route, & rp, false, None);
                 s.send(mess);
               } else {
                 send_nonconnected::<RT, T> (p, & rc , & mut route, & rp, mess);
               };
             };
           },
         }
       },
       Ok(PeerMgmtMessage::Refresh(max))  => {
         info!("Refreshing connection pool");
         let torefresh = route.get_pool_nodes(max);
         for n in torefresh.iter(){
           send_nonconnected_ping::<RT, T>(n, & rc , & mut route, & rp);
         }
       },
       Ok(PeerMgmtMessage::ShutDown)  => {
         info!("Shudown receive");
         break;
       },
       Ok(PeerMgmtMessage::StoreNode(qconf, result)) => {
         match qconf.modeinfo.get_rec_node().map(|r|r.clone()) {
           Some (rec)  => {
             if rec.get_key() != rc.me.get_key() { 
               let mess = ClientMessage::StoreNode(qconf.modeinfo.to_qid(), result);
               if(!local){
                 let s = get_or_init_client_connection::<RT, T>(& rec, & rc , & mut route, & rp, false, None); // TODO do something for not having to create an arc here eg arc in qconf + previous qconf clone
                 s.send(mess);
               } else {
                 send_nonconnected::<RT, T> (&rec, & rc , & mut route, & rp, mess);
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
       Ok(PeerMgmtMessage::StoreKV(qconf, result)) => {
         // (query sync over clients(the query contains nothing)) // TODO factorize a fun
         // for proxied msg 
         match qconf.modeinfo.get_rec_node().map(|r|r.clone()) {
           Some (rec) => {
             if rec.get_key() != rc.me.get_key() {
               let mess = ClientMessage::StoreKV(qconf.modeinfo.to_qid(), qconf.chunk, result);
               if (!local) {
                 let s = get_or_init_client_connection::<RT, T>(& rec, & rc , & mut route, & rp, false, None);
                 s.send(mess);
               } else {
                 send_nonconnected::<RT, T> (& rec, & rc , & mut route, & rp, mess);
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
       Err(pois) => {
         error!("channel pb"); // TODO something to relaunch peermgmt
       }

    }
  }
  route.commit_store();
  sem.release();
}


#[inline]
fn send_nonconnected<RT : RunningTypes, T : Route<RT::P,RT::V,RT::T>>
 (p : & Arc<RT::P> , 
  rc : & ArcRunningContext<RT>, 
  route : & mut T , 
  rp : & RunningProcesses<RT>,
  mess : ClientMessage<RT::P,RT::V>
 )
 ->  bool {
 match route.get_node(&p.get_key()) {
  Some(&(_,PeerState::Blocked(_),_,_)) => {
      // if existing consider ping true : normaly if offline or block , no channel
      info!("#####blocked : client do not send");
      false
  },
  _ => {
  let rpsp = rp.clone();
  let rcsp = rc.clone();
  let psp = p.clone();
  thread::scoped (move || {client::start::<RT>(psp, None, None, rcsp, rpsp, false, None, Some(mess),false)});
  true
  },
 }
}

#[inline]
fn send_nonconnected_ping<RT : RunningTypes, T : Route<RT::P,RT::V,RT::T>>
 (p : & Arc<RT::P> , 
  rc : & ArcRunningContext<RT>, 
  route : & mut T , 
  rp : & RunningProcesses<RT>,
 )
 ->  bool {
 match route.get_node(&p.get_key()) {
  Some(&(_,PeerState::Blocked(_),_,_)) => {
      // if existing consider ping true : normaly if offline or block , no channel
      info!("#####blocked : client do not send");

      false // TODO avoid this clone
  },
  _ => {
  let rpsp = rp.clone();
  let rcsp = rc.clone();
  let psp = p.clone();
  thread::scoped (move || {client::start::<RT>(psp, None, None, rcsp, rpsp, true, None, None,false)});
  true
  },
 }
}


#[inline]
fn get_or_init_client_connection<RT : RunningTypes, T : Route<RT::P,RT::V,RT::T>>
 (p : & Arc<RT::P> , 
  rc : & ArcRunningContext<RT>, 
  route : & mut T , 
  rp : & RunningProcesses<RT>,
  ping : bool, 
  pingres : Option<OneResult<bool>>) 
 ->  Sender<ClientMessage<RT::P,RT::V>> {
   // TODO refactor : client info cannot be clone so we may update diferently
   /*
let (upd, s) = match route.get_node(&p.get_key()) {
  Some(&(_,PeerPriority::Blocked,_,_))| Some(&(_,PeerPriority::Offline,_,_)) => {
    // retry connecting & accept for block
    let (tcl,rcl) = channel();
    debug!("##pingtrue");
    (Some(rcl), ClientInfo::Threaded(tcl,0))
  },
  // TODO new sendre of clinet
  Some(&(_,_,_, Some(ref s))) => {
    
     debug!("#####get or init find with chanel");
     (None, (*s).clone()) // TODO avoid this clone
    
  },
  None | Some(&(_, _, _,None)) => {
  let (tcl,rcl) = channel();
  (Some(rcl), ClientInfo::Threaded(tcl,0))
  },
 };
 match upd {
     Some(rcl) => {

     debug!("#####putting channel");
     route.add_node((p.clone(),PeerPriority::Offline,None,Some(s.clone())));


  let tcl3 = s.clone();

  // dup value for spawn TODO switch to arc!!!
  let rpsp = rp.clone();
  //when tcl store later
  let rcsp = rc.clone();
  let psp = p.clone();
  debug!("#####initiating client process from {:?} to {:?} with ping {:?}",rc.me.get_key(), p.get_key(),ping);
  thread::scoped (move || {client::start::<RT>(psp,Some(tcl3), Some(rcl), rcsp, rpsp, ping, pingres, None, true)});
     },
     None => {
       // we found an existing channel with seemlessly open connection
      pingres.map(|ares|{
              utils::ret_one_result(&ares, true)
      });

     },
 };
 s.clone()
 */
    let (tcl,rcl) = channel();
    tcl
}

#[inline]
fn update_lastsent_conf<P : Peer> ( queryconf : &QueryMsg<P>,  peers : &Vec<Arc<P>>, nbquery : u8) -> QueryMsg<P> {
  let mut newqueryconf = queryconf.clone();
  newqueryconf.hop_hist = match newqueryconf.hop_hist {
    Some(LastSent::LastSentPeer(maxnb,mut lpeers)) => {
      let totalnb =  (peers.len() + lpeers.len());
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
      if(hop > 0){
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
