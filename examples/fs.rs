
//#![feature(rsawot)]

/*fn main() {
}*/
 
#[macro_use] extern crate log;
#[macro_use] extern crate mydht;
extern crate env_logger;
extern crate rustc_serialize;
extern crate uuid;
extern crate time;
extern crate rand;
extern crate num;

#[cfg(feature="openssl-impl")]
fn main() {
  fs::main()
}

#[cfg(not(feature="openssl-impl"))]
fn main() {
  panic!("Missing openssl dependency for fs example");
}


#[cfg(feature="openssl-impl")]
pub mod fs {

//use std::marker::PhantomData;
use std::env;
//use std::io;
use std::io::Write;
use std::io::Read;
use std::io::BufRead;
use rustc_serialize::json;
use std::sync::Arc;
//use std::sync::Mutex;
//use std::io::Seek;
//use std::io::SeekFrom;
use std::fs::File;
use std::io::stdin;
use std::net::SocketAddr;
use time::Duration;
use mydht::Bincode;
//use mydht::Bencode;
use mydht::Json;
//use std::sync::mpsc::{Sender,Receiver};

use mydht::{Udp,Tcp};
use mydht::dhtif::{Peer,PeerMgmtMeths};
use mydht::{DHT,RunningContext,ArcRunningContext,RunningProcesses,RunningTypes};
use mydht::{PeerPriority,StoragePriority,QueryChunk,QueryMode,QueryConf};
use mydht::dhtif::KeyVal;
use mydht::dhtif::FileKeyVal;
//use mydht::transportif::Transport;
use mydht::dhtimpl::FileKV;
use mydht::CachePolicy;
//use mydht::{TrustedPeer,TrustedVal,Truster};
use mydht::{Truster};
use mydht::dhtimpl::{SimpleCache,SimpleCacheQuery,Inefficientmap,FileStore};
use rustc_serialize::{Encoder,Encodable,Decoder};
use mydht::kvstoreif::{KVStore};
//use mydht::kvstoreif::{KVStoreMgmtMessage};
use mydht::dhtimpl::{RSAPeer,PeerSign};
use mydht::dhtimpl::{WotStore,WotKV,WotK,TrustRules,ClassicWotTrust};
use rustc_serialize::hex::{ToHex,FromHex};
//use mydht::msgencif::{MsgEnc};
use mydht::{Attachment,SettableAttachment};
use std::collections::BTreeSet;
use std::path::{Path,PathBuf};

use mydht::utils::ArcKV;
//use mydht::utils;
use mydht::dhtimpl::DhtRules;
use mydht::dhtimpl::SimpleRules;

//use std::net::Ipv4Addr;
use uuid;
use mydht;
use env_logger;
use time;


#[derive(Debug,RustcDecodable,RustcEncodable)]
/// Config of the storage
pub struct FsConf {
  /// your own peer infos.
  pub me : RSAPeer,
  /// Dht rules
  pub rules : DhtRules,
  /// Transport to use
  pub transport : TransportDef,
  /// Encoding to use
  pub msgenc : MsgEncDef,
  /// Path to store / load from files
  pub storepath : String,
  /// Not working TODO replace with querychunk mode
  pub filetreshold : u64,
}

#[derive(Debug,RustcDecodable,RustcEncodable)]
/// Possible transports
pub enum TransportDef {
  Tcp(i64,i64),
  Udp(usize),
}

#[derive(Debug,RustcDecodable,RustcEncodable)]
/// Possible encodings 
pub enum MsgEncDef {
  Bincode,
  //Bencode,
  Json,
}

// this macros (expand kind) implies it is build for every enum variant (with smaller transport def smaller
// bin), there is a multiplicative factor in size
macro_rules! expand_transport_def(($t:ident, $p:ident, $add:expr, $ftn:expr ) => (
  match $t {
    &TransportDef::Tcp(stimeout,_) => {
      let $p = Tcp::new(
        $add,
        Duration::seconds(stimeout),
        true, // TODO cfig mult or not here mult
        //connecttimeout : Duration::seconds(ctimeout), TODO rem connect timeout from conf
      ).unwrap();
      type TmpTransportMacro = Tcp;
      $ftn;
    },
    &TransportDef::Udp(buffsize) => {
      // TODO cfg with or without spawn
      let $p =  Udp::new($add,buffsize,true).unwrap();
      type TmpTransportMacro = Udp;
      $ftn;
    },
  }
));

macro_rules! expand_msgenc_def(( $t:ident, $p:ident, $ftn:expr ) => (
  match $t {
    &MsgEncDef::Bincode => {
      let $p = Bincode;
      type TmpEncMacro = Bincode;
      $ftn;
    },
      /*      &MsgEncDef::Bencode => {
              let $p = Bencode;
              $ftn;
              },*/
    &MsgEncDef::Json => {
      let $p = Json;
      type TmpEncMacro = Json;
      $ftn;
    },
  }
));

pub fn main() {
  env_logger::init().unwrap();
  let show_help = || println!("-C <file> to select config file, -B <file> to choose node bootstrap file");
  let mut sconf_path = "fsconf.json".to_string();
  let mut boot_path = "fsbootstrap.json".to_string();
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
          ParamState::Conf => { sconf_path = a.to_string(); state = ParamState::Normal },
          ParamState::Boot => { boot_path = a.to_string(); state = ParamState::Normal },
        }
    }
  }

  info!("using conf file : {:?}" , sconf_path);
  info!("using boot file : {:?}" , boot_path);


  // no mgmt of io error (panic cf unwrap) when reading conf
  let conf_path = &PathBuf::from(&sconf_path.clone());
  let mut jsoncont = String::new();
  File::open(conf_path).unwrap().read_to_string(&mut jsoncont).unwrap(); 
  let fsconf : FsConf = json::decode(&jsoncont[..]).unwrap();

  let tcpdef = &fsconf.transport;
  info!("my conf is : {:?}" , fsconf);
  let mynode = ArcKV::new(fsconf.me); // TODO a serialize type for my node with less info and base 64 vec8


  let mynodewot = mynode.clone();
