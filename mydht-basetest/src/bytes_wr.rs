//! tests for bytes_wr


use mydht_base::bytes_wr::{
 BytesR,
 BytesW,
 new_bytes_w,
 new_bytes_r,
};
use std::io::{
  Write,
  Read,
  Result,
  Cursor,
};
use rand::os::OsRng;
use rand::Rng;
use mydht_base::bytes_wr::escape_term::{
  EscapeTerm,
};
use mydht_base::bytes_wr::sized_windows::{
  SizedWindows,
  SizedWindowsParams,
};

use readwrite_comp::{
  ExtRead,
  ExtWrite,
};


#[test]
fn escape_test () {
  let mut et = EscapeTerm::new(0);
  let mut et2 = EscapeTerm::new(0);
  test_bytes_wr(
    150,
    7,
    &mut et,
    &mut et2,
  ).unwrap();
  let mut et = EscapeTerm::new(1);
  let mut et2 = EscapeTerm::new(1);
  test_bytes_wr(
    150,
    15,
    &mut et,
    &mut et2,
  ).unwrap();
  let mut et = EscapeTerm::new(3);
  let mut et2 = EscapeTerm::new(3);
  test_bytes_wr(
    150,
    200,
    &mut et,
    &mut et2,
  ).unwrap();
}

struct Params1;
struct Params2;
struct Params3;
struct Params4;

impl SizedWindowsParams for Params1 {
    const INIT_SIZE : usize = 20;
    const GROWTH_RATIO : Option<(usize,usize)> = Some((4,3));
    const WRITE_SIZE : bool = false;
}
impl SizedWindowsParams for Params2 {
    const INIT_SIZE : usize = 20;
    const GROWTH_RATIO : Option<(usize,usize)> = None;
    const WRITE_SIZE : bool = false;
}
impl SizedWindowsParams for Params3 {
    const INIT_SIZE : usize = 20;
    const GROWTH_RATIO : Option<(usize,usize)> = Some((4,3));
    const WRITE_SIZE : bool = true;
}
impl SizedWindowsParams for Params4 {
    const INIT_SIZE : usize = 20;
    const GROWTH_RATIO : Option<(usize,usize)> = None;
    const WRITE_SIZE : bool = true;
}



#[test]
fn windows_test () {
  let mut et = SizedWindows::new(Params2);
  let mut et2 = SizedWindows::new(Params2);
  test_bytes_wr(
    150,
    15,
    &mut et,
    &mut et2,
  ).unwrap();
  test_bytes_wr(
    150,
    36,
    &mut et,
    &mut et2,
  ).unwrap();

  let mut et = SizedWindows::new(Params1);
  let mut et2 = SizedWindows::new(Params1);
  test_bytes_wr(
    150,
    7,
    &mut et,
    &mut et2,
  ).unwrap();

  let mut et = SizedWindows::new(Params2);
  let mut et2 = SizedWindows::new(Params2);
  test_bytes_wr(
    150,
    15,
    &mut et,
    &mut et2,
  ).unwrap();
  let mut et = SizedWindows::new(Params3);
  let mut et2 = SizedWindows::new(Params3);
  test_bytes_wr(
    150,
    200,
    &mut et,
    &mut et2,
  ).unwrap();
  let mut et = SizedWindows::new(Params4);
  let mut et2 = SizedWindows::new(Params4);
  test_bytes_wr(
    150,
    7,
    &mut et,
    &mut et2,
  ).unwrap();
}

// TODO test with known length from readext!!!

pub fn test_bytes_wr<BW : ExtWrite, BR : ExtRead> 
  (inp_length : usize, buf_length : usize, bw : &mut BW, br : &mut BR) -> Result<()> {
  let mut rng = OsRng::new().unwrap();
  let mut outputb = Cursor::new(Vec::new());
  let mut reference = Cursor::new(Vec::new());
  let output = &mut outputb;
  let mut bufb = vec![0;buf_length];
  let mut has_started = false;
  let buf = &mut bufb;
  // knwoledge of size to write
  let mut i = 0;
  while inp_length > i {
    rng.fill_bytes(buf);
    if !has_started {
      try!(bw.write_header(output));
      has_started = true;
    };
    let ww = if inp_length - i < buf.len() {
      try!(reference.write(&buf[..inp_length - i]));
      try!(bw.write_into(output,&buf[..inp_length - i])) 
    } else {
      try!(reference.write(buf));
      try!(bw.write_into(output,buf))
    };
    assert!(ww != 0);
    i += ww;
  }
  try!(bw.write_end(output));
  // write next content
  rng.fill_bytes(buf);
  let endcontent = buf[0];
  println!("EndContent{}",endcontent);
  try!(output.write(&buf[..1]));
  output.flush();

println!("Written lenght : {}", output.get_ref().len());
  // no knowledge of size to read
  output.set_position(0);
  let input = output;
  has_started = false;
  let mut rr = 1;
  i = 0;
  while rr != 0 {
    if !has_started {
      try!(br.read_header(input));
      has_started = true;
    }
    rr = try!(br.read_from(input,buf));
    // it could go over reference as no knowledge (if padding)
    // inp length is here only to content assertion
    println!("i {} rr {} inpl {}",i,rr,inp_length);
    if rr != 0 {
    if i + rr > inp_length {
      if inp_length > i {
        let padstart = inp_length - i;
        println !("pad start {}",padstart);
        assert!(buf[..padstart] == reference.get_ref()[i..]);
      } // else the window is bigger than buffer and padding is being read
    } else {
      assert!(buf[..rr] == reference.get_ref()[i..i + rr]);
    }
    }
    i += rr;
  }
println!(" C {} - {}",i,inp_length);
  try!(br.read_end(input));
  assert!(i >= inp_length);
println!("D");
  let ni = try!(input.read(buf));
  assert!(ni == 1);
  assert!(endcontent == buf[0]);
  Ok(())

}



