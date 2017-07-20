
use std::fmt::Result as FmtResult;
use std::fmt::{Display,Formatter};
use std::error::Error as ErrorTrait;
use std::io::{
  Error as IoError,
  ErrorKind as IoErrorKind,
};

use std::sync::PoisonError;
use std::sync::mpsc::SendError;
use std::sync::mpsc::RecvError;

use std::result::Result as StdResult;
use bincode::rustc_serialize::EncodingError as BincError;
use bincode::rustc_serialize::DecodingError as BindError;
use byteorder::Error as BOError;
/*use std::net::parser::AddrParseError as AddrError;

impl From<AddrError> for Error {
  #[inline]
  fn from(e : AddrError) -> Error {
    Error(e.description().to_string(), ErrorKind::ExternalLib, Some(Box::new(e)))
  }
}*/

impl Into<IoError> for Error {
  #[inline]
  fn into(self) -> IoError {
//    IoError::new(e.description().to_string(), ErrorKind::ExternalLib, Some(Box::new(e)))
    IoError::new(IoErrorKind::Other, self.to_string()) // not that good
  }
}
pub trait Into<T> {
    fn into(self) -> T;
}

impl From<BOError> for Error {
  #[inline]
  fn from(e : BOError) -> Error {
    Error(e.description().to_string(), ErrorKind::ExternalLib, Some(Box::new(e)))
  }
}


impl From<BincError> for Error {
  #[inline]
  fn from(e : BincError) -> Error {
    Error(e.description().to_string(), ErrorKind::EncodingError, Some(Box::new(e)))
  }
}
impl From<BindError> for Error {
  #[inline]
  fn from(e : BindError) -> Error {
    Error(e.description().to_string(), ErrorKind::DecodingError, Some(Box::new(e)))
  }
}


#[derive(Debug)]
pub struct Error(pub String, pub ErrorKind, pub Option<Box<ErrorTrait>>);

/*pub fn from_io_error<T>(r : StdResult<T, IoError>) -> Result<T> {
  r.map_err(|e| From::from(e))
}*/

impl Error {
  // TODO may be useless and costy
  pub fn new_from<E : ErrorTrait + 'static> (msg : String, kind : ErrorKind, orig : Option<E>) -> Error {
    match orig {
      Some (e) => {
    Error(msg, kind, Some(Box::new(e)))
      },
      None => 
    Error(msg, kind, None),
    }
  }
}

impl ErrorTrait for Error {
  
  fn description(&self) -> &str {
    &self.0
  }
  fn cause(&self) -> Option<&ErrorTrait> {
    match self.2 {
      Some(ref berr) => Some (&(**berr)),
      None => None,
    }
  }
}

impl From<IoError> for Error {
  #[inline]
  fn from(e : IoError) -> Error {
    Error(e.description().to_string(), ErrorKind::IOError, Some(Box::new(e)))
  }
}

impl<T> From<PoisonError<T>> for Error {
  #[inline]
  fn from(e : PoisonError<T>) -> Error {
    let msg = format!("Poison Error on a mutex {}", e);
    Error(msg, ErrorKind::MutexError, None)
  }
}

/* TODO when possible non conflicting imple
impl<T: Send + Reflect> From<SendError<T>> for Error {
  #[inline]
  fn from(e : SendError<T>) -> Error {
    Error(e.description().to_string(), ErrorKind::ChannelSendError, Some(Box::new(e)))
  }
}
*/
impl<T> From<SendError<T>> for Error {
  #[inline]
  fn from(e : SendError<T>) -> Error {
    Error(e.to_string(), ErrorKind::ChannelSendError,None)
  }
}
impl From<RecvError> for Error {
  #[inline]
  fn from(e : RecvError) -> Error {
    Error(e.to_string(), ErrorKind::ChannelRecvError,None)
  }
}





impl Display for Error {

  fn fmt(&self, ftr : &mut Formatter) -> FmtResult {
    let kind = format!("{:?} : ",self.1);
    try!(ftr.write_str(&kind));
    try!(ftr.write_str(&self.0));
    match self.2 {
      Some(ref tr) => {
        let trace = format!(" - trace : {}", tr);
        try!(ftr.write_str(&trace[..]));
      },
      None => (),
    };
    Ok(())
  }
}

#[derive(Debug)]
pub enum ErrorKind {
  DecodingError,
  EncodingError,
  MissingFile,
  IOError,
  ExpectedError,
  ChannelSendError,
  ChannelRecvError,
  RouteError,
  PingError,
  StoreError,
  MutexError,
  Bug, // something that should algorithmically not happen
  ExternalLib,
}

#[derive(Debug)]
/// Level of error criticity, to manage errors impact.
/// Most of the time default to higher criticity and could manually be changed.
pub enum ErrorCrit {
  /// result in all programm failing, (or at least thread)
  /// that is the case for channel error of running processes
  /// Impact similar to panic but with expected close program
  /// process (store finalize...)
  Dead,
  /// Expected error, for example disconnect, need to be processed accordingly (for example try to
  /// reconnect and/or pout offline in route).
  Alive,
}
 
/// Result type internal to mydht
pub type Result<R> = StdResult<R,Error>;

