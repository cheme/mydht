
use std::fs::File;
use std::path::{PathBuf};
use rustc_serialize::{Encodable, Decodable, Encoder, Decoder};
use rustc_serialize::hex::ToHex;
use mydht_base::keyval::{
  KeyVal,
  FileKeyVal,
  Attachment,
  SettableAttachment,
};
use std::io::Read;
use std::io::Write;

use mydht_base::utils;

#[derive(RustcDecodable,RustcEncodable,Debug,PartialEq,Eq,Hash,Clone)]
/// Possible implementation of a `FileKeyVal` : local info are stored with local file
/// reference
pub struct FileKV {
  /// key is hash of file
  hash : Vec<u8>,
  /// local path to file (not sent (see serialize implementation)).
  file : PathBuf,
  /// name of file
  name : String,
}

/// for file serialize as n chunck (to buff read)
const CHUNK_SIZE : usize = 8000;
impl FileKV {
  /// New FileKV from path TODO del
  pub fn new(p : PathBuf) -> FileKV {
    FileKV::from_path(p).unwrap()
  }

  fn from_path(path : PathBuf) -> Option<FileKV> {
    File::open(&path).map(|mut f|{
      // TODO choose hash lib
      //let hash = utils::hash_crypto(tmpf);
      // sha256 default impl TODO multiple hash support
      let hasho = utils::hash_default(&mut f);
      //error!("{:?}", hash);
      //error!("{:?}", hash.to_hex());
      debug!("Hash of file : {:?}", hasho.to_hex());
      let name = path.file_name().unwrap().to_str().unwrap().to_string();
      FileKV {
        hash : hasho,
        file : path,
        name : name,
      }
    }).ok()
  }

  fn encode_distant<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
    s.emit_struct("FileKV",2, |s| {
      try!(s.emit_struct_field("hash", 0, |s|{
        Encodable::encode(&self.hash, s)
      }));
      s.emit_struct_field("name", 1, |s|{
        s.emit_str(&self.name[..])
      })
    })
  }

  fn decode_distant<D:Decoder> (d : &mut D) -> Result<FileKV, D::Error> {
    d.read_struct("nonlocaldesckv",2, |d| {
      let hash : Result<Vec<u8>, D::Error>= d.read_struct_field("hash", 0, |d|{
        Decodable::decode(d)
      });
      let name : Result<String, D::Error>= d.read_struct_field("name", 1, |d|{
        d.read_str()
      });
      name.and_then(move |n| hash.map (move |h| FileKV{hash : h, name : n, file : PathBuf::new()}))
    })
  }

  fn encode_with_file<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
    debug!("encode with file");
    try!(s.emit_struct("filekv",2, |s| {
      try!(s.emit_struct_field("hash", 0, |s|{
        Encodable::encode(&self.hash, s)
      }));
      s.emit_struct_field("name", 1, |s|{
        s.emit_str(&self.name[..])
      })
    }));
    match self.get_attachment(){
      Some(path) => {
        let mut f = File::open(path).unwrap();
        let fsize = f.metadata().unwrap().len() as usize;
        if !fsize == 0 {
          let nbframe = ((fsize - 1)/ CHUNK_SIZE) + 1;
          let mut vbuf = vec!(0; CHUNK_SIZE);

          s.emit_struct_field("chuncked_file", 2, |s|{

            s.emit_seq(nbframe, |s|{
              for i in 0..nbframe {
                try!(s.emit_seq_elt(i, |s|{
                  match f.read(&mut vbuf[..]) {
                    Ok(i) if i == CHUNK_SIZE => {
                      Encodable::encode(&vbuf, s)
                    },
                    Ok(i) if i == 0 => panic!("Miscalculated nb of frame on file"),
                    Ok(i) => {
                      vbuf.truncate(i);
                      Encodable::encode(&vbuf, s)
                    },
                    Err(e) => panic!("Miscalculated nb of frame on file read issue : {:?}", e),
                  }

                }));
              };
              Ok(())
            })

          })
        } else {
        // TODO error on 0 when possible
          panic!("Trying to serialize filekv without file");// raise error
        }
      },
      None => panic!("Trying to serialize filekv without file"),// raise error
    }
  }

  fn decode_with_file<D:Decoder> (d : &mut D) -> Result<FileKV, D::Error> {
    debug!("decode with file");
    d.read_struct("filekv",2, |d| {
      let hash : Result<Vec<u8>, D::Error>= d.read_struct_field("hash", 0, |d|{
        Decodable::decode(d)
      });
      let name : Result<String, D::Error>= d.read_struct_field("name", 1, |d|{
        d.read_str()
      });

      let (fp, mut file) = match utils::create_tmp_file() {
        Ok(r) => r,
        Err(e) => return Err(d.error(format!("Could not create tmp file : {}", e).as_str())),
      };
 
      try!(d.read_struct_field("chunked_file", 2, |d|{
        d.read_seq(|d, len|{
          for i in 0..len {
            try!(d.read_seq_elt(i, |d| {
            let chunk : Vec<u8> = try!(Decodable::decode(d));
            match file.write_all(&chunk[..]) {
              Ok(_) => Ok(()),
              Err(e) => Err(d.error(format!("Could not write chunk in dest tmp file {}", e).as_str())),
            }
            }));
          };
          Ok(())
        })
      }));

      match file.flush() {
        Ok(_) => (),
        Err(e) => return Err(d.error(format!("Could flush file in dest tmp : {}", e).as_str())),
      }
 
      debug!("File added to tmp {:?}", fp);
      name.and_then(move |n| hash.map (move |h| FileKV{hash : h, name : n, file : fp}))
    })
  }
}

impl KeyVal for FileKV {
  type Key = Vec<u8>;
  fn get_key(& self) -> Vec<u8> {
    self.hash.clone()
  }
/* 
  fn get_key_ref<'a>(&'a self) -> &'a Vec<u8> {
    &self.hash
  }*/
  fn encode_kv<S:Encoder> (&self, s: &mut S, is_local : bool, with_att : bool) -> Result<(), S::Error> {
    if with_att {
  // encode with attachement in encoded content to be use with non filestore storage
  // (default serialize include path and no attachment for use with filestore)
  //
  // encode without path and attachment is serialize in encoded content
      self.encode_with_file(s)
    } else {
      if is_local {
        self.encode(s)
      } else {
   // encode without path and attachment is to be sent as attachment (not included)
        self.encode_distant(s)
      }
    }
  }
  fn decode_kv<D:Decoder> (d : &mut D, is_local : bool, with_att : bool) -> Result<FileKV, D::Error> {
    if with_att {
      FileKV::decode_with_file(d)
    } else {
      if is_local {
        FileKV::decode(d)
      } else {
        FileKV::decode_distant(d)
      }
    }
  }


  #[inline]
  /// attachment support as file (in tmp or in filestore)
  fn get_attachment(&self) -> Option<&Attachment>{
    Some(&self.file)
  }
}

impl SettableAttachment for FileKV {
  #[inline]
  /// attachment support as file (in tmp or in filestore)
  fn set_attachment(& mut self, f:&Attachment) -> bool{
    self.file = f.clone();
    true
  }
}


impl FileKeyVal for FileKV {
  fn name(&self) -> String {
    self.name.clone()
  }

  #[inline]
  fn from_path(tmpf : PathBuf) -> Option<FileKV> {
    FileKV::from_path(tmpf)
  }
}

