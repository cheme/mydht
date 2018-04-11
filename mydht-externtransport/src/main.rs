//! wasm for a transport module
use std::os::raw::{
  c_char,
  c_void,
};

#[macro_use] extern crate mydht_externtransport;
extern crate mydht_base;
extern crate mydht_userpoll;
use mydht_externtransport::*;

// reexport with a macro (for wasm) 
extern_func!();

fn main() {
   // placeholder 
}