//  let test = ArcKV::new(RSAPeer::new ("some name".to_string(), None, utils::sa4(Ipv4Addr::new(127,0,0,1), 9000)));

  /*    if test.key_check() && test.check_val(&test,&PeerInfoRel) {
        debug!("ok local");
        } else {
        debug!("bad local");
        }*/
  /*    let me2 = trustedpeers::RSAPeer::new (mynode.nodeid.clone(), None, mynode.address.clone(), mynode.port.clone());
        println!("MEEE2 : {:?}", me2);

        let mut tmppath = "tmp".to_string();
        let mut tmpfile = File::create(&Path::new(tmppath));
        tmpfile.write(json::encode(&me2).unwrap().into_bytes().as_slice());
        */
  let dhtrules = fsconf.rules;

  let mencdef = &fsconf.msgenc;
  let mut conffile = File::create(&Path::new("out")).unwrap();
  conffile.write_all(&json::encode(mencdef).unwrap().into_bytes()[..]).unwrap();
  // TODO access from conf!!
  let access = WotAccess {
treshold  : 2,
          refresh : Duration::seconds(300),
  };

  expand_msgenc_def!(mencdef, menc, {
  expand_transport_def!(tcpdef, transport, &mynode.to_address(), {

  struct RunningTypesImpl;

  impl RunningTypes for RunningTypesImpl {
    type A = SocketAddr;
    type P = RSAPeer;
    type V = MulKV;
    type M = WotAccess;
    type R = SimpleRules;
    type T = TmpTransportMacro;
    type E = TmpEncMacro;
  }
  let randqueryid = dhtrules.randqueryid;
          //let rc : ArcRunningContext<RunningTypes<P = RSAPeer, V = MulKV, Q = dhtrules::DhtRulesImpl, T = TmpTransportMacro, E = TmpEncMacro, R = WotAccess>> = Arc::new(
  let rc : ArcRunningContext<RunningTypesImpl> = Arc::new(
    RunningContext::new (
      mynode.0,
      access,
      SimpleRules::new(dhtrules),
      menc,
      transport,
  ));

  let route = move || Some(Inefficientmap::new());
  let querycache = move || Some(SimpleCacheQuery::new(randqueryid));
  let pconf = PathBuf::from(&fsconf.storepath[..]);
  let pdbfile = pconf.join("./files.db");
  let pfiles = pconf.join("./files");
  let prdb = pconf.join("./reverse.db");
  let pqueries = pconf.join("./query.db");
  let pwotpeers = pconf.join("./wotpeers.db");
  let pwotrels  = pconf.join("./wotrels.db");
  let pwotsigns = pconf.join("./wotsigns.db");
  let pwotrust = pconf.join("./wottrustcache.db");


  // getting bootstrap peers
  let mut jsonbcont = String::new();
  File::open(&Path::new(&boot_path[..])).unwrap().read_to_string(&mut jsonbcont).unwrap(); 
  let tmpboot_trustedpeers : Vec<RSAPeer>  = json::decode(&jsonbcont[..]).unwrap();



  // add boot peers
  let boot_trustedpeers : Vec<Arc<RSAPeer>> = tmpboot_trustedpeers.into_iter().map(|p|Arc::new(p)).collect();

  let boot_trustedpeerssend  = boot_trustedpeers.clone();
  let me_send = rc.me.clone(); // should be arckv if bootserver took arckv as param TODO
  let multip_store = move || {
    let mut storagequery = SimpleCache::new(Some(pqueries)); // TODO a path to serialize
    let ca = SimpleCache::new(Some(pdbfile));// TODO a path to serialize
    let filestore = {
      // map filestore to set key of storagequery
      let infn = &mut (|k : &ArcKV<FileKV> |{
        let v = ArcKV::new(fskeyval::FileQuery{
          query : k.name(),
          result : vec![(k.name(),k.get_key())],
        }); //warning erase existing val
        storagequery.add_val(v,(true,None))
      });
      FileStore::new(pfiles, prdb, Box::new(ca), true,infn).unwrap() // TODO fstore in conf for tests!!!
    };

    let fsstore = fskeyval::FSStore {
        queries : Box::new(storagequery),
        files   : Box::new(filestore),
    };
    let wotpeers = SimpleCache::new(Some(pwotpeers));
    let wotrels  = SimpleCache::new(Some(pwotrels));
    let wotsigns = SimpleCache::new(Some(pwotsigns));
    let wotrusts = SimpleCache::new(Some(pwotrust));
    let trust_rules : TrustRules = vec![1,1,3,3,3];
    let mut wotstore = WotStore::new(
      wotpeers,
      wotrels,
      wotsigns,
      wotrusts,
      mynodewot,
      // TODO this in param conf
      100,
      trust_rules,
    );
    let mekey = me_send.get_key();
    let ame = ArcKV(me_send);
    for p in boot_trustedpeerssend.iter() {
      // TODO add mechanism to sign only if needed (like param init sign or other)
      // all boot peer are signed as level 1 peers
      if mekey != p.get_key() {
        let akp = ArcKV(p.clone());
        // TODO Arc for derive KV do not work see add_val refactoring
        wotstore.add_val (WotKV::Peer(akp.clone()), (true,None));

        let ps : PeerSign<RSAPeer> = PeerSign::new(&ame, &(akp), 1, 0).unwrap();
        // TODO add those only if needed
        wotstore.add_val (WotKV::Sign(ps), (true,None));
      }
    }

    // TODO route possible errors to none
    Some(MultiplStore {
      fsstore : fsstore,
      wotstore : wotstore,
    })
  };


  let serv = DHT::<RunningTypesImpl>::boot_server(rc, route, querycache, multip_store, Vec::new(), boot_trustedpeers).unwrap();


  // prompt
  println!("add, find, quit?");
  let tstdin = stdin();
  let mut stdin = tstdin.lock();
  loop{
    let mut line = String::new();
    stdin.read_line(&mut line).unwrap();
    match &line[..] {
      "quit\n" => {
        break
      },
      "add\n" => {
        let mut path = String::new();
        stdin.read_line(&mut path).unwrap();
        path.pop();
        let p = Path::new (&path[..]);
        println!("Path : {:?}", p);
        let kv = <FileKV as FileKeyVal>::from_path(p.to_path_buf());
        match kv {
          None => println!("cannot initialize file"),
          Some(kv) => {
            let queryconf = QueryConf {
              mode : QueryMode::Asynch, 
              chunk : QueryChunk::None, 
              hop_hist : None,
            };
            if serv.store_val (MulKV::File(fskeyval::FSKV::File(ArcKV::new(kv))), &queryconf, 1, StoragePriority::Local){
              println!("local store ok");
            }else{
              println!("local store ko");
            };
          },
        };

      },
      "find\n" => {
        let mut query = String::new();
        stdin.read_line(&mut query).unwrap();
        query.pop();
        let queryconf = QueryConf {
          mode : QueryMode::Asynch, 
          chunk : QueryChunk::None, 
          hop_hist : Some((2,false))
        };
        let result = serv.find_val(MulK::File(fskeyval::FSK::Query(query)), &queryconf, 1, StoragePriority::Local, 1);
        match result.into_iter().next().unwrap_or(None) {
          Some(kv) => {
            println!("found {:?}",kv);
            println!("get file ix?");
            let mut six = String::new();
            stdin.read_line(&mut six).unwrap();
            six.pop();
            match kv {
              MulKV::File(fskeyval::FSKV::Query(ref q)) => {
                let hash = q.0.result.get(six.parse().unwrap());
                hash.map(|h|{
                  // TODO file size in h then use treshold to add use querychuncknone or file
                  let queryconf2 = QueryConf {
                    mode : QueryMode::Asynch, 
                    chunk : QueryChunk::Attachment, 
                    hop_hist : Some((2,false)),
                  };
                  let result = serv.find_val(MulK::File(fskeyval::FSK::File(h.1.clone())), &queryconf2, 1, StoragePriority::Local, 1);
                  println!("getfile {:?}",result);
                });
              },
              _ => (),
            };
          },
          None => println!("not found"),
        };
      },
      "newconf\n" => {
        let mut newname = String::new();
        stdin.read_line(&mut newname).unwrap();
        newname.pop();
        // jsoncont.seek(0,SeekStyle::SeekSet);
        let mut jsoncont = String::new();
        File::open(conf_path).unwrap().read_to_string(&mut jsoncont).unwrap(); 
        let mut fsconf2 : FsConf = json::decode(&jsoncont[..]).unwrap();

        let me2 = RSAPeer::new (newname, None, fsconf2.me.address.0.clone());
        fsconf2.me = me2;

        let tmppath = "tmp";
        let mut tmpfile = File::create(&Path::new(tmppath)).unwrap();
        tmpfile.write_all(&json::encode(&fsconf2).unwrap().into_bytes()[..]).unwrap();
        println!("New fsconf written to tmp");
        break;
      },

      "name\n" => {
        let mut newname = String::new();
        stdin.read_line(&mut newname).unwrap();
        newname.pop();
        // jsoncont.seek(0,SeekStyle::SeekSet);
        let mut jsoncont = String::new();
        File::open(conf_path).unwrap().read_to_string(&mut jsoncont).unwrap(); 
        let mut fsconf2 : FsConf = json::decode(&jsoncont[..]).unwrap();

        if fsconf2.me.update_info(newname){
          let tmppath = "tmp";
          let mut tmpfile = File::create(&Path::new(tmppath)).unwrap();
          tmpfile.write_all(&json::encode(&fsconf2).unwrap().into_bytes()[..]).unwrap();
          println!("New fsconf written to tmp");
          break;
        }else{
          println!("No change");
        }
      },
      c => { 
        println!("unrecognize command : {:?}", c);
      },
    };
  };

  // clean shut
  serv.shutdown();
  // wait
  serv.block();


  info!("exiting...");


  });
  });

}


