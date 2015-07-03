#![feature(int_uint)]
#![feature(core)]
#![feature(io)]
#![feature(collections)]
#![feature(std_misc)]
#![feature(file_path)]
#![feature(fs_walk)]
#![feature(path_ext)]
#![feature(net)]
#![feature(tcp)]
#![feature(convert)]
#![feature(alloc)]
#![feature(thread_sleep)]


// :nn <F2> :w<cr>:!cargo run --verbose -- -C node2.conf
extern crate rustc_serialize;
#[macro_use] extern crate mydht;
extern crate time;
#[macro_use] extern crate log;
extern crate env_logger;
extern crate rand;
extern crate num;

use rustc_serialize::json;
use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};

use std::marker::PhantomData;
use std::thread;
use std::fs::{File};
use std::path::Path;
use std::io::Read;
use time::Duration;
use std::env;
use std::net::{Ipv4Addr};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::channel;
use self::rand::{thread_rng, Rng};
use num::traits::{ToPrimitive, Bounded};
use mydht::{StoragePriority};
use mydht::kvstoreif::KeyVal;
use mydht::{DHT,RunningContext,RunningProcesses,ArcRunningContext};
use mydht::{QueryConf,QueryPriority,QueryMode,QueryChunk};
use mydht::{CachePolicy};
use mydht::queryif;
use mydht::Json;
use mydht::Bincode;
use mydht::Tcp;
use mydht::Udp;
use mydht::{Attachment,SettableAttachment};
use mydht::{PeerPriority};
use mydht::peerif::PeerMgmtRules;
use mydht::dhtimpl::{Node,SimpleCache,SimpleCacheQuery,Inefficientmap};

use mydht::queryif::{QueryRules};
use mydht::transportif::Transport;
use mydht::msgencif::{MsgEnc};
use mydht::utils::ArcKV;
use mydht::utils::SocketAddrExt;
use mydht::utils;

fn main() {
    env_logger::init().unwrap();
    let showHelp = || println!("-C <file> to select config file, -B <file> to choose node bootstrap file");
    debug!("hello world!");
    let mut confPath = "node.conf".to_string();
    let mut bootPath = "bootstrap.conf".to_string();
    enum ParamState {Normal, Conf, Boot}
    let mut state = ParamState::Normal;
    for arg in env::args() {
        match &arg[..] {
            "-h" => showHelp(),
            "--help" => showHelp(),
            "-C" => state = ParamState::Conf,
            "-B" => state = ParamState::Boot,
            a  => match state {
                ParamState::Normal => debug!("{:?}", arg),
                ParamState::Conf => { confPath = a.to_string(); state = ParamState::Normal },
                ParamState::Boot => { bootPath = a.to_string(); state = ParamState::Normal },
            }
        }
    }

    info!("using conf file : {:?}" , confPath);
    info!("using boot file : {:?}" , bootPath);

    let initNode = Node {nodeid: "dummyID1".to_string(), address : SocketAddrExt(utils::sa4(Ipv4Addr::new(127,0,0,1), 0))}; // default port to 0 (a port will be assigned by system)

//    let confFile = File::open(&Path::new(confPath));
//    let mut confFile = File::create(&Path::new(confPath));
//    confFile.write(json::encode(&initNode).into_bytes().as_slice());

    // no mgmt of io error (panic cf unwrap) when reading conf
    let mut jsonCont = String::new();
    File::open(&Path::new(&confPath[..])).unwrap().read_to_string(&mut jsonCont).unwrap(); 
    let mynode : Node = json::decode(&jsonCont[..]).unwrap();


let tcp_transport : Tcp = Tcp {
  streamtimeout : Duration::seconds(5),
  connecttimeout :  Duration::seconds(5),
};


    info!("my node is : {:?}" , mynode);
    /*loop{
      let newnode = Box::new(mynode.clone());
    printmynode(newnode);
    };*/
    // getting bootstrap peers
    let mut jsonBCont = String::new();
    File::open(&Path::new(&bootPath[..])).unwrap().read_to_string(&mut jsonBCont).unwrap(); 
    let tmpbootNodes : Vec<Node>  = json::decode(&jsonBCont[..]).unwrap();
    let bootNodes : Vec<Arc<Node>> = tmpbootNodes.into_iter().map(|p|Arc::new(p)).collect();
    let rc : ArcRunningContext<Node, DummyKeyVal, DummyRules, DummyQueryRules, Json, Tcp> = Arc::new(
    RunningContext {
      me : Arc::new(mynode),
      peerrules : DummyRules,
      queryrules : DummyQueryRules{idcnt:Mutex::new(0)},
      msgenc : Json,
      transport : tcp_transport,
      keyval : PhantomData,
    }
    );
 
    let mut serv = DHT::<_,DummyKeyVal, _, _, _, _>::boot_server(rc, Inefficientmap::new(), SimpleCacheQuery::new(), move || Some(SimpleCache::new(None)), Vec::new(), bootNodes);
    serv.block();
    info!("exiting...");
 
}
pub fn printmynode<V : KeyVal + 'static>(mynode : Box<V>){
  
  println!("my node is : {:?}" , mynode.get_key());
}


