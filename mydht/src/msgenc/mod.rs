//! msgenc : type of encoding.
//! there is two kind of encoding :
//! full encoding all frame are encode
//! partial : an intermediatory frame structure is used : some dht property may be directly forced by being set on decode and not include in msg : encode may force some configuring as their is the common serialize interface and a prior filter on protomessage. This should use a full encoding when not to specific.
//! Most of the time encode do not need to be a type but in some case it could : eg signing of
//! message and/or cryptiong content



pub use mydht_base::msgenc::*;

pub mod json;
















