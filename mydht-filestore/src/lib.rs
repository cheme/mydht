
#![feature(convert)]
#![feature(fs_walk)]
#[macro_use] extern crate log;
extern crate bincode;
extern crate mydht_base;
extern crate rustc_serialize;

mod filestore;
pub mod filekv;
pub use filestore::FileStore;