pub type DummyKeyVal = ArcKV<DummyKeyValIn>;
// TODO put keyval in an Arc to avoid copies or change all msg if to use arc
#[derive(RustcDecodable,RustcEncodable,Debug,PartialEq,Eq,Clone)]
pub struct DummyKeyValIn {
    pub id : String,
//  pub value : String, // value is not needed for testing TODO latter test for value conflict
}
impl KeyVal for DummyKeyValIn {
    type Key = String;
    fn get_key(&self) -> String {
        self.id.clone()
    }
    noattachment!();
}

impl SettableAttachment for DummyKeyValIn {
}



//#[test] // TODO hardcoded ip and port, find a way to define it
fn proxyPeerDiscovery () {peerConnectScenario((QueryMode::Proxy, QueryChunk::None, Some((2,true))), 40534, 5, 2)}

//#[test]
fn aproxyPeerDiscovery () {peerConnectScenario((QueryMode::AProxy, QueryChunk::None, Some((4,false))), 40534, 5, 2)}

//#[test]
fn amixProxyPeerDiscovery () {peerConnectScenario((QueryMode::AMix(9), QueryChunk::None, None), 40534, 5, 2)}

//#[test]
fn asynchPeerDiscovery () {peerConnectScenario((QueryMode::Asynch, QueryChunk::None, Some((3,true))), 41534, 5, 2)}


fn peerConnectScenario (queryconf : QueryConf, startPort: u16, nbpeer : u16, knownratio : u16){

    let mut r : Vec<u16> = (startPort .. startPort+nbpeer).collect();
    let nodes : Vec<Node> = r.iter().map(
      |i| Node {nodeid: "dummyID".to_string() + (&i.to_string()[..]), address : SocketAddrExt(utils::sa4(Ipv4Addr::new(127,0,0,1), *i))}
    ).collect();
    let mut rng = thread_rng();
    let procs : Vec<DHT<Node,DummyKeyVal,DummyRules,DummyQueryRules,Json,Tcp>> = nodes.iter().map(|n|{
        info!("node : {:?}", n);
let tcp_transport : Tcp = Tcp {
  streamtimeout : Duration::seconds(5),
  connecttimeout :  Duration::seconds(5),
};


//        rng.shuffle(noderng);
        let bpeers = nodes.clone().into_iter().filter(|i| (*i != *n && rng.gen_range(0,knownratio) == 0) ).map(|p|Arc::new(p)).collect();
//        let mut noderng : &mut [Node] = bpeers.as_slice();
        let nsp = Arc::new(n.clone());
        DHT::boot_server(Arc:: new(
        RunningContext {
          me : nsp,
          peerrules : DummyRules,
          queryrules : DummyQueryRules{idcnt:Mutex::new(0)},
          msgenc : Json,
          transport : tcp_transport,
          keyval : PhantomData,
        }
        ), Inefficientmap::new(), SimpleCacheQuery::new(), move || Some(SimpleCache::new(None)), Vec::new(), bpeers)
    }).collect();

    thread::sleep_ms(2000);
    // find all node from the first node first node
    let ref fprocs = procs[0];

    let mut itern = nodes.iter();
    itern.next();
    for n in itern{
        let fpeer = fprocs.find_peer(n.nodeid.clone(), queryconf.clone(), 1); // TODO put in future first then match result (simultaneous search) + queryconf.clone inacceptable : use Arc queryconf!!!
        let matched = match fpeer {
            Some(v) => *v == *n,
            _ => false,
        };
        assert!(matched, "Peer not found {:?}", n);
    }

    for p in procs.iter(){p.shutdown()}
//    procs.iter().map(|p|{p.shutdown()}); // no cause lazy

//    for mut p in procs.iter(){p.block()}
//    assert_eq!(1i,1i);
}


