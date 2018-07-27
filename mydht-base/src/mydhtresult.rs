use std::error::Error as ErrorTrait;
use std::io::{
  Error as IoError,
  ErrorKind as IoErrorKind,
};
use std::convert::Into;
use std::sync::PoisonError;
use std::sync::mpsc::SendError;
use std::sync::mpsc::RecvError;
use std::sync::mpsc::TryRecvError;

use bincode::Error as BinError;

pub use service_pre::error::ErrorLevel;
pub use service_pre::error as service_error;

// TODO consider merging with service error??
// TODO audit use of ChannelFinishedDest

error_chain! {
  // standard types
  types {
      Error, ErrorKind, ResultExt, Result;
  }

  links {
    Service(service_error::Error, service_error::ErrorKind);
  }

  foreign_links {
//        Fmt(::std::fmt::Error);
//    Io(::std::io::Error) #[cfg(unix)];
    ChannelRecv(RecvError);
    ChannelTryRecv(TryRecvError);
    BinError(BinError);
//    Coroutine(self::coroutine::Error) #[cfg(feature = "with-coroutine")];
  }

  errors {
    ChannelSend(t: String) {
      description("Channel send error")
      display("Channel send error: '{}'", t)
    }
    SyncPoison(t: String) {
      description("Poison error")
      display("Poison error: '{}'", t)
    }
    Bug(t: String) {
      description("A bug, should not happen")
      display("Bug: '{}'", t)
    }
    Serializing(t: String) {
      description("Error occuring while serializing or deserializing")
      display("SerializingError: '{}'", t)
    }
    Io(err: IoError) {
      description(::std::error::Error::description(err))
      display("Io: '{:?}'", err)
    }
    ExpectedError {
      description("Yield")
      display("Yield")
    }
    MissingStartConf {
      description("MissingStartConf")
      display("MissingStartConf")
    }
    ChannelFinishedDest {
      description("ChannelFinishedDest")
      display("ChannelFinishedDest")
    }
  }
}

impl<T> From<SendError<T>> for Error {
  #[inline]
  fn from(e: SendError<T>) -> Error {
    ErrorKind::ChannelSend(e.to_string()).into()
  }
}

impl<T> From<PoisonError<T>> for Error {
  #[inline]
  fn from(e: PoisonError<T>) -> Error {
    ErrorKind::SyncPoison(e.to_string()).into()
  }
}

impl From<IoError> for Error {
  #[inline]
  fn from(e: IoError) -> Error {
    if let IoErrorKind::WouldBlock = e.kind() {
      ErrorKind::ExpectedError.into()
    } else {
      ErrorKind::Io(e).into()
    }
  }
}

impl Error {
  #[inline]
  pub fn level(&self) -> ErrorLevel {
    self.kind().level()
  }
}



impl ErrorKind {
  // TODO redundant with into conversion : delete
  pub fn level(&self) -> ErrorLevel {
    match *self {
      ErrorKind::Bug(_) => ErrorLevel::Panic,
      ErrorKind::MissingStartConf => ErrorLevel::Panic,
      ErrorKind::ExpectedError => ErrorLevel::Yield,
      ErrorKind::ChannelFinishedDest => ErrorLevel::Yield,
      ErrorKind::Service(ref servkind) => servkind.level(),
      /*      ErrorKind::Msg(_) => ErrorLevel::Yield,
      ErrorKind::Service(_) => ErrorLevel::Yield,
      ErrorKind::ChannelRecv(_) => ErrorLevel::Yield,
      ErrorKind::ChannelTryRecv(_) => ErrorLevel::Yield,
      ErrorKind::BinError(_) => ErrorLevel::Yield,
      ErrorKind::SyncPoison(_) => ErrorLevel::Yield,
      ErrorKind::Io(_) => ErrorLevel::Yield,
      ErrorKind::Serializing(_) => ErrorLevel::Yield,
      ErrorKind::ChannelSend(_) => ErrorLevel::Yield,*/
      _ => ErrorLevel::ShutAction,
    }
  }
}

impl Into<service_error::Error> for Error {
  // Very awkward conversion (should be call the less possible : main use case being in a service
  // implementation at the very end to return a loop error) -> therefore we do not use into()
  fn into(self) -> service_error::Error {
    //    unimplemented!()
    let nk = match self.kind() {
      ErrorKind::Bug(_) => service_error::ErrorKind::Bug("".to_string()),
      ErrorKind::MissingStartConf => service_error::ErrorKind::Panic("missing start conf".into()),
      ErrorKind::ExpectedError => service_error::ErrorKind::Yield,
      ErrorKind::ChannelFinishedDest => service_error::ErrorKind::Yield,
      ErrorKind::Service(servkind) => match servkind.level() {
        ErrorLevel::Panic => service_error::ErrorKind::Panic("".to_string()),
        ErrorLevel::ShutAction => "service error to mydht error".into(),
        ErrorLevel::Yield => service_error::ErrorKind::Yield,
      },
      _ => "service error to mydht error".into(),
    };
    service_error::Error::with_chain(self, nk)
  }
}