derive_enum_keyval!(MulKV {}, MulK, {
    0 , Wot   => WotKV<RSAPeer> ,
    1 , File  => fskeyval::FSKV,
    });

derive_kvstore!(MultiplStore, MulKV, MulK, {
    fsstore   => fskeyval::FileShareStore,
    wotstore => WotStore<RSAPeer, ClassicWotTrust<RSAPeer>>,
    },
    {
    File   => fsstore,
    Wot    => wotstore,
    }
    );




#[derive(Debug,Clone)]
struct UnsignedOpenAccess;
unsafe impl Send for UnsignedOpenAccess {
}

impl PeerMgmtMeths<RSAPeer, MulKV> for UnsignedOpenAccess {
  fn challenge (&self, _ : &RSAPeer) -> String{
    "".to_string()
  }
  fn signmsg   (&self, _ : &RSAPeer, _ : &String) -> String{
    "".to_string()
  }
  fn checkmsg  (&self, _ : &RSAPeer, _ : &String, _ : &String) -> bool{true}
  fn accept<M : PeerMgmtMeths<RSAPeer, MulKV>, RT : RunningTypes<P=RSAPeer,V=MulKV,A=<RSAPeer as Peer>::Address,M=M>> (
      &self, 
      _ : &RSAPeer, 
      _ : &RunningProcesses<RT>, 
      _ : &ArcRunningContext<RT>) 
    -> Option<PeerPriority> {
      // direct local query to kvstore process to know which trust we got for peer (trust is use as
      // priority level.
      Some (PeerPriority::Normal)
    }
#[inline]
  fn for_accept_ping<M : PeerMgmtMeths<RSAPeer, MulKV>, RT : RunningTypes<P=RSAPeer,V=MulKV,A=<RSAPeer as Peer>::Address,M=M>> (&self, _ : &Arc<RSAPeer>, 
      _ : &RunningProcesses<RT>, 
      _ : &ArcRunningContext<RT>) 
  {
  }
}