#[derive(Debug,Clone)]
struct DummyRules;
unsafe impl Send for DummyRules {
}

impl PeerMgmtRules<Node, DummyKeyVal> for DummyRules{
  fn challenge (&self, n : &Node) -> String{
    "dummychallenge not random at all".to_string()
  }
  fn signmsg   (&self, n : &Node, chal : &String) -> String{
    "dummy signature".to_string()
  }
  fn checkmsg  (&self, n : &Node, chal : &String, sign : &String) -> bool{ true}
  // typically accept return either normal (no priority managed) or a int priority
  fn accept<R : PeerMgmtRules<Node,DummyKeyVal>, Q : QueryRules, E : MsgEnc, T : Transport> 
  (&self, n : &Arc<Node>, _ : &RunningProcesses<Node,DummyKeyVal>, _ : &ArcRunningContext<Node,DummyKeyVal,R,Q,E,T>) 
  -> Option<PeerPriority>
  {Some (PeerPriority::Priority(1))}
  #[inline]
  fn for_accept_ping<R : PeerMgmtRules<Node,DummyKeyVal>, Q : QueryRules, E : MsgEnc, T : Transport> 
  (&self, n : &Arc<Node>, _ : &RunningProcesses<Node,DummyKeyVal>, _ : &ArcRunningContext<Node,DummyKeyVal,R,Q,E,T>) 
  {}
}

#[derive(Debug,Clone)]
struct DummyRules2;
unsafe impl Send for DummyRules2 {
}

impl PeerMgmtRules<Node, DummyKeyVal> for DummyRules2{
  fn challenge (&self, n : &Node) -> String{
    "dummychallenge not random at all".to_string()
  }
  fn signmsg   (&self, n : &Node, chal : &String) -> String{
    "dummy signature".to_string()
  }
  fn checkmsg  (&self, n : &Node, chal : &String, sign : &String) -> bool{ true}
  // typically accept return either normal (no priority managed) or a int priority
  fn accept<R : PeerMgmtRules<Node,DummyKeyVal>, Q : QueryRules, E : MsgEnc, T : Transport> 
  (&self, n : &Arc<Node>, _ : &RunningProcesses<Node,DummyKeyVal>, _ : &ArcRunningContext<Node,DummyKeyVal,R,Q,E,T>) 
  -> Option<PeerPriority>
  {Some (PeerPriority::Priority(2))}
  #[inline]
  fn for_accept_ping<R : PeerMgmtRules<Node,DummyKeyVal>, Q : QueryRules, E : MsgEnc, T : Transport> 
  (&self, n : &Arc<Node>, _ : &RunningProcesses<Node,DummyKeyVal>, _ : &ArcRunningContext<Node,DummyKeyVal,R,Q,E,T>) 
  {}
}


struct DummyQueryRules{
    idcnt : Mutex<usize>,
}

unsafe impl Send for DummyQueryRules {
}

impl queryif::QueryRules for DummyQueryRules{

  #[inline]
  fn nbhop_dec (&self) -> u8{
    1 // most of the time (some time we may random to 0 so we do not know if first hop
  }

  // here both a static counter and a rand one just for show
  fn newid (&self) -> String{
      // (eg database connection)
      let mut rng = thread_rng();
      let s = rng.gen_range(0,65555);
      let mut i = self.idcnt.lock().unwrap();
      *i += 1;
      let r = "query ".to_string() + &s.to_string()[..] + "_" + &(*i).to_string()[..];
      println!("############### {}" , r);
      r
  }
  fn nbhop (&self, prio : QueryPriority) -> u8{
      match prio {
          1 => 6, // do not change (used in tests)
          2 => 3, // do not change (used in tests)
          3 => 2,
          4 => 1,
          _ => 0, // do not change (used in tests)
      }
  }
  fn lifetime (&self, prio : QueryPriority) -> Duration{
      match prio {
          1 => Duration::seconds(6),
          _ => Duration::seconds(6),
      }
      
  }

