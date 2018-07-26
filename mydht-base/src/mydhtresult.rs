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
use bincode::Error as BinError;
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

impl From<BinError> for Error {
  #[inline]
  fn from(e: BinError) -> Error {
    Error(
      e.description().to_string(),
      ErrorKind::SerializingError,
      Some(Box::new(e)),
    )
  }
}

/// TODO refactor (String in error replace by &'static str plus array of params)
#[derive(Debug)]
pub struct Error(pub String, pub ErrorKind, pub Option<Box<ErrorTrait>>);

#[derive(Debug, Clone, Eq, PartialEq, Copy)]
pub enum ErrorLevel {
  /// end program
  Panic,
  /// end current action but do not halt program (propagate or shut action (close connection or
  /// retry connect...)
  ShutAction,
  /// error is expected and should not end process
  Ignore,
}
/*pub fn from_io_error<T>(r : StdResult<T, IoError>) -> Result<T> {
  r.map_err(|e| From::from(e))
}*/

impl Error {
  // TODO may be useless and costy
  pub fn new_from<E: ErrorTrait + 'static>(msg: String, kind: ErrorKind, orig: Option<E>) -> Error {
    match orig {
      Some(e) => Error(msg, kind, Some(Box::new(e))),
      None => Error(msg, kind, None),
    }
  }

  pub fn level(&self) -> ErrorLevel {
    self.1.level()
  }
}
unsafe impl Send for Error {}

impl ErrorTrait for Error {
  fn description(&self) -> &str {
    &self.0
  }
  fn cause(&self) -> Option<&ErrorTrait> {
    match self.2 {
      Some(ref berr) => Some(&(**berr)),
      None => None,
    }
  }
}

impl From<IoError> for Error {
  #[inline]
  fn from(e: IoError) -> Error {
    if let IoErrorKind::WouldBlock = e.kind() {
      Error("".to_string(), ErrorKind::ExpectedError, None)
    } else {
      Error(
        e.description().to_string(),
        ErrorKind::IOError,
        Some(Box::new(e)),
      )
    }
  }
}

impl<T> From<PoisonError<T>> for Error {
  #[inline]
  fn from(e: PoisonError<T>) -> Error {
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
  fn from(e: SendError<T>) -> Error {
    Error(e.to_string(), ErrorKind::ChannelSendError, None)
  }
}
impl From<RecvError> for Error {
  #[inline]
  fn from(e: RecvError) -> Error {
    Error(e.to_string(), ErrorKind::ChannelRecvError, None)
  }
}

impl Display for Error {
  fn fmt(&self, ftr: &mut Formatter) -> FmtResult {
    let kind = format!("{:?} : ", self.1);
    try!(ftr.write_str(&kind));
    try!(ftr.write_str(&self.0));
    match self.2 {
      Some(ref tr) => {
        let trace = format!(" - trace : {}", tr);
        try!(ftr.write_str(&trace[..]));
      }
      None => (),
    };
    Ok(())
  }
}

#[derive(Debug, Eq, PartialEq)]
pub enum ErrorKind {
  SerializingError,
  MissingFile,
  MissingStartConf,
  IOError,
  ExpectedError,
  ChannelSendError,
  ChannelRecvError,
  RouteError,
  PingError,
  StoreError,
  MutexError,
  ChannelFinishedDest,
  Bug, // something that should algorithmically not happen
  ExternalLib,
  EndService, // not at expected error level : we need another level for wouldblock error kind (currently expected is seen as wouldblock which is not what we expect for this error)
}

impl ErrorKind {
  fn level(&self) -> ErrorLevel {
    match *self {
      ErrorKind::MissingStartConf | ErrorKind::Bug => ErrorLevel::Panic,
      ErrorKind::ExpectedError => ErrorLevel::Ignore,
      ErrorKind::ChannelFinishedDest => ErrorLevel::Ignore,
      _ => ErrorLevel::ShutAction,
    }
  }
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
pub type Result<R> = StdResult<R, Error>;
