use rustc_serialize::json;
use procs::mesgs::{self,PeerMgmtMessage,ClientMessage};
use std::io::Result as IoResult;
use peer::{PeerMgmtRules, PeerPriority};
use std::sync::mpsc::{Sender,Receiver};
use std::str::from_utf8;
use procs::{client,RunningContext,ArcRunningContext,RunningProcesses,RunningTypes};
use std::collections::{HashMap,BTreeSet,VecDeque};
use std::sync::{Arc,Semaphore,Condvar,Mutex};
use query::{self,QueryRules,QueryModeMsg,LastSent,QueryConfMsg};
use route::Route;
use std::sync::mpsc::channel;
use std::thread;
use transport::{Transport,TransportStream};
use peer::Peer;
use utils::{self,OneResult,Either};
use keyval::{KeyVal};
use msgenc::{MsgEnc};
use num::traits::ToPrimitive;


// peermanager manage communication with the storage : add remove update. The storage is therefore
// not shared, we use message passing. 
/// Start a new peermanager process
pub fn start<RT : RunningTypes, T : Route<RT::P,RT::V>>
 (rc : ArcRunningContext<RT>,
  mut route : T, 
  r : &Receiver<PeerMgmtMessage<RT::P,RT::V>>, 
  rp : RunningProcesses<RT::P,RT::V>, 
  sem : Arc<Semaphore>) {
  loop {
    let connected = <RT::T as Transport>::is_connected();
    match r.recv() {
      Ok(PeerMgmtMessage::PeerRemChannel(p))  => {
        let nodeid = p.get_key().clone();
        route.remchan(&nodeid);
      },
      Ok(PeerMgmtMessage::PeerAdd(p,prio))  => {
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
          if(!hasnode){
            debug!("-init up");
            route.add_node(p,None);
          }
          debug!("-actual up");
          route.update_priority(&nodeid,prio);
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
          if(connected){  
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
        let remhop = query::get_nbhop(&queryconf);
        let nbquery = query::get_nbquer(&queryconf);
        let qp = query::get_prio(&queryconf);
        if remhop > 0 {
          // get closest node to query
          // if no result launch (spawn) discovery processing
          debug!("!!!in peer find of procman bef get closest");
          let peers = match queryconf.2 {
            None => route.get_closest_for_query(&key, nbquery, &VecDeque::new()),
            Some(LastSent::LastSentPeer(_, ref filter)) | Some(LastSent::LastSentHop(_, ref filter))  => route.get_closest_for_query(&key, nbquery, filter),
          };
          let mut newqueryconf = update_lastsent_conf ( &queryconf , &peers , nbquery);
          let rsize = peers.len();
          // update number of result to expect for each proxyied request
          let nbqus = rsize.to_usize().unwrap();
          let rnbres = query::get_req_nb_res(&newqueryconf);
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
          newqueryconf.7 = newrnbres;
          match oquery {
            Some(ref query) => {
              // adjust nb request
              query.lessen_query((nbquery.to_usize().unwrap() - rsize.to_usize().unwrap()),&rp.peers);
            },
            None => {
              if rsize == 0 {
                // no proxy should reply None (send to ourselve)
                rp.peers.send(PeerMgmtMessage::StoreKV(queryconf, None));
              };
            }, 
          };
 
          for p in peers.iter(){
            let mess =  ClientMessage::KVFind(key.clone(),oquery.clone(), newqueryconf.clone()); // TODO queryconf arc?? + remove newqueryconf.clone() already cloned why the second (bug or the fact that we send)
            if(connected) {
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
        let remhop = query::get_nbhop(&queryconf);
        let nbquery = query::get_nbquer(&queryconf);
        let qp = query::get_prio(&queryconf);
   
        // query ourselve if here and local // no ping at this point : trust current
        // status, proxyto is either right with destination or left with a possible result in
        // parameter
        let proxyto = match route.get_node(&nid) {
          // no ping at this point : trust current status
          Some(&(ref ap,PeerPriority::Normal, ref s)) | Some(&(ref ap,PeerPriority::Priority(_), ref s)) => Either::Left(Some((*ap).clone())),
          // blocked peer are also blocked for transmission (this is likely to make them
          // disapear) TODO might not be a nice idea
          Some(&(ref ap,PeerPriority::Blocked, ref s)) => Either::Left(None),
          // None or offline (offline may mean we need updated info for peer
          _ => {
            if remhop > 0 {
              // get closest node to query
              // if no result launch (spawn) discovery processing
              let peers = match queryconf.2 {
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
                thread::spawn(move || {
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
                match queryconf.0.clone().get_rec_node() {
                  Some (recnode) => {
                    let mess = ClientMessage::StoreNode(queryconf, r);
                    if (connected){
                      // send result directly
                      let s = get_or_init_client_connection::<RT, T>(&recnode, & rc , & mut route, & rp, false, None);
                      s.send(mess);
                    } else {
                       send_nonconnected::<RT, T> (&recnode, & rc , & mut route, & rp, mess);
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
               if (connected) {
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
           if connected{
             get_or_init_client_connection::<RT, T>(n, & rc , & mut route, & rp, true, None);
           } else {
             send_nonconnected_ping::<RT, T>(n, & rc , & mut route, & rp);
           };
         }
       },
       Ok(PeerMgmtMessage::ShutDown)  => {
         info!("Shudown receive");
         break;
       },
       Ok(PeerMgmtMessage::StoreNode(qconf, result)) => {
         match qconf.0.clone().get_rec_node() {
           Some (rec)  => {
             if rec.get_key() != rc.me.get_key() { 
               let mess = ClientMessage::StoreNode(qconf, result);
               if(connected){
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
             error!("Cannot proxied received store node to originator for conf {:?} ",qconf.0);
           },
         }
       },
       Ok(PeerMgmtMessage::StoreKV(qconf, result)) => {
         // (query sync over clients(the query contains nothing)) // TODO factorize a fun
         // for proxied msg
         match qconf.0.clone().get_rec_node() {
           Some (rec) => {
             if rec.get_key() != rc.me.get_key() {
               let mess = ClientMessage::StoreKV(qconf, result);
               if (connected) {
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
             error!("Cannot proxied received store value to originator for conf {:?} ",qconf.0);
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
fn send_nonconnected<RT : RunningTypes, T : Route<RT::P,RT::V>>
 (p : & Arc<RT::P> , 
  rc : & ArcRunningContext<RT>, 
  route : & mut T , 
  rp : & RunningProcesses<RT::P,RT::V>,
  mess : ClientMessage<RT::P,RT::V>
 )
 ->  bool {
 match route.get_node(&p.get_key()) {
  Some(&(_,PeerPriority::Blocked,_)) => {
      // if existing consider ping true : normaly if offline or block , no channel
      info!("#####blocked : client do not send");
      false
  },
  _ => {
  let rpsp = rp.clone();
  let rcsp = rc.clone();
  let psp = p.clone();
  thread::spawn (move || {client::start::<RT>(psp, None, None, rcsp, rpsp, false, None, Some(mess),false)});
  true
  },
 }
}

#[inline]
fn send_nonconnected_ping<RT : RunningTypes, T : Route<RT::P,RT::V>>
 (p : & Arc<RT::P> , 
  rc : & ArcRunningContext<RT>, 
  route : & mut T , 
  rp : & RunningProcesses<RT::P,RT::V>,
 )
 ->  bool {
 match route.get_node(&p.get_key()) {
  Some(&(_,PeerPriority::Blocked,_)) => {
      // if existing consider ping true : normaly if offline or block , no channel
      info!("#####blocked : client do not send");

      false // TODO avoid this clone
  },
  _ => {
  let rpsp = rp.clone();
  let rcsp = rc.clone();
  let psp = p.clone();
  thread::spawn (move || {client::start::<RT>(psp, None, None, rcsp, rpsp, true, None, None,false)});
  true
  },
 }
}


#[inline]
fn get_or_init_client_connection<RT : RunningTypes, T : Route<RT::P,RT::V>>
 (p : & Arc<RT::P> , 
  rc : & ArcRunningContext<RT>, 
  route : & mut T , 
  rp : & RunningProcesses<RT::P,RT::V>, 
  ping : bool, 
  pingres : Option<OneResult<bool>>) 
 ->  Sender<ClientMessage<RT::P,RT::V>> {
let (upd, s) = match route.get_node(&p.get_key()) {
  Some(&(_,PeerPriority::Blocked,_))| Some(&(_,PeerPriority::Offline,_)) => {
    // retry connecting & accept for block
    let (tcl,rcl) = channel();
    debug!("##pingtrue");
    (Some(rcl), tcl)
  },
  Some(&(_,_, Some(ref s))) => {
     debug!("#####get or init find with chanel");
     (None, s.clone()) // TODO avoid this clone
  },
  None | Some(&(_, _, None)) => {
  let (tcl,rcl) = channel();
  (Some(rcl), tcl)
  },
 };
 match upd {
     Some(rcl) => {

     debug!("#####putting channel");
     route.add_node(p.clone(), Some(s.clone()));


  let tcl3 = s.clone();

  // dup value for spawn TODO switch to arc!!!
  let rpsp = rp.clone();
  //when tcl store later
  let rcsp = rc.clone();
  let psp = p.clone();
  debug!("#####initiating client process from {:?} to {:?} with ping {:?}",rc.me.get_key(), p.get_key(),ping);
  thread::spawn (move || {client::start::<RT>(psp,Some(tcl3), Some(rcl), rcsp, rpsp, ping, pingres, None, true)});
     },
     None => {
       // we found an existing channel with seemlessly open connection
      pingres.map(|ares|{
              utils::ret_one_result(&ares, true)
      });

     },
 };
 s.clone()
}

#[inline]
fn update_lastsent_conf<P : Peer> ( queryconf : &QueryConfMsg<P>,  peers : &Vec<Arc<P>>, nbquery : u8) -> QueryConfMsg<P> {
                let mut newqueryconf = queryconf.clone();
                newqueryconf.2 = match newqueryconf.2 {
                  Some(LastSent::LastSentPeer(maxnb,mut lpeers)) => {
                    let totalnb =  (peers.len() + lpeers.len());
                    if totalnb > maxnb {
                    for _ in 0..(totalnb - maxnb) {
                      lpeers.pop_front();
                    };}else{};
                    for p in peers.iter(){
                      lpeers.push_back(p.get_key());
                    };
                    Some(LastSent::LastSentPeer(maxnb, lpeers))
                    
                  },
                  Some(LastSent::LastSentHop(mut hop,mut lpeers)) => {
                    if(hop > 0){
                      hop -= 1;
                    }
                    else{
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
