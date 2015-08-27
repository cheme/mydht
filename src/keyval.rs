//! KeyVal common traits
use std::path::{PathBuf};
use rustc_serialize::{Encodable, Decodable, Encoder, Decoder};
use rustc_serialize::hex::ToHex;
use utils;
use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::hash::Hash;
use std::fmt;
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
pub trait Key : Encodable + Decodable + fmt::Debug + Eq + Clone + 'static {
//  fn key_encode(&self) -> MDHTResult<Vec<u8>>;
// TODO 
//fn as_ref<KR : KeyRef<Key = Self>>(&'a self) -> KR;
}

// TODO get keyref in keyval as parametric function (keyref could not be associated type as not
// 'static).
// TODO without associated lifetime, passing lifetime to all keyval is to heavy and conversion
// trait is to hacky
pub trait KeyRef : Encodable + fmt::Debug + Eq + Clone {
}

impl<'a> KeyRef for &'a [u8] {
}

impl<'a> KeyRef for &'a str {
}

impl<'a, K : Key> KeyRef for &'a K {
}

impl<'a, K1 : KeyRef, K2 : KeyRef> KeyRef for (K1,K2) {
}

/*
impl<'a, K : Key> AsKeyRef<'a> for &'a K {
  type KeyRef = Self;
  fn as_keyref(&'a self) -> Self::KeyRef {
    self
  }
}
*/

// TODO remove for 'as_key_ref'
impl<K : Encodable + Decodable + fmt::Debug + Eq + Clone + 'static> Key for K {
 /* fn key_encode(&self) -> MDHTResult<Vec<u8>> {
    Ok(try!(bincode::encode(self, bincode::SizeLimit::Infinite)))
  }*/
}

/// KeyVal is the basis for DHT content, a value with key.
// TODO rem 'static and add it only when needed (Arc) : method as_static??
pub trait KeyVal : Encodable + Decodable + fmt::Debug + Clone + Send + Sync + Eq + SettableAttachment + 'static {
  /// Key type of KeyVal
  type Key : Key + Send + Sync; //aka key // Ord , Hash ... might be not mandatory but issue currently
  /// getter for key value
  fn get_key(&self) -> Self::Key;
  /* Get key ref usage is of for type like enum over multiple keyval or other key with calculated
  // super type we need more generic way to derive and then keyref should be doable
  // Other possible refactoring would be to add lifetime to KeyVal (then key should be either &'a
  // actual val or Container<'a>(&'a subkey). TODO rethink it (even post a question for deriving
  // strategie + askvif (limit )...
  #[inline]
  fn get_key(&self) -> Self::Key {
    self.get_key_ref().clone()
  }
  /// getter for key value
  fn get_key_ref<'a>(&'a self) -> &'a Self::Key;
  */
  /// optional attachment
  fn get_attachment(&self) -> Option<&Attachment>;
/*
  /// default serialize encode of keyval is used at least to store localy without attachment,
  /// it could be specialize with the three next variant (or just call from).
  fn encode_dist_with_att<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error>;
  fn decode_dist_with_att<D:Decoder> (d : &mut D) -> Result<Self, D::Error>;
  fn encode_dist<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error>;
  fn decode_dist<D:Decoder> (d : &mut D) -> Result<Self, D::Error>;
  fn encode_loc_with_att<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error>;
  fn decode_loc_with_att<D:Decoder> (d : &mut D) -> Result<Self, D::Error>;
*/
  /// serialize function for
  /// default to nothing specific (reuse of serialize implementation)
  /// When specific treatment, serialize implementation should reuse this with local and no
  /// attachment.
  /// First boolean indicates if the encoding is locally used (not send to other peers).
  /// Second boolean indicates if attachment must be added in the encoding (or just a reference
  /// kept).
  /// Default implementation encode through encode trait.
  fn encode_kv<S:Encoder> (&self, s: &mut S, _ : bool, _ : bool) -> Result<(), S::Error> {
    self.encode(s)
  }
  /// First boolean indicates if the encoding is locally used (not send to other peers).
  /// Second boolean indicates if attachment must be added in the encoding (or just a reference
  /// kept).
  /// Default implementation decode through encode trait.
  fn decode_kv<D:Decoder> (d : &mut D, _ : bool, _ : bool) -> Result<Self, D::Error> {
    Self::decode(d)
  }
}

