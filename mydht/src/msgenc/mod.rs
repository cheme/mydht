//! msgenc : type of encoding.
//! there is two kind of encoding :
//! full encoding all frame are encode
//! partial : an intermediatory frame structure is used : some dht property may be directly forced by being set on decode and not include in msg : encode may force some configuring as their is the common serialize interface and a prior filter on protomessage. This should use a full encoding when not to specific.
//! Most of the time encode do not need to be a type but in some case it could : eg signing of
//! message and/or cryptiong content


use keyval::{KeyVal,Attachment};
use peer::{Peer};
use query::{QueryID,QueryMsg};
use serde::{Serializer,Serialize,Deserializer,Deserialize};
use mydhtresult::Result as MDHTResult;
use std::io::Write;
use std::io::Read;
use mydhtresult::{Error,ErrorKind};
use std::fs::File;
use std::io::{Seek,SeekFrom};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use num::traits::ToPrimitive;
use mydht_base::utils;
use std::error::Error as StdError;
use self::send_variant::ProtoMessage as ProtoMessageSend;


pub use mydht_base::msgenc::*;

pub mod json;
