//#[derive(Clone)]
struct WotAccess {
  /// treshold on block, peer prio is otherwhise same as trust
treshold : u8,
         /// time to wait before doing an new sign discovery
         refresh : Duration,
}

impl WotAccess {
  fn accept_rec<M : PeerMgmtMeths<RSAPeer, MulKV>, RT : RunningTypes<P=RSAPeer,V=MulKV,A=<RSAPeer as Peer>::Address,M=M>>
     (&self, 
      n : &RSAPeer, 
      rp : &RunningProcesses<RT>, 
      rc : &ArcRunningContext<RT>,
      rec : bool) 
    -> Option<PeerPriority> {

      let cachetrust = mydht::find_local_val(rp, rc, MulK::Wot(WotK::TrustQuery(n.get_key())));
      let (disc, res) = match cachetrust {
        Some(am) => {
          match am {
            MulKV::Wot(WotKV::TrustQuery(ref tr)) => {
              if tr.lastdiscovery.0 + self.refresh > time::get_time() {
                (true, None)
              } else {
                if tr.trust > self.treshold {
                  // TODO use blocked and return no option (currently None is same as block with code
                  // outside).
                  // Some(PeerPriority::Blocked)
                  (false, None)
                }else{
                  (false, Some(PeerPriority::Priority(tr.trust)))
                }
              }

            },
              _ => {
                // do not accept
                error!("unexpected message");
                (false, None)
              },
          }},
          None => (true, None),
      };
      if disc {
        if rec {
          debug!("NEED Peer Discovery");
          let queryconf = QueryConf {
mode : QueryMode::Asynch, 
     chunk : QueryChunk::None, 
     hop_hist : Some((3,true))
          };
          // for now find one promsign TODO find n promsign!!! (if we got one with only one sign,
          // we are kinda f***
          // TODO configure number of prom for discovery
          let nbprosign = 5;
          let promsigns = mydht::find_val(rp, rc, MulK::Wot(WotK::P2S(n.get_key())), &queryconf, 1, StoragePriority::NoStore, nbprosign);

          debug!("PROMSIGN got for nbpromsign {:?} : {:?}", nbprosign, promsigns);
          // fuse result
          let mut alreadysend = BTreeSet::new();
          if promsigns.len() > 0 {
            // first add peer to wotstore (required to update sign) // TODO should be include
            // in query and include in wotkv impl : because sometime it is already here
            // (subsequent to findpeer), sometime not (direct reply in Asynch mode for
            // instance).
            mydht::store_val(rp, rc, MulKV::Wot(WotKV::Peer(ArcKV::new(n.clone()))), &queryconf,1, StoragePriority::Local);
          };

          for promsign in promsigns.into_iter() {

            match promsign {
              None => (),
                   Some(aprs) => {
                     match aprs {
                       MulKV::Wot(WotKV::P2S(ref prosigns)) => {

                         // then find signpeer on them TODO evolve prosign so it contain signpeer (here slow for
                         // nothing) (new kind of keyval : like trustquery (no store)
                         for sigkey in prosigns.signby.iter() {
                           if !alreadysend.contains(sigkey){
                             // Do find on sign (about only) through multiple find to update this 
                             match mydht::find_val(rp, rc, MulK::Wot(WotK::Sign(sigkey.clone())), &queryconf, 1, StoragePriority::Local, 1).pop().unwrap_or(None) {
                               None => (),
                               Some(_) => {
                                 alreadysend.insert(sigkey.clone());
                               },
                             }
                           }
                         }

                       },
                       _ => {
                         error!("invalid wotkv response in multkv find p2s");
                       },

                     }
                   },
            }
          };
          if !alreadysend.is_empty() {
            // call accept again. Warning wot implementation peer discovery must always initiate
            // something or update will run peer discovery a lot
            self.accept_rec(n, rp, rc, false)
          } else {
            res
          }
        } else {
          res
        }
      } else {
        res
      }

    }

