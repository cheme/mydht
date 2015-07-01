//! Tcp transport. Connected transport (tcp), with basic support for attachment.
//! Some overhead to send size of frame.
//! For new IO temporarilly use bincode to encode byte inf TODO when api stabilize use the right
//! serialization. + strengthen (non expected size receive and max size then split).
extern crate byteorder;
use self::byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::net::{TcpListener};
use std::net::{TcpStream};
use std::io::Result as IoResult;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::net::{SocketAddr};
use std::io::Seek;
use std::io::Write;
use std::io::Read;
use std::io::SeekFrom;
use std::fs::File;
use std::fs::OpenOptions;
use time::Duration;
use std::thread::Thread;
use peer::{Peer};
use kvstore::Attachment;
use super::{Transport,TransportStream};
use std::iter;
use utils;
use std::path::{Path,PathBuf};
use num::traits::ToPrimitive;

static BUFF_SIZE : usize = 10000; // use for attachment send/receive -- 21888 seems to be maxsize
static MAX_BUFF_SIZE : usize = 21888; // 21888 seems to be maxsize

/// Tcp struct : two options, timeout for connect and time out when connected.
pub struct Tcp {
  pub streamtimeout : Duration,
  pub connecttimeout : Duration,
}

impl Transport for Tcp {
  type Stream  = TcpStream;
  //type Address = SocketAddr;

  fn is_connected() -> bool {
    true
  }

  fn receive<C> (&self, p : &SocketAddr, closure : C) where C : Fn(TcpStream, Option<(Vec<u8>, Option<Attachment>)>) -> () {
    let mut listener = TcpListener::bind(p).unwrap();
    for socket in listener.incoming(){
        match socket {
            Err(e) => {error!("Socket acceptor error : {:?}", e);}
            Ok(mut s)  => {
              debug!("Initiating socket exchange : ");
              debug!("  - From {:?}", s.local_addr());
              debug!("  - From {:?}", s.peer_addr());
              debug!("  - With {:?}", s.peer_addr());
              s.set_keepalive (self.streamtimeout.num_seconds().to_u32());
//              s.set_timeout (self.streamtimeout.num_milliseconds().to_u64()); 
              closure(s, None);
            }
        }
    }
  }

  fn connectwith (&self, p : &SocketAddr, timeout : Duration) -> IoResult<TcpStream>{
    // connect TODO new api timeout
    //let s = TcpStream::connect_timeout(p, self.connecttimeout);
    let s = TcpStream::connect(p);
    s.map(|mut s| {
      s.set_keepalive (self.streamtimeout.num_seconds().to_u32());
      s
    })
  }

}

impl TransportStream for TcpStream {

  fn streamwrite(&mut self, bytes : &[u8], a : Option<&Attachment>) -> IoResult<()> {
    let l : usize = bytes.len();
    debug! ("sendlengt {:?}", l);
//    try!(self.write_u8(if a.is_some(){1}else{0}));
    try!(self.write_all(&[if a.is_some(){1}else{0}]));

    try!(self.write_u32::<LittleEndian>(l.to_u32().unwrap()));
    try!(self.write_all(bytes));
    let ar : Option<IoResult<()>> = a.map(|path|  {
      let mut f = try!(File::open(&path));
      debug!("trynig nwriting att");
      // send over buff size
      let fsize = f.metadata().unwrap().len();
      let nbframe = (fsize / (BUFF_SIZE).to_u64().unwrap()).to_usize().unwrap();
      let lfrsize = (fsize - (BUFF_SIZE.to_u64().unwrap() * nbframe.to_u64().unwrap())).to_usize().unwrap();
      debug!("fsize{:?}",fsize);
      debug!("nwbfr{:?}",nbframe);
      debug!("frsiz{:?}",lfrsize);
      debug!("busize{:?}",BUFF_SIZE);
      try!(self.write_u32::<LittleEndian>(nbframe.to_u32().unwrap()));
      try!(self.write_u32::<LittleEndian>(BUFF_SIZE.to_u32().unwrap()));
      try!(self.write_u32::<LittleEndian>(lfrsize.to_u32().unwrap()));
      f.seek(SeekFrom::Start(0));
      let mut tmpvec : Vec<u8> = vec![0; BUFF_SIZE];
      let buf = tmpvec.as_mut_slice();
      for i in 0..(nbframe + 1) {
        debug!("fread : {:?}", i);
        match f.read(buf) {
          Ok(nb) => {
            if (nb == BUFF_SIZE) {
              try!(self.write_all(buf));
            } else {
              if (nb != lfrsize){
                panic!("mismatch file size calc for tcp transport"); // TODO change error to manage file error to
              };
              // truncate buff
              try!(self.write(&buf[..nb]));
            }
          },
          Err(_) => {
            panic!("error happened when reading file for hashing");
            //break;
          },
        };
      };

      Ok(())
    });
    ar.unwrap_or(Ok(()))
  }

  fn streamread(&mut self) -> IoResult<(Vec<u8>,Option<Attachment>)>{
//    self.read_u8().and_then(|a|{

    let mut buf = [0];
    try!(self.read(&mut buf));
    let a = buf[0];
    let l = try!(self.read_u32::<LittleEndian>()).to_usize().unwrap();
    //self.read_le_u32().and_then(|l|{
      debug!("rec len {:?}", l);
      // TODO find better and right way to allocate null length vec
     let mut r : Vec<u8> = vec![0;l];
     let o = {let rbuf = r.as_mut_slice();
      // TODO compare size receive in res of read (to avoid bad errors...)
      // + TODO replace timeout by size max
      self.read(rbuf)};//.map(|r|{ 
        let b : bool = a == 1;
        let att = if b {
          debug!("Reding an attached file");
          println!("Reding an attached file");
          read_to_tmp(self).ok()
        }  else  {
          None
        };
       Ok((r,att))
    // })
   // })
   //})
  }

}

fn read_to_tmp(s : &mut TcpStream)-> IoResult<PathBuf> {

  let nbframe = try!(s.read_u32::<LittleEndian>()).to_usize().unwrap();
  let bsize   = try!(s.read_u32::<LittleEndian>()).to_usize().unwrap();
  let lfrsize = try!(s.read_u32::<LittleEndian>()).to_usize().unwrap();
 
  debug!("bs{:?}",bsize);
  debug!("nwbfr{:?}",nbframe);
  debug!("frsiz{:?}",lfrsize);
  let (fp, mut f) = utils::create_tmp_file();
  let mut tmpvec : Vec<u8> = vec![0; bsize];
  let buf = tmpvec.as_mut_slice();
  for i in 0..(nbframe + 1) {
    match s.read(buf) {
      Ok(nb) => {
        if (nb == bsize) {
          f.write(buf);
        } else {
          if (nb != lfrsize){
            // TODO delete file
            return Err(IoError::new(
              IoErrorKind::Other,
              "mismatch file size receive calc for tcp transport",
            ));
          };
        // truncate buff
        f.write(&buf[..nb]);
      }
    },
    Err(_) => {
      panic!("error happened when reading file from tcp");
      break;
    },
  };
  };

  f.seek(SeekFrom::Start(0));
  Ok(fp)
}


/* useless drop (defined in tcp stream) only for debugging
impl Drop for TcpStream {
    fn drop (& mut self){
                    println!("Closing socket exchange : ");
  println!("  - From {:?}", self.socket_name());
  println!("  - With {:?}", self.peer_name());

  self.close_read(); // TODO cannot do end operation , add it to trait as a function eg finalize
  // useless?

}

}*/


