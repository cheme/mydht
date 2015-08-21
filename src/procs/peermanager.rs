use procs::mesgs::{PeerMgmtMessage,ClientMessage};
use peer::{PeerMgmtMeths, PeerPriority,PeerState,PeerStateChange};
use std::sync::mpsc::{Receiver};
use procs::{client,ArcRunningContext,RunningProcesses,RunningTypes};
use std::collections::{VecDeque};
use std::sync::{Arc,Semaphore,Mutex};
use query::{LastSent,QueryMsg};
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
use procs::server::start_listener;
use utils::TransientOption;
use mydhtresult::{Error,ErrorKind};
use mydhtresult::Result as MDHTResult;


/// TODO expect problem if pool of server managed by peermgmt & client local to peermgmt : client
/// create will wait for a reply by its same thread : Pool of server handle is needed in start
/// client param (direct if client local, sender if client threaded)
/// This type is close to either but it is deconstructed before use (so we do not have to implement
/// send or sync for local handle.
/// ClientPool does not require such handle as client is always started from peer manager
/// TODO Need a option server pool as parameter of start client. And a start distant fn wrapper
/// without this (could not be send to process).
/// TODO replace resend of message to ourselve by fn call
pub enum ServerPoolHandle<'a> {
  /// TODO replace by ServerPool type
  Local(&'a String),
  /// pool message in peermgmt for now, so use of sender of running process TODO see if othe pool process
  Threaded,
}