/*
/// default serialize encode of keyval is used at least to store localy without attachment,
impl<KV : KeyVal> Encodable for KV {
#[inline]
  fn encode<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
    self.encode_kv(s, true, false)
  }
}

/// default serialize encode of keyval is used at least to store localy without attachment,
impl<KV : KeyVal> Decodable for KV {
#[inline]
  fn decode<D:Decoder> (d : &mut D) -> Result<KV, D::Error> {
    KV::decode_kv(d, true, false);
  }
}
*/

/// This trait has been extracted from keyval, because it is essentially use for KeyVal init,
/// but afterwards KeyVal should be used as trait object. Furthermore in some case it simplify
/// KeyVal derivation of structure by allowing use of `AsKeyValIf` :
/// ``` impl<KV : Keyval> KeyVal for AsKeyValIf<KV> ```
/// It also make trivial in most case (no attachment support) the implementation.
pub trait SettableAttachment {
  /// optional attachment
  fn set_attachment(& mut self, &Attachment) -> bool {
    // default to no attachment support
    false
  }
}
/*
/// currently this could only be used for type ,
/// If/when trait object could include associated type def, as_key_val_if should return &KeyVal,
/// and this could be extend to derivation of enum (replacement for derive_keyval macro).
pub trait AsKeyValIf : fmt::Debug + Clone + Send + Sync + Eq + SettableAttachment {
  type KV : KeyVal;
  type BP;
  fn as_keyval_if(& self) -> & Self::KV;
  fn build_from_keyval(Self::BP, Self::KV) -> Self;
  fn encode_bef<S:Encoder> (&self, s: &mut S, is_local : bool, with_att : bool) -> Result<(), S::Error> { Ok(()) }
  fn decode_bef<D:Decoder> (d : &mut D, is_local : bool, with_att : bool) -> Result<Self::BP, D::Error>;
}
*/
// Library adapter to convert AsRef
//pub struct AsRefSt<'a, KV : 'a> (&'a KV);
/*
impl<'a, KV : KeyVal, BP, AKV : AsKeyValIf< KV = KV, BP = BP>> AsRef<KV> for AsRefSt<'a, AKV> {
  fn as_ref (&self) -> &KV {
    self.0.as_keyval_if()
  }
}
*/
/*
impl<AKV : AsKeyValIf> Encodable for AKV {
  fn encode<S:Encoder> (&self, s: &mut S) -> Result<(), S::Error> {
    try!(self.encode_bef(true, false));
    self.as_keyval_if().encode(s);
  }
}
impl<AKV : AsKeyValIf> Decodable for AKV {
  fn decode<D:Decoder> (d : &mut D) -> Result<AKV, D::Error> {
    let bp = try!(<AKV as AsKeyValIf>::decode_bef(d, true, false));
    <<AKV as AsKeyValIf>::KV as Decodable>::decode(d).map(|r|Self::build_from_keyval(r, bp))
  }
}
*/
/*
impl<AKV : AsKeyValIf + Encodable + Decodable> KeyVal for AKV 
{
  type Key = <<AKV as AsKeyValIf>::KV as KeyVal>::Key;

  #[inline]
  fn get_key(&self) -> Self::Key {
    self.as_keyval_if().get_key()
  }
  #[inline]
  fn get_attachment(&self) -> Option<&Attachment> {
    self.as_keyval_if().get_attachment() 
  }
  #[inline]
  fn encode_kv<S:Encoder> (&self, s: &mut S, is_local : bool, with_att : bool) -> Result<(), S::Error> {
    try!(self.encode_bef(s, true, false));
    self.as_keyval_if().encode_kv(s, is_local, with_att)
  }
  #[inline]
  fn decode_kv<D:Decoder> (d : &mut D, is_local : bool, with_att : bool) -> Result<Self, D::Error> {
    let bp = try!(<AKV as AsKeyValIf>::decode_bef(d, true, false));
    <<AKV as AsKeyValIf>::KV as KeyVal>::decode_kv(d, is_local, with_att).map(|r|Self::build_from_keyval(bp,r))
  }

}

*/
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