  fn nbquery (&self, prio : QueryPriority) -> u8{
     match prio {
          1 => 2,
          2 => 1,
          3 => 3,
          _ => 6,
      }
 
  }
  fn asynch_clean(&self) -> Option<Duration>{
      Some(Duration::seconds(5)) // fast one for testing purpose
  }

  // TODO add info to get propagation or not of stored value (or new function)
  fn do_store (&self, islocal : bool, qprio : QueryPriority, sprio : StoragePriority, hopnb : Option<usize>) -> (bool,Option<CachePolicy>) {
      let res = match sprio {
          StoragePriority::Local =>
              if islocal { (true,Some(Duration::minutes(10))) } else { (false,None) }, // for non local we should have done something depending on queryprio
          StoragePriority::PropagateL(estnbhop) => 
              if islocal { (true,Some(Duration::minutes(10)))  } else {
                  match hopnb {
                      None => (false, None),
                      Some(esthop) => { 
                          if esthop > estnbhop {
                          (false,None)
                          } else {
                              if esthop > 2 {
                                  (false, None)
                              } else {
                                  (false,Some(Duration::minutes(5)))
                              }
                          }},
                  }},
          StoragePriority::PropagateH(estnbhop) => 
              if islocal { (true,Some(Duration::minutes(10)))  } else {
                  match hopnb {
                      None => (false, None),
                      Some(esthop) => {
                          if esthop > estnbhop {
                          (false,None)
                          } else {
                              if esthop > 2 {
                                  (false,Some(Duration::minutes(5)))
                              } else {
                                  (true,Some(Duration::minutes(10)))
                              }
                          }},
                  }},
          StoragePriority::Trensiant => (false,Some(Duration::minutes(5))),
          StoragePriority::All =>  (true,Some(Duration::minutes(10))),
          StoragePriority::NoStore => (false,None),
      };
      (res.0, res.1.map(|d| CachePolicy(time::get_time() + d)))
  } // wether you need to store the keyval or not
}

static alltestmode : [QueryMode; 1] = [
//                   QueryMode::Proxy,
//                   QueryMode::Proxy,
                     QueryMode::Asynch,
       //            QueryMode::AProxy,
    //               QueryMode::AMix(1),
    //               QueryMode::AMix(2),
     //              QueryMode::AMix(3)
                   ];

//#[test]
fn testPeer2hopget (){
    let n = 4;
    let map : &[&[usize]] = &[&[2],&[3],&[],&[3]];
    finddistantpeer(45440,n,QueryMode::Proxy,DummyRules,1,map,true); 
    finddistantpeer(45450,n,QueryMode::Asynch,DummyRules,1,map,true); 
    finddistantpeer(45460,n,QueryMode::AProxy,DummyRules,1,map,true); 
    finddistantpeer(45470,n,QueryMode::AMix(1),DummyRules,1,map,true); 
    finddistantpeer(45480,n,QueryMode::AMix(2),DummyRules,1,map,true); 
    finddistantpeer(45490,n,QueryMode::AMix(3),DummyRules,1,map,true); 
}

//#[test]
fn testPeermultipeersnoresult (){
    let n = 6;
    let map : &[&[usize]] = &[&[2,3,4],&[3,5],&[1],&[4],&[1],&[]];
    finddistantpeer(45540,n,QueryMode::Proxy,DummyRules,1,map,false); 
    finddistantpeer(45550,n,QueryMode::Asynch,DummyRules,1,map,false); 
    finddistantpeer(45560,n,QueryMode::AProxy,DummyRules,1,map,false); 
    finddistantpeer(45570,n,QueryMode::AMix(1),DummyRules,1,map,false); 
    finddistantpeer(45580,n,QueryMode::AMix(2),DummyRules,1,map,false); 
    finddistantpeer(45590,n,QueryMode::AMix(3),DummyRules,1,map,false); 
}


