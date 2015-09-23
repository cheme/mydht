

// :nn <F2> :w<cr>:!cargo run --verbose -- -C node2.conf
extern crate rustc_serialize;
#[macro_use] extern crate mydht;
extern crate time;
#[macro_use] extern crate log;
extern crate env_logger;
extern crate rand;
extern crate num;

use rustc_serialize::json;
use rustc_serialize::{Encoder,Encodable,Decoder};

use std::marker::PhantomData;
use std::fs::{File};
use std::path::Path;
use std::io::Read;
use time::Duration;
use std::net::{SocketAddr};
use std::env;
use std::sync::Arc;
use std::sync::Mutex;
//use self::rand::{thread_rng};
use mydht::{StoragePriority};
use mydht::dhtif::KeyVal;
use mydht::{DHT,RunningContext,RunningProcesses,ArcRunningContext,RunningTypes};
use mydht::{QueryPriority,QueryID};
use mydht::{CachePolicy};
use mydht::dhtif;
use mydht::Json;
use mydht::Tcp;
use mydht::{Attachment,SettableAttachment};
use mydht::{PeerPriority};
use mydht::dhtif::{Peer,PeerMgmtMeths};
use mydht::dhtimpl::{Node,SimpleCache,SimpleCacheQuery,Inefficientmap};

use mydht::dhtif::{DHTRules};
use mydht::transportif::Transport;
use mydht::msgencif::{MsgEnc};
use mydht::utils::ArcKV;

