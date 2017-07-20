use std::io::Result as IoResult;
use std::io::Write;
use std::io::Read;
use time::Duration;

use std::net::{SocketAddr};
use std::path::PathBuf;
use std::fmt::Debug;
use mydhtresult::Result; 
use std::thread::JoinHandle;

#[cfg(feature="mio-impl")]
use coroutine::Handle as CoHandle;
#[cfg(feature="mio-impl")]
//pub mod tcp_loop;
//pub mod tcp;
//pub mod udp;
pub type Attachment = PathBuf;

// reexport from base
pub use mydht_base::transport::*;