//#[test]
fn testPeer4hopget (){
    let n = 6;
    let map : &[&[usize]] = &[&[2],&[3],&[4],&[5],&[6],&[]];
    finddistantpeer(46440,n,QueryMode::Proxy,DummyRules,1,map,true); 
    finddistantpeer(46450,n,QueryMode::Asynch,DummyRules,1,map,true); 
    finddistantpeer(46460,n,QueryMode::AProxy,DummyRules,1,map,true); 
    finddistantpeer(46470,n,QueryMode::AMix(2),DummyRules,1,map,true); 
    finddistantpeer(46480,n,QueryMode::AMix(4),DummyRules,1,map,true); 
    finddistantpeer(46490,n,QueryMode::AMix(5),DummyRules,1,map,true);
    // prio 2 max nb hop is 3 
    finddistantpeer(46500,n,QueryMode::Proxy,DummyRules,2,map,false);
    finddistantpeer(46510,n,QueryMode::Asynch,DummyRules,2,map,false); 
    finddistantpeer(46520,n,QueryMode::AProxy,DummyRules,2,map,false); 
    finddistantpeer(46530,n,QueryMode::AMix(1),DummyRules,2,map,false); 
    finddistantpeer(46540,n,QueryMode::AMix(3),DummyRules,2,map,false); 
}

//#[test]
fn testloopget (){ // TODO this only test loop over our node TODO circuit loop test
    let n = 4;
    // closest used are two first nodes (first being ourselves
    let map : &[&[usize]] = &[&[1,2],&[2,3],&[3,4],&[4]];
    //let map : &[&[usize]] = &[&[1,2],&[2,1,3],&[3,4],&[4]];
    finddistantpeer(55440,n,QueryMode::Proxy,DummyRules,1,map,true); 
    finddistantpeer(55450,n,QueryMode::Asynch,DummyRules,1,map,true); 
    finddistantpeer(55460,n,QueryMode::AProxy,DummyRules,1,map,true); 
    finddistantpeer(55470,n,QueryMode::AMix(1),DummyRules,1,map,true); 
    finddistantpeer(55480,n,QueryMode::AMix(2),DummyRules,1,map,true); 
    finddistantpeer(55490,n,QueryMode::AMix(3),DummyRules,1,map,true);
}


fn finddistantpeer<R : PeerMgmtRules<Node, DummyKeyVal> + Clone>  (startport : u16,nbpeer : usize, qm : QueryMode, rules : R, prio : QueryPriority, map : &[&[usize]], find : bool) {
    let peers = initpeers(startport,nbpeer, map, rules);
    let queryconf = (qm.clone(), QueryChunk::None, Some((3,true))); // note that we only unloop to 3 hop 
    let dest = peers.get(nbpeer -1).unwrap().0.clone();
    let fpeer = peers.get(0).unwrap().1.find_peer(dest.nodeid.clone(), queryconf.clone(), prio);
    let matched = match fpeer {
       Some(ref v) => **v == dest,
       _ => false,
    };
    if(find){
    assert!(matched, "Peer not found {:?} , {:?}", fpeer, qm);
    }else{
    assert!(!matched, "Peer found {:?} , {:?}", fpeer, qm);
    }
}


