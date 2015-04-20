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
#![feature(rsawot)]

//! Toy implementation of a filestore using mydht.
#[macro_use] extern crate log;
extern crate env_logger;
#[macro_use] extern crate mydht;
extern crate rustc_serialize;
extern crate uuid;
extern crate time;
extern crate rand;
use std::env;
use std::io;
use std::io::Write;
use std::io::Read;
use std::io::BufRead;
use rustc_serialize::json;
use std::sync::Arc;
use std::sync::Mutex;
use std::io::Seek;
use std::io::SeekFrom;
use std::fs::File;
use std::io::stdin;
use std::time::Duration as OldDuration;
use time::Duration;
use mydht::Bincode;
//use mydht::Bencode;
use mydht::Json;
use std::sync::mpsc::{Sender,Receiver};

use mydht::{Udp,Tcp};
use mydht::peerif::PeerMgmtRules;
use mydht::{DHT,RunningContext, RunningProcesses};
use mydht::{PeerPriority,StoragePriority,QueryChunk,QueryMode};
use mydht::kvstoreif::KeyVal;
use mydht::kvstoreif::FileKeyVal;
use mydht::transportif::Transport;
use mydht::dhtimpl::FileKV;
use mydht::CachePolicy;
use mydht::{TrustedPeer,TrustedVal,Truster};

use mydht::dhtimpl::{SimpleCache,SimpleCacheQuery,Inefficientmap,FileStore};
use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};
use mydht::kvstoreif::{KVStore};
use mydht::kvstoreif::{KVStoreMgmtMessage};
use mydht::dhtimpl::{RSAPeer,PeerSign};
use mydht::dhtimpl::{WotStore,WotKV,WotK,TrustRules,ClassicWotTrust};
use rustc_serialize::hex::{ToHex,FromHex};
//use mydht::kvstoreif::KVCache;
use mydht::queryif::{QueryRules};
use mydht::msgencif::{MsgEnc};
use mydht::Attachment;
use std::collections::BTreeSet;
use std::path::{Path,PathBuf};

use mydht::utils::ArcKV;
use mydht::utils;

use std::net::Ipv4Addr;

#[derive(Debug,RustcDecodable,RustcEncodable)]
/// Config of the storage
pub struct FsConf {
  /// your own peer infos.
  pub me : RSAPeer,
  /// Dht rules
  pub rules : dhtrules::DhtRules,
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
macro_rules! expand_transport_def(( $t:ident, $p:ident, $ftn:expr ) => (
   match $t {
      &TransportDef::Tcp(stimeout,ctimeout) => {
        let $p = Tcp {
       streamtimeout : Duration::seconds(stimeout),
       connecttimeout : Duration::seconds(ctimeout),
    };
    $ftn;
      },
      &TransportDef::Udp(buffsize) => {
       let $p =  Udp::new(buffsize);
       $ftn;
      },
   }
));

macro_rules! expand_msgenc_def(( $t:ident, $p:ident, $ftn:expr ) => (
   match $t {
      &MsgEncDef::Bincode => {
        let $p = Bincode;
        $ftn;
      },
/*      &MsgEncDef::Bencode => {
        let $p = Bencode;
        $ftn;
      },*/
      &MsgEncDef::Json => {
        let $p = Json;
        $ftn;
      },
   }
));

fn main() {
    env_logger::init().unwrap();
    let showHelp = || println!("-C <file> to select config file, -B <file> to choose node bootstrap file");
    let mut sconfPath = "fsconf.json".to_string();
    let mut bootPath = "fsbootstrap.json".to_string();
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
                ParamState::Conf => { sconfPath = a.to_string(); state = ParamState::Normal },
                ParamState::Boot => { bootPath = a.to_string(); state = ParamState::Normal },
            }
        }
    }

    info!("using conf file : {:?}" , sconfPath);
    info!("using boot file : {:?}" , bootPath);


    // no mgmt of io error (panic cf unwrap) when reading conf
    let confPath = &PathBuf::from(&sconfPath.clone());
    let mut jsonCont = String::new();
    File::open(confPath).unwrap().read_to_string(&mut jsonCont).unwrap(); 
    let fsconf : FsConf = json::decode(&jsonCont[..]).unwrap();

    let tcpdef = &fsconf.transport;
    info!("my conf is : {:?}" , fsconf);
    let mynode = ArcKV::new(fsconf.me); // TODO a serialize type for my node with less info and base 64 vec8
   

    let mynodewot = mynode.clone();
    let test = ArcKV::new(RSAPeer::new ("some name".to_string(), None, utils::sa4(Ipv4Addr::new(127,0,0,1), 9000)));