  // TODO propagate fn to send updates of me (if name change and so on).

}

// TODO PeerMgmtMeths to Vec<u8> !!!! here parse_str is very unsaf (si
impl PeerMgmtMeths<RSAPeer, MulKV> for WotAccess {
  fn challenge (&self, _ : &RSAPeer) -> String{
    uuid::Uuid::new_v4().to_simple_string()
  }
  fn signmsg (&self, n : &RSAPeer, chal : &String) -> String{
    match uuid::Uuid::parse_str(chal) {
      Err(_) => "bad challenge result in bad signature".to_string(),
        Ok(c) =>  n.content_sign(c.as_bytes()).to_hex(),
    }
  }
  fn checkmsg (&self, n : &RSAPeer, chal : &String, sign : &String) -> bool{ 
    match uuid::Uuid::parse_str(chal) {
      Err(_) => false,
        Ok(c) =>  n.content_check(c.as_bytes(), &sign.from_hex().unwrap()[..]), 
    }
  }
#[inline]
  fn accept<M : PeerMgmtMeths<RSAPeer, MulKV>, RT : RunningTypes<P=RSAPeer,V=MulKV,A=<RSAPeer as Peer>::Address,M=M>> (
      &self, 
      n : &RSAPeer, 
      rp : &RunningProcesses<RT>, 
      rc : &ArcRunningContext<RT>) 
    -> Option<PeerPriority> {
      self.accept_rec(n, rp, rc, true)
    }
  fn for_accept_ping<M : PeerMgmtMeths<RSAPeer, MulKV>, RT : RunningTypes<P=RSAPeer,V=MulKV,A=<RSAPeer as Peer>::Address,M=M>> (
      &self,
      n : &Arc<RSAPeer>, 
      rp : &RunningProcesses<RT>, 
      rc : &ArcRunningContext<RT>) 
  {
    // update peer in peer kv (for new peer associated info) - localonly
    let queryconf = QueryConf {
mode : QueryMode::Asynch, 
       chunk : QueryChunk::None, 
       hop_hist : None,
    };
    mydht::store_val(rp, rc, MulKV::Wot(WotKV::Peer(ArcKV(n.clone()))), &queryconf,1, StoragePriority::Local);
  }
}

