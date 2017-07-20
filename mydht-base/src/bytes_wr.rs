//! bytes_wr : reader and writer (adapter) when no knowledge of content length (ie a byte stream)
//! The point is to be able to write content without knowledge of the final lenghth and without
//! loading all bytes in memory (only buffer). 
//! Common usecase is tunnel proxy where we do not know the length of content written (encyphered
//! bytes or encodable unknow content), the point is to avoid mismatch between this content and
//! further content.
//!

use std::io::{
  Write,
  Read,
  Result,
};
use readwrite_comp::{
  ExtRead,
  ExtWrite,
  CompW,
  CompR,
};



//pub type ShadowWriteOnce<'a, S : 'a + Shadow, W : 'a + Write> = CompW<'a, 'a, W , S>;
pub type BytesW<'a, W : 'a + Write, BW : 'a + ExtWrite> = CompW<'a, 'a, W , BW>;
pub type BytesR<'a, R : 'a + Read, BR : 'a + ExtRead> = CompR<'a, 'a, R , BR>;

pub fn new_bytes_w<'a, W : Write, BW : ExtWrite>(bw : &'a mut BW, w : &'a mut W) -> BytesW<'a,W,BW> {
    CompW::new(w, bw)
}
pub fn new_bytes_r<'a, R : Read, BR : ExtRead>(br : &'a mut BR, r : &'a mut R) -> BytesR<'a,R,BR> {
    CompR::new(r, br)
}

/// an adaptable (or fix) content is send, at the end if bytes 0 end otherwhise skip byte and read
/// another window
pub mod sized_windows {

use rand::OsRng;
use rand::Rng;
use std::io::{
  Write,
  Read,
  Result,
  Error as IoError,
  ErrorKind as IoErrorKind,
};
use readwrite_comp::{
  ExtRead,
  ExtWrite,
};


use std::marker::PhantomData;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

  /// conf trait
  pub trait SizedWindowsParams {
    const INIT_SIZE : usize;
    const GROWTH_RATIO : Option<(usize,usize)>;
    /// size of window is written, this way INIT_size and growth_ratio may diverge (multiple
    /// profiles)
    const WRITE_SIZE : bool;
    /// could disable random padding, default to enable for sec consideration
    const SECURE_PAD : bool = true;
  }

  #[derive(Clone)]
  pub struct SizedWindows<P : SizedWindowsParams>  {
    init_size : usize, // TODO rename to last_size
    winrem : usize,
    _p : PhantomData<P>,
  }

  impl<P : SizedWindowsParams>  SizedWindows<P> {
    /// p param is only to simplify type inference (it is an empty struct)
    pub fn new (_ : P) -> Self {
      SizedWindows {
        init_size : P::INIT_SIZE,
        winrem : P::INIT_SIZE,
        _p : PhantomData,
      }
    }
  #[inline]
  fn next_winsize<R : Read> (&mut self, r : &mut R ) -> Result<()>{
    // winrem for next
    self.winrem = if P::WRITE_SIZE {
      try!(r.read_u64::<LittleEndian>()) as usize
    } else {
      match P::GROWTH_RATIO {
         Some((n,d)) => self.init_size * n / d,
         None => P::INIT_SIZE,
      }
    };
    self.init_size = self.winrem;
    Ok(())

  }

  }

impl<P : SizedWindowsParams> ExtWrite for SizedWindows<P> {
  #[inline]
  fn write_header<W : Write>(&mut self, w : &mut W) -> Result<()> {
    if P::WRITE_SIZE {
      try!(w.write_u64::<LittleEndian>(self.winrem as u64));
    }
//    self.init_size = self.winrem;
    Ok(())
  }

  fn write_into<W : Write>(&mut self, w : &mut W, cont : &[u8]) -> Result<usize> {
    let mut tot = 0;
    while tot < cont.len() {
      if self.winrem == 0 {

        // init next winrem
        self.winrem = match P::GROWTH_RATIO {
            Some((n,d)) => self.init_size * n / d,
            None => P::INIT_SIZE,
        };

        self.init_size = self.winrem;
        // non 0 (terminal) value
        try!(w.write(&[1]));
        if P::WRITE_SIZE {
          try!(w.write_u64::<LittleEndian>(self.winrem as u64));
        };
      }



      let ww = if self.winrem + tot < cont.len() {
        try!(w.write(&cont[tot..tot + self.winrem]))
      } else {
        try!(w.write(&cont[tot..]))
      };
      tot += ww;
      self.winrem -= ww;
    }
    Ok(tot)
  }

  #[inline]
  fn write_end<W : Write>(&mut self, r : &mut W) -> Result<()> {
    // TODO buffer is not nice here and more over we need random content (nice for debugging
    // without but in tunnel it gives tunnel length) -> !!!
    let mut buffer = [0; 256];

    if P::SECURE_PAD {
      let mut rng = try!(OsRng::new()); // TODO test for perf (if cache)
      rng.fill_bytes(&mut buffer);
    };
    while self.winrem != 0 {
      let ww = if self.winrem > 256 {
        try!(r.write(&mut buffer))
      } else {
        try!(r.write(&mut buffer[..self.winrem]))
      };
      self.winrem -= ww;
    }
    // terminal 0
    try!(r.write(&[0]));
    // init as new
    self.init_size = P::INIT_SIZE;
    self.winrem = P::INIT_SIZE;
    Ok(())
  }

}


impl<P : SizedWindowsParams> ExtRead for SizedWindows<P> {
  #[inline]
  fn read_header<R : Read>(&mut self, r : &mut R) -> Result<()> {
    if P::WRITE_SIZE {
      self.winrem = try!(r.read_u64::<LittleEndian>()) as usize;
      self.init_size = self.winrem;
    }
    Ok(())
  }