// peermanager manage communication with the storage : add remove update. The storage is therefore
// not shared, we use message passing. 
/// Start a new peermanager process
pub fn start<RT : RunningTypes, 
  T : Route<RT::A,RT::P,RT::V,RT::T>,
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
    let localspawn = *clientmode == ClientMode::Local(true);
    match r.recv() {
      Ok(PeerMgmtMessage::PeerAuth(node,sign)) => {
        let k = node.get_key();
        // pong received in server
        let onenewprio = match route.get_node(&k) {
          Some(ref p) => {
            match p.1 {
              PeerState::Ping(ref chal, ref ores, ref prio) => {
                // check auth with peer challenge
                if rc.peerrules.checkmsg(&(*p.0),&chal,&sign) {
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
                          rpthread.peers.send(PeerMgmtMessage::PeerUpdatePrio(afrom.clone(),PeerState::Online(prio)));
                        },
                        None => {
                          // from client & from server to close both TODO a message for both?? (or
                          // shared message)
                          rpthread.peers.send(PeerMgmtMessage::PeerRemFromClient(afrom.clone(), PeerStateChange::Refused));
                          rpthread.peers.send(PeerMgmtMessage::PeerRemFromServer(afrom.clone(), PeerStateChange::Refused));
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
            rp.peers.send(PeerMgmtMessage::PeerPing(Arc::new(node),None));
            None
          },
        };
        onenewprio.map(|(prio, rm)|{
          route.update_priority(&k,Some(prio),None);
          if rm {
            // close connection
            route.remchan(&k,&rc.transport);
          };
        }).is_none();
      },
      Ok(PeerMgmtMessage::ClientMsg(msg, key)) => {
        //  get from route and send with client info (either direct or with channel)
        if local {
          match cli_local_send::<RT,_>(&mut route, &key,msg) {
            Ok(upd) => {
              if upd == false {
                // no peer
                error!("No handle for cli when server exists (local)");
              }

            },
            Err(e) => {
                error!("Error when client sending locally to peermgmt : {:?}",e);
            },
          }
        } else {
          // TODO replace by get_or_init info, then send fn with local in param
          match route.get_node(&key) {
            Some(&(_,_, (_,Some(ref pi)))) => {
              pi.send_climsg(msg);
            },
            Some(&(_,_, (_,None))) => {
              // TODO start cli process
              error!("No handle for cli when server exists");
            },
            None => (),
          };
        }
      },
      Ok(PeerMgmtMessage::PeerRemFromClient(p,pschange))  => {
        let nodeid = p.get_key().clone();
        // close all info
        route.remchan(&nodeid,&rc.transport);
        // update prio
        route.update_priority(&nodeid,None,Some(pschange));
      },
      Ok(PeerMgmtMessage::PeerRemFromServer(p,pschange))  => {
        let nodeid = p.get_key().clone();
        // close all info
        route.remchan(&nodeid,&rc.transport);
        // update prio
        route.update_priority(&nodeid,None,Some(pschange));
      },
      Ok(PeerMgmtMessage::PeerAddOffline(p)) => {
        info!("Adding offline peer : {:?}", p);
        let nodeid = p.get_key().clone();
        let hasnode = route.has_node(&nodeid);
        if !hasnode {
          route.add_node((p,PeerState::Offline(PeerPriority::Unchecked),(None,None)));
        } else {
          route.update_priority(&nodeid,None,Some(PeerStateChange::Offline));
        };
      },
      Ok(PeerMgmtMessage::PeerPong(p,prio,chal,ows))  => {
        if(p.get_key() != rc.me.get_key()) {
          // a ping has been received we need to reply with pong : this may also init the peer
          let nodeid = p.get_key().clone();
          let hasnode = route.has_node(&nodeid);
          let upd = !hasnode || match route.get_node(&nodeid) {
          //  is offline TODO blocked??
            Some(&(_,PeerState::Offline(_), _)) => true,
            _ => false,
          };
          if upd {
            peer_ping(&rc,&rp,&mut route,&p,None,prio,heavyaccept,localspawn,local,&clientmode);
          };

          // send pong to cli
          let mess = ClientMessage::PeerPong(chal);
          if !local {
            route.get_client_info(&p).and_then(|ci|
              ci.send_climsg(mess));
          } else {
            if !send_local::<RT,T>(&p,&rc,& mut route,&rp,mess,localspawn) {
              error!("send_local_pong_reply failure from peerman");
            };
          };
        } else {
          error!("Trying to ping ourselve");
        }
      },
      Ok(PeerMgmtMessage::PeerAddFromServer(p,prio,ows,ssend,servinfo))  => {
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
              match init_client_info(&p,&rc,&rp,ows,&clientmode) {
                Ok(r) => (Some(r.0),r.1),
                Err(e) => {
                  error!("cannot connect client connection (a server connection was connected but without send handle, putting peer offline");
                  route.remchan(&nodeid,&rc.transport);
                  // update prio
                  route.update_priority(&nodeid,None,Some(PeerStateChange::Offline));
 
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
              ssend.send(clihandler);
              // on send of new clihandler to server (either new peer or connection lost to server
              // and reinit, start a replying ping (pong is sent directly)
              peer_ping(&rc,&rp,&mut route,&p,None,prio,heavyaccept,localspawn,local,&clientmode);
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
      Ok(PeerMgmtMessage::ServerInfoFromClient(p,serinfo))  => {
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
            serinfo.shutdown(&rc.transport, &(*p));
          },
          Err(e) => {
            // TODO switch to panic??
            error!("Error on updating a client in route {}",e);
          },
          _ => (),
        };
      },
      Ok(PeerMgmtMessage::PeerChangeState(p,prio)) => {
        debug!("Change peer state : {:?}, {:?} from {:?}", p.get_key(), prio, rc.me.get_key());
        route.update_priority(&p.get_key(),None,Some(prio));
      },
      Ok(PeerMgmtMessage::PeerUpdatePrio(p,prio)) => {
        debug!("Update peer prio : {:?}, {:?} from {:?}", p.get_key(), prio, rc.me.get_key());
        route.update_priority(&p.get_key(),Some(prio),None);
      },
      Ok(PeerMgmtMessage::PeerQueryPlus(p)) => {
        debug!("Adding peer query ");
        route.query_count_inc(&p.get_key());
      },
      Ok(PeerMgmtMessage::PeerQueryMinus(p,_)) => {
        debug!("Adding peer query ");
        route.query_count_dec(&p.get_key());
      },
      Ok(PeerMgmtMessage::PeerPing(p, ores)) => {
        peer_ping(&rc,&rp,&mut route,&p,ores.clone(),PeerPriority::Unchecked,heavyaccept,localspawn,local,&clientmode)
          .map_err(|_|
            route.update_priority(&p.get_key(),None,Some(PeerStateChange::Offline)));
        /* return false already in drop of peerping state
            ores.map(|res|
              utils::ret_one_result(&res,false))); */
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
          match oquery {
            Some(ref query) => {
              // adjust nb request
              let nftresh = rc.rules.notfoundtreshold(nbquery, remhop, &newqueryconf.modeinfo.get_mode());
              let nbless = calc_adj(nbquery, rsize, nftresh);
              query.lessen_query(nbless,&rp.peers);
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
              route.get_client_info(p).and_then(|ci|
                ci.send_climsg(mess));
              // TODO mult ix
            } else {
              send_local::<RT, T> (p, & rc , & mut route, & rp, mess, localspawn);
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
          Some(&(ref ap,PeerState::Blocked(_), _)) => Either::Left(None),
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
          // result local due to not proxied (none) or result found localy
          Either::Left(r) => {
            match oquery {
              Some(query) => {
                // put result !! in a spawn (do not want to manipulate mutex here (even
                let querysp = query.clone();
                let ssp = rp.peers.clone();
                let srp = rp.store.clone();
                // TODO remove this spawn even if dealing with mutex (too costy?? )
                thread::spawn(move || {
                  debug!("!!!!found not sent unlock semaphore");
                  if (r == None) {
                    // No update of result so will reply none
                    querysp.release_query(& ssp);
                  } else {
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
                // eg Async hop reply directly TODO call storepeer to recnode
                // TODO fn
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
                      peer_ping(&rc,&rp,&mut route,recnode,None,PeerPriority::Unchecked,heavyaccept,localspawn,local,&clientmode).is_ok()
                    } else {
                      true
                    };
                    if ok && !local {
                      // send result directly
                      route.get_client_info(&recnode.clone()).and_then(|ci|
                        ci.send_climsg(mess)); // TODO borderline as no auth : just proxy (a ping has been send but without checking of pong : we need to trust query from : implement a condition to send message on peerauth (add msg to peerping state)
                    } else if ok {
                       send_local::<RT, T> (recnode, & rc , & mut route, & rp, mess, localspawn);
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
                // TODO nf treashold in queryconf??
                let nftresh = rc.rules.notfoundtreshold(nbquery, remhop, &newqueryconf.modeinfo.get_mode());
                let nbless = calc_adj(nbquery, rsize, nftresh);
                // Note that it doesnot prevent unresponsive client
                query.lessen_query(nbless,&rp.peers);
               },
               _ => (), 
             };
             for p in peers.iter() {
               let mess = ClientMessage::PeerFind(nid.clone(),oquery.clone(), newqueryconf.clone()); // TODO queryconf arc??
               if !local {
                 // get connection
                 route.get_client_info(p).and_then(|ci|
                   ci.send_climsg(mess));
               } else {
                 send_local::<RT, T> (p, & rc , & mut route, & rp, mess, localspawn);
               };
             };
           },
         }
       },
       Ok(PeerMgmtMessage::Refresh(max))  => {
         info!("Refreshing connection pool");
         let torefresh = route.get_pool_nodes(max);
         for n in torefresh.iter(){
           if local {
             send_local_ping::<RT, T>(n, & rc , & mut route, & rp,localspawn,None);
           } else {
             // normal process
             peer_ping(&rc,&rp,&mut route,n,None,PeerPriority::Unchecked,heavyaccept,localspawn,local,&clientmode).is_ok();
           }
         }
       },
       Ok(PeerMgmtMessage::ShutDown)  => {
         info!("Shudown receive");
         break;
       },
       Ok(PeerMgmtMessage::StoreNode(qconf, result)) => {
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
                 peer_ping(&rc,&rp,&mut route,&rec,None,PeerPriority::Unchecked,heavyaccept,localspawn,local,&clientmode).is_ok()
               } else {
                 true
               };
 
               if ok && !local {
                 route.get_client_info(& rec).and_then(|ci|
                   ci.send_climsg(mess)); // TODO do something for not having to create an arc here eg arc in qconf + previous qconf clone
               } else if ok {
                 send_local::<RT, T> (&rec, & rc , & mut route, & rp, mess, localspawn);
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
             let nodeid = rec.get_key();
             if nodeid != rc.me.get_key() {
               let mess = ClientMessage::StoreKV(qconf.modeinfo.to_qid(), qconf.chunk, result);
               // TODO this upd check should be only for asynch modes
               let upd = !route.has_node(&nodeid) || match route.get_node(&nodeid) {
                 Some(&(_,PeerState::Offline(_), _)) => true,
                 _ => false,
               };
               let ok = if upd {
                 peer_ping(&rc,&rp,&mut route,&rec,None,PeerPriority::Unchecked,heavyaccept,localspawn,local,&clientmode).is_ok()
               } else {
                 true
               };
               if !local && ok {
                 route.get_client_info(& rec).and_then(|ci|
                   ci.send_climsg(mess));
               } else if ok {
                 send_local::<RT, T> (& rec, & rc , & mut route, & rp, mess, localspawn);
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
// TODO  complete to start client process (more param : see send_local or send_local_ping
fn cli_local_send<RT : RunningTypes, T : Route<RT::A,RT::P,RT::V,RT::T>> 
(route : &mut T, nodeid : &<RT::P as KeyVal>::Key, msg : ClientMessage<RT::P,RT::V>) -> MydhtResult<bool> {
  route.update_infos(nodeid, |sercli|match sercli.1 {
      Some(ref mut ci) => ci.send_climsg_local(msg),
      None => {
        error!("local send use on no local clinet info");
        Ok(())
      },
    })
}


#[inline]
// could not initiate client info of route, it is considered that we need to reping
// TODO return result
fn send_local<RT : RunningTypes, T : Route<RT::A,RT::P,RT::V,RT::T>>
 (p : & Arc<RT::P> , 
  rc : & ArcRunningContext<RT>, 
  route : & mut T , 
  rp : & RunningProcesses<RT>,
  mess : ClientMessage<RT::P,RT::V>,
  localspawn : bool,
 )
 -> bool {
   // TODO first route.updateinfo  (and send_mesg_local on ci), then if Err with kind no Cliinfo in
   // route : create cliinfo by transport connect (even for spawn true : cannot connect in new
   // thread or we may connect numerous times).
 
 let key = p.get_key();
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
     Some(mutws) => {
       let rpsp = rp.clone();
       let rcsp = rc.clone();
       let psp = p.clone();
       let mpsp = p.clone();
       let erpeer = rpsp.peers.clone();
       thread::spawn (move || {
         if ! client::start::<RT>(&psp, None, rcsp, rpsp, Some(mess),mutws,false) {
           // TODO try reconnect or add it to client directly
           erpeer.send(PeerMgmtMessage::PeerRemFromClient(psp, PeerStateChange::Offline));
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
             let mpsp = p.clone();
             let osend = cli.get_mut_sender();
             assert!(osend.is_some());
             if !try!(client::start_local::<RT>(&p, None, &rc, &rp, Some(mess),osend,false)) {
               rem = true;
               rec = true;
             }
           },
           None => {
             let (mut cli, oser) = try!(init_client_info(&p, &rc, &rp, None, &ClientMode::Local(false)));
             sercli.1 = Some(cli);
             if let Some(ref mut clii) = sercli.1 {
               let osend = clii.get_mut_sender();
               assert!(osend.is_some());
             
               let mpsp = p.clone();
               if !try!(client::start_local::<RT>(&p, None, &rc, &rp, Some(mess),osend,false)) {
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
     route.update_priority(&key,None,Some(PeerStateChange::Offline));
     route.remchan(&key,&rc.transport);
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
  ores : Option<OneResult<bool>>,
 )
 ->  bool {

   let chal = rc.peerrules.challenge(&(*p));
   let mess = ClientMessage::PeerPing(p.clone(), chal.clone());
   let r = send_local(p,rc,route,rp,mess,localspawn);
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
      let ci = ClientInfo::Threaded(tcl,0);
      let psp = p.clone();
      let rcsp = rc.clone();
      let rpsp = rp.clone();

      thread::spawn (move || {client::start::<RT>(&psp, Some(rcl), rcsp, rpsp, None, ows.map(|ws|ClientSender::Threaded(ws)), true)});
      Ok((ci,None))
    },
    _ => {panic!("TODO implement cli pools")},
  }
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

#[inline]
pub fn init_local<P : Peer, V : KeyVal, T : Transport> (cm : &ClientMode, ows : Option<T::WriteStream>) -> Option<ClientInfo<P,V,T>> {
  match cm {
    &ClientMode::Local(false) => ows.map(|ws|ClientInfo::Local(ClientSender::Local(ws))),
    &ClientMode::Local(true) => ows.map(|ws|ClientInfo::LocalSpawn(ClientSender::LocalSpawn(Arc::new(Mutex::new(ws))))),
    _ => None,
  }
}

/// process to run for pinging a peer, for new peer, peer initialization is don
pub fn peer_ping <RT : RunningTypes, 
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
      init_client_info(&p,&rc,&rp,None,clientmode).map(|ci|{
        assert!(!(ci.1.is_some() && hasserv));
        route.add_node((p.clone(),PeerState::Offline(pri),(ci.1,Some(ci.0))));
      }).is_ok()
    });
    debug!("Pinging peer : {:?}", p);
    if ok && !local {
      let chal = rc.peerrules.challenge(&(*p));
      route.update_priority(&nodeid,None,Some(PeerStateChange::Ping(chal.clone(),TransientOption(ores))));
      route.get_client_info(&p).and_then(|ci|
        ci.send_climsg(ClientMessage::PeerPing(p.clone(),chal)))
    } else if ok {
      if !send_local_ping::<RT, T>(p, rc , route, rp,localspawn,ores) {
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

