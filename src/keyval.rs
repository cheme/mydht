//! KeyVal common traits
use bincode;
use std::path::{Path, PathBuf};
use rustc_serialize::{Encodable, Decodable, Encoder, Decoder};
use rustc_serialize::hex::ToHex;
use utils;
use std::fs::File;
use std::io::Write;
use std::io::Read;
use std::hash::Hash;
use std::fmt;
use mydhtresult::Result as MDHTResult;
use num::traits::ToPrimitive;

/// Non serialize binary attached content.
/// This is usefull depending on msg encoding implementation, obviously when implementation does
/// not use Read/Write interface, message is fully in memory and big content should be serialize as
/// an attachment : a linked file (if multiple content, use tmp file then split).
/// Even with well implemented serializer, it could be usefull to use attachment because it is
/// transmitted afterward and a deserialize error could be catch before transmission of all
/// content.
pub type Attachment = PathBuf; // TODO change to Path !!! to allow copy ....


//pub trait Key : fmt::Debug + Hash + Eq + Clone + Send + Sync + Ord + 'static{}
pub trait Key : Encodable + Decodable + fmt::Debug + Eq + Clone {
//  fn key_encode(&self) -> MDHTResult<Vec<u8>>;
}

impl<K : Encodable + Decodable + fmt::Debug + Eq + Clone> Key for K {
 /* fn key_encode(&self) -> MDHTResult<Vec<u8>> {
    Ok(try!(bincode::encode(self, bincode::SizeLimit::Infinite)))
  }*/
}

/// KeyVal is the basis for DHT content, a value with key.
// TODO rem 'static and add it only when needed (Arc) : method as_static??
pub trait KeyVal : Encodable + Decodable + fmt::Debug + Clone + Send + Sync + Eq + 'static {
  /// Key type of KeyVal
  type Key : Key + Send + Sync + 'static; //aka key // Ord , Hash ... might be not mandatory but issue currently
  /// getter for key value
  fn get_key(&self) -> Self::Key; // TODO change it to return &Key (lot of useless clone in impls
  /// optional attachment
  fn get_attachment(&self) -> Option<&Attachment>;
  /// optional attachment
  fn set_attachment(& mut self, &Attachment) -> bool;

  /// default serialize encode of keyval is used at least to store localy without attachment,
  /// it could be specialize with the three next variant (or just call from).
  fn encode_dist_with_att<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error>;
  fn decode_dist_with_att<D:Decoder> (d : &mut D) -> Result<Self, D::Error>;
  fn encode_dist<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error>;
  fn decode_dist<D:Decoder> (d : &mut D) -> Result<Self, D::Error>;
  fn encode_loc_with_att<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error>;
  fn decode_loc_with_att<D:Decoder> (d : &mut D) -> Result<Self, D::Error>;

}

/// Specialization of Keyval for FileStore
pub trait FileKeyVal : KeyVal {
  /// initiate from a file (usefull for loading)
  fn from_path(PathBuf) -> Option<Self>;
  /// name of the file
  fn name(&self) -> String;
  /// get attachment
  fn get_file_ref(&self) -> &Attachment {
    self.get_attachment().unwrap()
  }
}


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
      s.emit_struct_field("hash", 0, |s|{
        Encodable::encode(&self.hash, s)
      });
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
    s.emit_struct("filekv",2, |s| {
      s.emit_struct_field("hash", 0, |s|{
        Encodable::encode(&self.hash, s)
      });
      s.emit_struct_field("name", 1, |s|{
        s.emit_str(&self.name[..])
      })
    });
    match self.get_attachment(){
      Some(path) => {
        let mut f = File::open(path).unwrap();
        let fsize = f.metadata().unwrap().len().to_usize().unwrap();
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
                    Err(e) => panic!("Miscalculated nb of frame on file read issue"),
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

      let(fp, mut file) = utils::create_tmp_file();
      try!(d.read_struct_field("chunked_file", 2, |d|{
        d.read_seq(|d, len|{
          for i in 0..len {
            try!(d.read_seq_elt(i, |d| {
            let chunk : Vec<u8> = try!(Decodable::decode(d));
            match file.write_all(&chunk[..]) {
              Ok(_) => Ok(()),
              Err(e) => Err(d.error("Could not write chunk in dest tmp file")),
            }
            }));
          };
          Ok(())
        })
      }));

 
      file.flush();

      debug!("File added to tmp {:?}", fp);
      name.and_then(move |n| hash.map (move |h| FileKV{hash : h, name : n, file : fp}))
    })
  }
}

impl KeyVal for FileKV {
  type Key = Vec<u8>;
  fn get_key(&self) -> Vec<u8> {
    self.hash.clone()
  }
  #[inline]
  /// encode without path and attachment is serialize in encoded content
  fn encode_dist_with_att<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error>{
    self.encode_with_file(s)
  }
  #[inline]
  fn decode_dist_with_att<D:Decoder> (d : &mut D) -> Result<FileKV, D::Error>{
    FileKV::decode_with_file(d)
  }
  #[inline]
   /// encode without path and attachment is to be sent as attachment (not included)
  fn encode_dist<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error>{
    self.encode_distant(s)
  }
  #[inline]
  fn decode_dist<D:Decoder> (d : &mut D) -> Result<FileKV, D::Error>{
    FileKV::decode_distant(d)
  }
  #[inline]
  /// encode with attachement in encoded content to be use with non filestore storage
  /// (default serialize include path and no attachment for use with filestore)
  fn encode_loc_with_att<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error>{
    self.encode_with_file(s)
  }
  #[inline]
  fn decode_loc_with_att<D:Decoder> (d : &mut D) -> Result<FileKV, D::Error>{
    FileKV::decode_with_file(d)
  }
  #[inline]
  /// attachment support as file (in tmp or in filestore)
  fn get_attachment(&self) -> Option<&Attachment>{
    Some(&self.file)
  }
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