#[derive(Debug,Clone)]
struct PrivateAccess {
  // everybody share a symetric key and encode challenges (salt in challenge needed)
key : usize,
}

// TODO put keyval in an Arc to avoid copies or change all msg if to use arc
#[derive(RustcDecodable,RustcEncodable,Debug,PartialEq,Eq,Clone)]
pub struct DummyKeyVal {
  pub id : String,
      //  pub value : String, // value is not needed for testing TODO latter test for value conflict
}
impl KeyVal for DummyKeyVal {
  type Key = String;
  fn get_key(&self) -> String {
    self.id.clone()
  }
  noattachment!();
}

impl SettableAttachment for DummyKeyVal {}

mod fskeyval {
  //! the keyval used for fs will be either a file id request (fileid are their hash) with list of
  //! the closest match as reply and a string as key or a hash as key and File as value
  //! It is a basic type of keyval where we plug an enum in front to get two kinds of query running
  //! Also notice that the kvstore will need to be customized to allow dynamic search

  use mydht::dhtimpl::FileKV;
//  use std::fs::File;
  use mydht::kvstoreif::{KVStore};
  use mydht::dhtif::{FileKeyVal,KeyVal};
  use mydht::CachePolicy;
//  use std::sync::Arc;

  use mydht::{Attachment,SettableAttachment};
  use rustc_serialize::{Encoder,Encodable,Decoder};
  use mydht::utils::ArcKV;

#[derive(RustcDecodable,RustcEncodable,Debug,PartialEq,Eq,Clone)]
  pub struct FileQuery {
    pub query : String, // query by file name
        pub result : Vec<(String,Vec<u8>)>, // first is actual file name second is actual file hash
  }