fn initpeers<R : PeerMgmtRules<Node, DummyKeyVal> + Clone> (startPort : u16, nbpeer : usize, map : &[&[usize]], rules : R) -> Vec<(Node, DHT<Node,DummyKeyVal,R,DummyQueryRules,Json,Tcp>)>{


    let mut r : Vec<usize> = (0..nbpeer).collect();
    let nodes : Vec<Node> = r.iter().map(
      |j| {
          Node {nodeid: "NodeID".to_string() + &(*j + 1).to_string()[..], address : SocketAddrExt(utils::sa4(Ipv4Addr::new(127,0,0,1), startPort + (*j).to_u16().unwrap()))}
          }
    ).collect();
    let nodes2 = nodes.clone(); // not efficient but for test
    let mut i = 0;// TODO redesign with zip of map and nodes iter
    let result :  Vec<(Node, DHT<Node,DummyKeyVal,R,DummyQueryRules,Json,Tcp>)> = nodes.iter().map(|n|{
        info!("node : {:?}", n);
        println!("{:?}",map[i]);
        let bpeers = map[i].iter().map(|j| nodes2.get(*j-1).unwrap().clone()).map(|p|Arc::new(p)).collect();
        i += 1;
        let nsp = Arc::new(n.clone());
let tcp_transport : Tcp = Tcp {
  streamtimeout : Duration::seconds(5),
  connecttimeout :  Duration::seconds(5),
};
        // add node without ping
        //(n.clone(), DHT::boot_server(Arc:: new((nsp,rules.clone(), DummyQueryRules{idcnt:Mutex::new(0)},Json,tcp_transport)), Inefficientmap::new(), SimpleCacheQuery::new(), SimpleCache::new(), bpeers, Vec::new()))
        (n.clone(), DHT::boot_server(Arc:: new(
        RunningContext {
          me : nsp,
          peerrules : rules.clone(),
          queryrules : DummyQueryRules{idcnt:Mutex::new(0)},
          msgenc : Json,
          transport : tcp_transport,
          keyval : PhantomData,
        }
        ), Inefficientmap::new(), SimpleCacheQuery::new(), move || Some(SimpleCache::new(None)), bpeers, Vec::new()))
 }).collect();

    // all has started
    for n in result.iter(){
      thread::sleep_ms(100); // local get easily stuck
      n.1.refresh_closest_peers(1000); // Warn hard coded value.
    };
    // ping established
    //timer.sleep(Duration::seconds(1));
    result
}





fn initpeers_udp<R : PeerMgmtRules<Node,DummyKeyVal> + Clone> (startPort : u16, nbpeer : usize, map : &[&[usize]], rules : R) -> Vec<(Node, DHT<Node,DummyKeyVal,R,DummyQueryRules,Bincode,Udp>)>{
//fn initpeers_udp<R : PeerMgmtRules<Node> + Clone> (startPort : u16, nbpeer : usize, map : &[&[usize]], rules : R) -> Vec<(Node, DHT<Node,DummyKeyVal,R,DummyQueryRules,Json,Udp>)>{
//fn initpeers<R : PeerMgmtRules<Node> + Clone> (startPort : u16, nbpeer : usize, map : &[&[usize]], rules : R) -> Vec<(Node, DHT<Node,DummyKeyVal,R,DummyQueryRules,Json,Tcp>)>{


    let mut r : Vec<usize> = (0..nbpeer).collect();
    let nodes : Vec<Node> = r.iter().map(
      |j| {
          Node {nodeid: "NodeID".to_string() + &(*j + 1).to_string()[..], address : SocketAddrExt(utils::sa4(Ipv4Addr::new(127,0,0,1), startPort + (*j).to_u16().unwrap()))}
          }
    ).collect();
    let nodes2 = nodes.clone(); // not efficient but for test
    let mut i = 0;// TODO redesign with zip of map and nodes iter
    //let result :  Vec<(Node, DHT<Node,DummyKeyVal,R,DummyQueryRules,Json,Tcp>)> = nodes.iter().map(|n|{
//    let result :  Vec<(Node, DHT<Node,DummyKeyVal,R,DummyQueryRules,Json,Udp>)> = nodes.iter().map(|n|{
    let result :  Vec<(Node, DHT<Node,DummyKeyVal,R,DummyQueryRules,Bincode,Udp>)> = nodes.iter().map(|n|{
        info!("node : {:?}", n);
        println!("{:?}",map[i]);
        let bpeers = map[i].iter().map(|j| nodes2.get(*j-1).unwrap().clone()).map(|p|Arc::new(p)).collect();
        i += 1;
        let nsp = Arc::new(n.clone());
let tcp_transport : Tcp = Tcp {
  streamtimeout : Duration::seconds(5),
  connecttimeout :  Duration::seconds(5),
};


        // add node without ping
        //(n.clone(), DHT::boot_server(Arc:: new((nsp,rules.clone(), DummyQueryRules{idcnt:Mutex::new(0)},Json,tcp_transport)), Inefficientmap::new(), SimpleCacheQuery::new(), SimpleCache::new(), bpeers, Vec::new()))
        let tran = Udp::new(2048); // here udp with a json encoding with last sed over a few hop : we need a big buffer
//        (n.clone(), DHT::boot_server(Arc:: new((nsp,rules.clone(), DummyQueryRules{idcnt:Mutex::new(0)},Json,tran)), Inefficientmap::new(), SimpleCacheQuery::new(), SimpleCache::new(), bpeers, Vec::new()))
        (n.clone(), DHT::boot_server(Arc:: new(
        RunningContext {
          me : nsp,
          peerrules : rules.clone(),
          queryrules : DummyQueryRules{idcnt:Mutex::new(0)},
          msgenc : Bincode,
          transport : tran,
          keyval : PhantomData,
        }
        ), Inefficientmap::new(), SimpleCacheQuery::new(), move || Some(SimpleCache::new(None)), bpeers, Vec::new()))
 }).collect();

    // all has started
    for n in result.iter(){
      thread::sleep_ms(100); // local get easily stuck
      n.1.refresh_closest_peers(1000); // Warn hard coded value.
    };
    // ping established
    //timer.sleep(Duration::seconds(1));
    result
}