  fn read_from<R : Read>(&mut self, r : &mut R, buf : &mut[u8]) -> Result<usize> {
    if self.init_size == 0 {
      // ended read (still padded)
      return Ok(0);
    }
    let rr = if self.winrem < buf.len() {
      try!(r.read(&mut buf[..self.winrem]))
    } else {
      try!(r.read(buf))
    };
    self.winrem -= rr;
    if self.winrem == 0 {
      let mut b = [0];
      let rb = try!(r.read(&mut b));
      if rb != 1 {
        return
         Err(IoError::new(IoErrorKind::Other, "No bytes after window size, do not know if ended or repeat"));
      }
      if b[0] == 0 {
        // ended (case where there is no padding or we do not know what we read and read also
        // the padding)
        self.init_size = 0;
        return Ok(rr)
      } else {
        // new window and drop this byte
        try!(self.next_winsize(r));
      }
    }
    Ok(rr)
  }
  #[inline]
  fn read_end<R : Read>(&mut self, r : &mut R) -> Result<()> {

    if self.winrem == 0 {
      self.init_size = P::INIT_SIZE;
      self.winrem = P::INIT_SIZE;
      Ok(())
    } else {
    // TODO buffer is needed here -> see if Read interface should not have a fn drop where we read
      // without buffer and drop content. For now hardcoded buffer length...
      let mut buffer = [0; 256];
      buffer[0] = 1;
      while buffer[0] != 0 {

        while self.winrem != 0 {
          let ww = if self.winrem > 256 {
            try!(r.read(&mut buffer))
          } else {
            try!(r.read(&mut buffer[..self.winrem]))
          };
          if ww ==  0 {
           error!("read end pading : missing {}",self.winrem);
           return
             Err(IoError::new(IoErrorKind::Other, "End read missing padding"));
          }
          self.winrem -= ww;
        }

        let ww = try!(r.read(&mut buffer[..1]));
        if ww != 1  {
           error!("read end no terminal 0 : nbread {}\n{}\n",ww,buffer[0]);
           return
             Err(IoError::new(IoErrorKind::Other, "End read does not find expected terminal 0 of windows"));
        }
        if buffer[0] != 0 {
          try!(self.next_winsize(r));
        }
      }
      // init as new
      self.init_size = P::INIT_SIZE;
      self.winrem = P::INIT_SIZE;

      Ok(())
    }
  }



}

}


/// an end byte is added at the end
/// This sequence is escaped in stream through escape bytes
/// Mostly for test purpose (read byte per byte). Less usefull now that bytes_wr does not have its
/// own traits anymore but is simply ExtWrite and ExtRead for Composable use
pub mod escape_term {
  // TODO if esc char check for esc seq
  // if esc char in content wr esc two times
use std::io::{
  Write,
  Read,
  Result,
};
use readwrite_comp::{
  ExtRead,
  ExtWrite,
};
use std::marker::PhantomData;


  /// contain esc byte, and if escaped, and if waiting for end read
  pub struct EscapeTerm (u8,bool,bool);
  
  impl EscapeTerm {
    pub fn new (t : u8) -> Self {
      EscapeTerm (t,false,false)
    }
  }

impl ExtRead for EscapeTerm {
  #[inline]
  fn read_header<R : Read>(&mut self, _ : &mut R) -> Result<()> {
    Ok(())
  }
  /// return 0 if ended (content might still be read afterward on reader but endof BytesWR
  fn read_from<R : Read>(&mut self, r : &mut R, buf : &mut[u8]) -> Result<usize> {
    let mut b = [0];
    if self.2 {
      return Ok(0);
    }
    let mut i = 0;
    while i < buf.len() {
      let rr = try!(r.read(&mut b[..]));
      if rr == 0 {
        return Ok(i);
      }
      if b[0] == self.0 {
        if self.1 {
        debug!("An escaped char read effectively");
          buf[i] = b[0];
          i += 1;
          self.1 = false;
        } else {
        debug!("An escaped char read start");
          self.1 = true;
        }
      } else {
        if self.1 {
        debug!("An escaped end");
          // end
          self.2 = true;
          return Ok(i);
        } else {
          buf[i] = b[0]; 
          i += 1;
        }
      }

    }
    Ok(i)
  }

  /// end read : we know the read is complete (for instance rust serialize object decoded), some
  /// finalize operation may be added (for instance read/drop padding bytes).
  #[inline]
  fn read_end<R : Read>(&mut self, r : &mut R) -> Result<()> {
    self.2 = false;
    Ok(())
  }

}

impl ExtWrite for EscapeTerm {
  #[inline]
  fn write_header<W : Write>(&mut self, _ : &mut W) -> Result<()> {
    Ok(())
  }

  fn write_into<W : Write>(&mut self, w : &mut W, cont : &[u8]) -> Result<usize> {
    for i in 0.. cont.len() {
      let b = cont[i];
      if b == self.0 {
        debug!("An escaped char");
        try!(w.write(&cont[i..i+1]));
        try!(w.write(&cont[i..i+1]));
      } else {
        try!(w.write(&cont[i..i+1]));
      }
    }
    Ok(cont.len())
  }

  /// end of content write
  #[inline]
  fn write_end<W : Write>(&mut self, w : &mut W) -> Result<()> {
    let two = if self.0 == 0 {
      try!(w.write(&[self.0,1]))
    }else {
      try!(w.write(&[self.0,0]))
    };
    // TODO clean io error if two is not 2
    assert!(two == 2);
    Ok(())
  }
}


}