/*    if test.key_check() && test.check_val(&test,&PeerInfoRel) {
      debug!("ok local");
    } else {
      debug!("bad local");
    }*/
/*    let me2 = trustedpeers::RSAPeer::new (mynode.nodeid.clone(), None, mynode.address.clone(), mynode.port.clone());
    println!("MEEE2 : {:?}", me2);

    let mut tmpPath = "tmp".to_string();
    let mut tmpFile = File::create(&Path::new(tmpPath));
    tmpFile.write(json::encode(&me2).unwrap().into_bytes().as_slice());
*/
    let dhtrules = fsconf.rules;

    let mencdef = &fsconf.msgenc;
    let mut confFile = File::create(&Path::new("out")).unwrap();
    confFile.write_all(&json::encode(mencdef).unwrap().into_bytes()[..]);
    // TODO access from conf!!
    let access = WotAccess {
      treshold  : 2,
      refresh : Duration::seconds(300),
    };

  expand_msgenc_def!(mencdef, menc, {
  expand_transport_def!(tcpdef, transport, {

    let rc : RunningContext<RSAPeer, MulKV, _, dhtrules::DhtRulesImpl, _, _> = Arc::new((mynode.0, access, dhtrules::DhtRulesImpl::new(dhtrules),menc,transport,None));



    let route = Inefficientmap::new();
    let querycache = SimpleCacheQuery::new();
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
    let mut jsonBCont = String::new();
    File::open(&Path::new(&bootPath[..])).unwrap().read_to_string(&mut jsonBCont).unwrap(); 
    let tmpbootTrustedPeers : Vec<RSAPeer>  = json::decode(&jsonBCont[..]).unwrap();



    // add boot peers
    let bootTrustedPeers : Vec<Arc<RSAPeer>> = tmpbootTrustedPeers.into_iter().map(|p|Arc::new(p)).collect();

    let bootTrustedPeersSend  = bootTrustedPeers.clone();
    let meSend = rc.0.clone(); // should be arckv if bootserver took arckv as param TODO
let mut multip_store = move || {
    let mut storagequery = SimpleCache::new(Some(pqueries)); // TODO a path to serialize
    let mut ca = SimpleCache::new(Some(pdbfile));// TODO a path to serialize
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
    let mut wotpeers = SimpleCache::new(Some(pwotpeers));
    let mut wotrels  = SimpleCache::new(Some(pwotrels));
    let mut wotsigns = SimpleCache::new(Some(pwotsigns));
    let mut wotrusts = SimpleCache::new(Some(pwotrust));
    let mut trustRul : TrustRules = vec![1,1,3,3,3];
    let mut wotstore = WotStore::new(
      wotpeers,
      wotrels,
      wotsigns,
      wotrusts,
      mynodewot,
      // TODO this in param conf
      100,
      trustRul,
    );
    let mekey = meSend.get_key();
    let ame = ArcKV(meSend);
    for p in bootTrustedPeersSend.iter() {
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
    Some(MultiplStore{
      fsstore : fsstore,
      wotstore : wotstore,
    })
 
};

 
    let mut serv = DHT::<_,MulKV, _, _, _, _>::boot_server(rc, route, querycache, multip_store, Vec::new(), bootTrustedPeers);

 
    // prompt
    println!("add, find, quit?");
    let mut tstdin = stdin();
    let mut stdin = tstdin.lock();
    loop{
      let mut line = String::new();
      stdin.read_line(&mut line);
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
          let f = File::open(&p); 
          let kv = <FileKV as FileKeyVal>::from_file(&mut f.unwrap());
          match kv {
            None => println!("cannot initialize file"),
            Some(kv) => {
          let queryconf = (QueryMode::Asynch, QueryChunk::None, None);
          if serv.store_val (MulKV::File(fskeyval::FSKV::File(ArcKV::new(kv))), queryconf, 1, StoragePriority::Local){
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
          let queryconf = (QueryMode::Asynch, QueryChunk::None, Some((2,false)));
          let result = serv.find_val(MulK::File(fskeyval::FSK::Query(query)), queryconf.clone(), 1, StoragePriority::Local, 1);
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
                 let queryconf2 = (QueryMode::Asynch, QueryChunk::Attachment, Some((2,false)));
                 let result = serv.find_val(MulK::File(fskeyval::FSK::File(h.1.clone())), queryconf2, 1, StoragePriority::Local, 1);
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
          // jsonCont.seek(0,SeekStyle::SeekSet);
          let mut jsonCont = String::new();
          File::open(confPath).unwrap().read_to_string(&mut jsonCont).unwrap(); 
          let mut fsconf2 : FsConf = json::decode(&jsonCont[..]).unwrap();

          let me2 = RSAPeer::new (newname, None, fsconf2.me.address.0.clone());
          fsconf2.me = me2;

          let mut tmpPath = "tmp";
          let mut tmpFile = File::create(&Path::new(tmpPath)).unwrap();
          tmpFile.write_all(&json::encode(&fsconf2).unwrap().into_bytes()[..]);
          println!("New fsconf written to tmp");
          break;
        },

        "name\n" => {
          let mut newname = String::new();
          stdin.read_line(&mut newname).unwrap();
          newname.pop();
          // jsonCont.seek(0,SeekStyle::SeekSet);
          let mut jsonCont = String::new();
          File::open(confPath).unwrap().read_to_string(&mut jsonCont).unwrap(); 
          let mut fsconf2 : FsConf = json::decode(&jsonCont[..]).unwrap();

 
          if fsconf2.me.update_info(newname){

          let mut tmpPath = "tmp";
          let mut tmpFile = File::create(&Path::new(tmpPath)).unwrap();
          tmpFile.write_all(&json::encode(&fsconf2).unwrap().into_bytes()[..]);
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

impl PeerMgmtRules<RSAPeer, MulKV> for UnsignedOpenAccess {
  fn challenge (&self, n : &RSAPeer) -> String{
    "".to_string()
  }
  fn signmsg   (&self, n : &RSAPeer, chal : &String) -> String{
    "".to_string()
  }
  fn checkmsg  (&self, n : &RSAPeer, chal : &String, sign : &String) -> bool{ true}
  fn accept<R : PeerMgmtRules<RSAPeer, MulKV>, Q : QueryRules, E : MsgEnc, T : Transport> (&self, n : &Arc<RSAPeer>, 
  rp : &RunningProcesses<RSAPeer,MulKV>, 
  rc : &RunningContext<RSAPeer,MulKV,R,Q,E,T>) 
  -> Option<PeerPriority> {
    // direct local query to kvstore process to know which trust we got for peer (trust is use as
    // priority level.
    Some (PeerPriority::Normal)
  }
  #[inline]
  fn for_accept_ping<R : PeerMgmtRules<RSAPeer, MulKV>, Q : QueryRules, E : MsgEnc, T : Transport> (&self, n : &Arc<RSAPeer>, 
  rp : &RunningProcesses<RSAPeer,MulKV>, 
  rc : &RunningContext<RSAPeer,MulKV,R,Q,E,T>) 
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
  fn accept_rec<R : PeerMgmtRules<RSAPeer, MulKV>, Q : QueryRules, E : MsgEnc, T : Transport> (&self, n : &Arc<RSAPeer>, 
  rp : &RunningProcesses<RSAPeer,MulKV>, 
  rc : &RunningContext<RSAPeer,MulKV,R,Q,E,T>,
  rec : bool) 
  -> Option<PeerPriority> {
 
    let cachetrust = mydht::find_local_val(rp, rc, MulK::Wot(WotK::TrustQuery(n.get_key())));
    let (disc, res) = match cachetrust {
      Some(am) => {
        match am {
          MulKV::Wot(WotKV::TrustQuery(ref tr)) => {
        if (tr.lastdiscovery.0 + self.refresh > time::get_time()) {
          (true, None)
        } else {
          if (tr.trust > self.treshold){
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
          let queryconf = (QueryMode::Asynch, QueryChunk::None, Some((3,true)));
          // for now find one promsign TODO find n promsign!!! (if we got one with only one sign,
          // we are kinda f***
          // TODO configure number of prom for discovery
          let nbprosign = 5;
          let promsigns = mydht::find_val(rp, rc, MulK::Wot(WotK::P2S(n.get_key())), queryconf.clone(), 1, StoragePriority::NoStore, 5);

          debug!("PROMSIGN got : {:?}", promsigns);
          // fuse result
          let mut alreadysend = BTreeSet::new();
          if promsigns.len() > 0 {
             // first add peer to wotstore (required to update sign) // TODO should be include
             // in query and include in wotkv impl : because sometime it is already here
             // (subsequent to findpeer), sometime not (direct reply in Asynch mode for
             // instance).
             mydht::store_val(rp, rc, MulKV::Wot(WotKV::Peer(ArcKV(n.clone()))), queryconf.clone(),1, StoragePriority::Local);
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
                    match mydht::find_val(rp, rc, MulK::Wot(WotK::Sign(sigkey.clone())), queryconf.clone(), 1, StoragePriority::Local, 1).pop().unwrap_or(None) {
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

// TODO PeerMgmtRules to Vec<u8> !!!! here parse_str is very unsaf (si
impl PeerMgmtRules<RSAPeer, MulKV> for WotAccess {
  fn challenge (&self, n : &RSAPeer) -> String{
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
  fn accept<R : PeerMgmtRules<RSAPeer, MulKV>, Q : QueryRules, E : MsgEnc, T : Transport> (&self, n : &Arc<RSAPeer>, 
  rp : &RunningProcesses<RSAPeer,MulKV>, 
  rc : &RunningContext<RSAPeer,MulKV,R,Q,E,T>) 
  -> Option<PeerPriority> {
    self.accept_rec(n, rp, rc, true)
  }
  fn for_accept_ping<R : PeerMgmtRules<RSAPeer, MulKV>, Q : QueryRules, E : MsgEnc, T : Transport> (&self, n : &Arc<RSAPeer>, 
  rp : &RunningProcesses<RSAPeer,MulKV>, 
  rc : &RunningContext<RSAPeer,MulKV,R,Q,E,T>) 
  {
    // update peer in peer kv (for new peer associated info) - localonly
    let queryconf = (QueryMode::Asynch, QueryChunk::None, None);
    mydht::store_val(rp, rc, MulKV::Wot(WotKV::Peer(ArcKV(n.clone()))), queryconf,1, StoragePriority::Local);
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
    nospecificencoding!(DummyKeyVal);
    noattachment!();
}


mod fskeyval {
  //! the keyval used for fs will be either a file id request (fileid are their hash) with list of
  //! the closest match as reply and a string as key or a hash as key and File as value
  //! It is a basic type of keyval where we plug an enum in front to get two kinds of query running
  //! Also notice that the kvstore will need to be customized to allow dynamic search
 
  use mydht::dhtimpl::FileKV;
  use std::fs::File;
  use std::ffi::{AsOsStr};
  use mydht::kvstoreif::{FileKeyVal,KeyVal,KVStore};
//  use mydht::kvstoreif::{KVCache};
  use mydht::CachePolicy;
  use std::sync::Arc;

  use mydht::Attachment;
  use rustc_serialize::{Encoder,Encodable,Decoder,Decodable};
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
      nospecificencoding!(FileQuery);
      noattachment!();
  }

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
   let mut r = true;
   
   r = self.files.commit_store();
   r = self.queries.commit_store() && r;
   r

 }
  }


}


mod dhtrules {
extern crate time;
extern crate rand;
extern crate rustc_serialize;
use mydht::{StoragePriority};
use time::Duration;
use std::time::Duration as OldDuration;
use mydht::{CachePolicy};
use mydht::{QueryPriority};
use mydht::queryif;
use mydht::{PeerPriority};
use std::sync::Mutex;
use self::rand::{thread_rng,Rng};
use mydht::dhtimpl::{Node};
use std::num::{ToPrimitive};

pub struct DhtRulesImpl (Mutex<usize>, DhtRules);

impl DhtRulesImpl {
  pub fn new(dr : DhtRules) -> DhtRulesImpl{
    DhtRulesImpl(Mutex::new(0), dr)
  }
}

#[derive(Debug,Clone,RustcDecodable,RustcEncodable)]
pub struct DhtRules {
  pub randqueryid : bool, // TODO switch to mutex usize or threadrng
  pub nbhopfact : u8, // nbhop is prio * fact or nbhopfact if normal // TODO invert prio (higher being 1)
  pub nbqueryfact : f32, // nbquery is 1 + query * fact
  pub lifetime : i64, // seconds of lifetime, static bound
  pub lifetimeinc : u8, // increment of lifetime per priority inc
  pub cleaninterval : Option<i64>, // in seconds if needed
  pub cacheduration : Option<i64>, // cache in seconds
  pub cacheproxied : bool, // do you cache proxied result
  pub storelocal : bool, // is result stored locally
  pub storeproxied : Option<usize>, // store only if less than nbhop // TODO implement other alternative (see comment)
}

impl queryif::QueryRules for DhtRulesImpl {


  #[inline]
  fn nbhop_dec (&self) -> u8 {
    1
  }

  // here both a static counter and a rand one just for show // TODO switch QueryID to BigInt
  fn newid (&self) -> String {
    if self.1.randqueryid {
      let mut rng = thread_rng();
      // (eg database connection)
      let s = rng.gen_range(0,65555);
      s.to_string()
    } else {
      let mut i = self.0.lock().unwrap();
      *i += 1;
      (*i).to_string()
    }
  }

  fn nbhop (&self, prio : QueryPriority) -> u8 {
    self.1.nbhopfact * prio
  }

  fn lifetime (&self, prio : QueryPriority) -> Duration {
    Duration::seconds(self.1.lifetime + (prio * self.1.lifetimeinc).to_i64().unwrap())
  }

  fn nbquery (&self, prio : QueryPriority) -> u8 {
    1 + (self.1.nbqueryfact * prio.to_f32().unwrap()).to_u8().unwrap()
  }

  fn asynch_clean(&self) -> Option<OldDuration> {
    self.1.cleaninterval.map(|s|OldDuration::seconds(s))
  }
  
  fn do_store (&self, islocal : bool, qprio : QueryPriority, sprio : StoragePriority, hopnb : Option<usize>) -> (bool,Option<CachePolicy>) {
    let cacheduration = self.1.cacheduration.map(|s|Duration::seconds(s));
    let res = match sprio {
      StoragePriority::Local =>
        if islocal && self.1.storelocal { (true,cacheduration) } else { (false,cacheduration) },
      StoragePriority::PropagateL(_) | StoragePriority::PropagateH(_) => 
        if islocal { (true,cacheduration)  } else {
          match hopnb {
            None => (false, None),
            Some(esthop) => { 
              match self.1.storeproxied {
                None => {
                  (false, None)
                },
                Some(hoptresh) => {
              if esthop > hoptresh {
                  (false, None)
              } else {
                  (false, cacheduration)
              }
                },
              }
            },
          }
        },
        StoragePriority::Trensiant => (false,cacheduration),
        StoragePriority::All =>  (true,cacheduration),
        StoragePriority::NoStore => (false,None),
      };
      (res.0, res.1.map(|d| CachePolicy(time::get_time() + d)))
  } // wether you need to store the keyval or not
}
}