//#[test]
fn testPeer2hopfindval_udp (){
    let nbpeer = 4;
    let val = ArcKV::new(DummyKeyValIn{id:"value to find ky".to_string()});
    let map : &[&[usize]] = &[&[],&[1,3],&[],&[3]];

    let mut startport = 73440;
    let prio = 1;
    let peers = initpeers_udp(startport,nbpeer, map, DummyRules);
    let ref dest = peers.get(nbpeer -1).unwrap().1;
    for conf in alltestmode.iter(){
    let queryconf = (conf.clone(), QueryChunk::None, Some((7,false)));
    assert!(dest.store_val(val.clone(), queryconf.clone(), prio, StoragePriority::Local));
    let res = peers.get(0).unwrap().1.find_val(val.get_key().clone(), queryconf.clone(), prio,StoragePriority::NoStore, 1).pop().unwrap_or(None);
    assert_eq!(res, Some(val.clone()));
    }
}



//#[test]
fn testPeer2hopfindval (){
    let nbpeer = 4;
    let val = ArcKV::new(DummyKeyValIn{id:"value to find ky".to_string()});
    let map : &[&[usize]] = &[&[],&[1,3],&[],&[3]];

    let mut startport = 73440;
    let prio = 1;
    let peers = initpeers(startport,nbpeer, map, DummyRules);
    let ref dest = peers.get(nbpeer -1).unwrap().1;
    for conf in alltestmode.iter(){
    let queryconf = (conf.clone(), QueryChunk::None, Some((7,false)));
    assert!(dest.store_val(val.clone(), queryconf.clone(), prio, StoragePriority::Local));
    let res = peers.get(0).unwrap().1.find_val(val.get_key().clone(), queryconf.clone(), prio,StoragePriority::NoStore, 1).pop().unwrap_or(None);
    assert_eq!(res, Some(val.clone()));
    }
}

//#[test]
fn testPeer2hopstoreval (){
    let nbpeer = 4;
    let val = ArcKV::new(DummyKeyValIn{id:"value to find ky".to_string()});
    let map : &[&[usize]] = &[&[],&[1,3],&[],&[3]];
    let mut startport = 73440;
    let prio = 1;
    let peers = initpeers(startport,nbpeer, map, DummyRules);
    let ref dest = peers.get(nbpeer -1).unwrap().1;
    let conf = alltestmode.get(0).unwrap();
    let queryconf = (conf.clone(), QueryChunk::None, Some((4,true)));
    assert!(dest.store_val(val.clone(), queryconf.clone(), prio, StoragePriority::Local));
    let res = peers.get(0).unwrap().1.find_val(val.get_key().clone(), queryconf.clone(), prio,StoragePriority::Local, 1).pop().unwrap_or(None);
    assert_eq!(res, Some(val.clone()));
    // prio 10 is nohop (we see if localy store)
    let res = peers.get(0).unwrap().1.find_val(val.get_key().clone(), queryconf.clone(), 10,StoragePriority::NoStore, 1).pop().unwrap_or(None);
    assert_eq!(res, Some(val.clone()));
    let res = peers.get(1).unwrap().1.find_val(val.get_key().clone(), queryconf.clone(), 10,StoragePriority::NoStore, 1).pop().unwrap_or(None);
    assert!(!(res == Some(val.clone())));
}