fn main() {
    env_logger::init().unwrap();
    let show_help = || println!("-C <file> to select config file, -B <file> to choose node bootstrap file");
    debug!("hello world!");
    let mut conf_path = "node.conf".to_string();
    let mut boot_path = "bootstrap.conf".to_string();
    enum ParamState {Normal, Conf, Boot}
    let mut state = ParamState::Normal;
    for arg in env::args() {
        match &arg[..] {
            "-h" => show_help(),
            "--help" => show_help(),
            "-C" => state = ParamState::Conf,
            "-B" => state = ParamState::Boot,
            a  => match state {
                ParamState::Normal => debug!("{:?}", arg),
                ParamState::Conf => { conf_path = a.to_string(); state = ParamState::Normal },
                ParamState::Boot => { boot_path = a.to_string(); state = ParamState::Normal },
            }
        }
    }

    info!("using conf file : {:?}" , conf_path);

//    let confFile = File::open(&Path::new(conf_path));
//    let mut confFile = File::create(&Path::new(conf_path));
//    confFile.write(json::encode(&initNode).into_bytes().as_slice());

    // no mgmt of io error (panic cf unwrap) when reading conf
    let mut json_cont = String::new();
    File::open(&Path::new(&conf_path[..])).unwrap().read_to_string(&mut json_cont).unwrap(); 
    let mynode : Node = json::decode(&json_cont[..]).unwrap();
    let myadd = mynode.to_address();


    let tcp_transport = Tcp::new(
      &myadd,
      Duration::seconds(5), // timeout
    //  Duration::seconds(5), // conn timeout
      true,//mult
    ).unwrap();


    info!("my node is : {:?}" , mynode);
    /*loop{
      let newnode = Box::new(mynode.clone());
    printmynode(newnode);
    };*/
    // getting bootstrap peers
    let mut json_bcont = String::new();
    File::open(&Path::new(&boot_path[..])).unwrap().read_to_string(&mut json_bcont).unwrap(); 
    let tmpboot_nodes : Vec<Node>  = json::decode(&json_bcont[..]).unwrap();
    let boot_nodes : Vec<Arc<Node>> = tmpboot_nodes.into_iter().map(|p|Arc::new(p)).collect();
    let rc : ArcRunningContext<RunningTypesImpl<DummyRules, Tcp, Json>> = Arc::new(
    RunningContext::new(
      Arc::new(mynode),
      DummyRules,
      DummyQueryRules{idcnt:Mutex::new(0)},
      Json,
      tcp_transport
    )
    );
 
    let serv = DHT::<RunningTypesImpl<DummyRules, Tcp, Json>>::boot_server(
      rc, 
      move || Some(Inefficientmap::new()), 
      move || Some(SimpleCacheQuery::new(false)), 
      move || Some(SimpleCache::new(None)), 
      Vec::new(), 
      boot_nodes).unwrap();
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



#[derive(Debug,Clone)]
struct DummyRules;
unsafe impl Send for DummyRules {
}

impl PeerMgmtMeths<Node, DummyKeyVal> for DummyRules{
  fn challenge (&self, _ : &Node) -> Vec<u8> {
    "dummychallenge not random at all".as_bytes().to_vec()
  }
  fn signmsg   (&self, _ : &Node, _ : &[u8]) -> Vec<u8> {
    "dummy signature".as_bytes().to_vec()
  }
  fn checkmsg  (&self, _ : &Node, _ : &[u8], _ : &[u8]) -> bool { true}
  // typically accept return either normal (no priority managed) or a int priority
 
  fn accept<M : PeerMgmtMeths<Node, DummyKeyVal>, RT : RunningTypes<P=Node,V=DummyKeyVal,A=<Node as Peer>::Address,M=M>>
  (&self, _ : &Node, _ : &RunningProcesses<RT>, _ : &ArcRunningContext<RT>) 
  -> Option<PeerPriority>
  {Some (PeerPriority::Priority(1))}
  #[inline]
  fn for_accept_ping<M : PeerMgmtMeths<Node, DummyKeyVal>, RT : RunningTypes<P=Node,V=DummyKeyVal,A=<Node as Peer>::Address,M=M>>
  (&self, _ : &Arc<Node>, _ : &RunningProcesses<RT>, _ : &ArcRunningContext<RT>) 
  {}
}

#[derive(Debug,Clone)]
struct DummyRules2;
unsafe impl Send for DummyRules2 {
}

impl PeerMgmtMeths<Node, DummyKeyVal> for DummyRules2 {
  fn challenge (&self, _ : &Node) -> Vec<u8> {
    "dummychallenge not random at all".as_bytes().to_vec()
  }
  fn signmsg   (&self, _ : &Node, _ : &[u8]) -> Vec<u8> {
    "dummy signature".as_bytes().to_vec()
  }
  fn checkmsg  (&self, _ : &Node, _ : &[u8], _ : &[u8]) -> bool {true}


  // typically accept return either normal (no priority managed) or a int priority
  fn accept<M : PeerMgmtMeths<Node, DummyKeyVal>, RT : RunningTypes<P=Node,V=DummyKeyVal,A=<Node as Peer>::Address,M=M>>
  (&self, _ : &Node, _ : &RunningProcesses<RT>, _ : &ArcRunningContext<RT>) 
  -> Option<PeerPriority>
  {Some (PeerPriority::Priority(2))}
  #[inline]
  fn for_accept_ping<M : PeerMgmtMeths<Node, DummyKeyVal>, RT : RunningTypes<P=Node,V=DummyKeyVal,A=<Node as Peer>::Address,M=M>>
  (&self, _ : &Arc<Node>, _ : &RunningProcesses<RT>, _ : &ArcRunningContext<RT>) 
  {}
  

}


struct DummyQueryRules {
    idcnt : Mutex<usize>,
}

unsafe impl Send for DummyQueryRules {
}

impl dhtif::DHTRules for DummyQueryRules {

  #[inline]
  fn nbhop_dec (&self) -> u8{
    1 // most of the time (some time we may random to 0 so we do not know if first hop
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
  fn do_store (&self, islocal : bool, _ : QueryPriority, sprio : StoragePriority, hopnb : Option<usize>) -> (bool,Option<CachePolicy>) {
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
  #[inline]
  fn is_authenticated(&self) -> bool {
    true
  }
  #[inline]
  fn client_mode(&self) -> &dhtif::ClientMode {
    &CMODE
  }

  #[inline]
  fn server_mode_conf(&self) -> (usize, usize, usize, Option<Duration>) {
    (0, 0, 0, None)
  }
  
  #[inline]
  fn is_accept_heavy(&self) -> bool {
    false
  }

  #[inline]
  fn is_routing_heavy(&self) -> (bool,bool,bool) {
    (false, false, false)
  }


}
static CMODE : dhtif::ClientMode = dhtif::ClientMode::ThreadedOne;

struct RunningTypesImpl<M : PeerMgmtMeths<Node, DummyKeyVal>, T : Transport, E : MsgEnc> (PhantomData<M>,PhantomData<T>, PhantomData<E>);

impl<M : PeerMgmtMeths<Node, DummyKeyVal>, T : Transport<Address=SocketAddr>, E : MsgEnc> RunningTypes for RunningTypesImpl<M, T, E> {
  type A = SocketAddr;
  type P = Node;
  type V = DummyKeyVal;
  type M = M;
  type R = DummyQueryRules;
  type E = E;
  type T = T;
}