  impl KeyVal for FileQuery {
    type Key = String;
    fn get_key(&self) -> String {
      self.query.clone()
    }
    noattachment!();
  }
  impl SettableAttachment for FileQuery {}

  derive_enum_keyval!(FSKV {}, FSK, {
      0 , File  => ArcKV<FileKV>,
      1 , Query => ArcKV<FileQuery>,
      });


  pub struct FSStore<V1 : KeyVal, V2 : FileKeyVal> {
    pub queries : Box<KVStore<ArcKV<V1>>>,
        pub files   : Box<KVStore<ArcKV<V2>>>,
  }

  pub type FileShareStore = FSStore<FileQuery, FileKV>;

  impl KVStore<FSKV> for FileShareStore {
    //  impl KVCache<<FSKV as KeyVal>::Key, Arc<FSKV>> for FileShareStore 
    //fn c_add_val(& mut self, v : Arc<FSKV>, stconf : (bool, Option<CachePolicy>))
    fn add_val(& mut self, v : FSKV, stconf : (bool, Option<CachePolicy>)){
      match v {
        FSKV::File(ref fkv) => {
          self.files.add_val(fkv.clone(), stconf);
          // TODO manage already existing -> do it in fquery imp
          self.queries.add_val(
              ArcKV::new(FileQuery {
query : fkv.name(),
result : vec![(fkv.name(),fkv.get_key())],
})
              , stconf)
          },
          FSKV::Query(ref quer) => {
            // TODO first get and add to vec if needed
            self.queries.add_val(quer.clone(), stconf)
          },
          }
}
fn get_val(& self, k : &FSK) -> Option<FSKV> {
  match k {
    &FSK::File(ref hash) =>
      self.files.get_val(&hash).map(|afkv| FSKV::File((afkv).clone())),
      &FSK::Query(ref quer) => 
        self.queries.get_val(&quer).map(|afqu| FSKV::Query((afqu).clone())),
  }
}
fn has_val(& self, k : &FSK) -> bool {
  match k {
    &FSK::File(ref hash) =>
      self.files.has_val(&hash),
      &FSK::Query(ref quer) => 
        self.queries.has_val(&quer),
  }
}

fn remove_val(& mut self, k : &FSK) {
  match k {
    &FSK::File(ref hash) =>
      self.files.remove_val(&hash),
      &FSK::Query(ref quer) => 
        self.queries.remove_val(&quer),
  }
}

#[inline]
fn commit_store(& mut self) -> bool{
  // TODO serialize simple caches

  let r = self.files.commit_store();
  self.queries.commit_store() && r
}
}


}



}

