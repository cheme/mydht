//! hash table with torrent bucket implementation
//! Running over Vec<u8> ids
//!
//! Warning no constraint on has key, you need to ensure KElem impl of both peer and contents are
//! of same size (the size defined in the bucket), when associated constant stabilize Bytes could
//! became Borrow<[u8;Self::KLEN]>.

extern crate bit_vec;
use self::bit_vec::BitVec;
//use std::convert::AsRef;
use std::borrow::Borrow;
use std::mem::swap;
/// need to contain a fix size u8 representation
pub trait KElem {
//  const KLEN : usize;
  fn bits_ref (& self) -> & BitVec;
  /// we do not use Eq or Bits compare as in some case (for example a keyval with long key but
  /// short bits it may make sense to compare the long key in the bucket)
  /// return true if equal
  fn bucket_elem_eq(&self, other : &Self) -> bool;
}

/// Convenience trait if bitvec representation for kelem is not stored in kelem (computed each time).
///
pub trait KElemBytes<'a> {
  type Bytes : 'a + Borrow<[u8]>;
//  const KLENB : usize;
  fn bytes_ref_keb (&'a self) -> Self::Bytes;
  fn bucket_elem_eq_keb(&self, other : &Self) -> bool;
}

// TODO test without HRTB : put
struct SimpleKElemBytes<KEB> (KEB, BitVec)
  where for<'a> KEB : KElemBytes<'a>;


impl<KEB> SimpleKElemBytes<KEB>
  where for<'a> KEB : KElemBytes<'a> {
  pub fn new(v : KEB) -> SimpleKElemBytes<KEB> {
    let bv = BitVec::from_bytes(v.bytes_ref_keb().borrow());
    SimpleKElemBytes(v,bv)
  }
}

impl<KEB> KElem for SimpleKElemBytes<KEB>
  where for<'a> KEB : KElemBytes<'a> {
//  const KLEN : usize = <Self as KElemBytes<'a>>::KLENB;
  fn bits_ref (& self) -> & BitVec {
    &self.1
  }
  fn bucket_elem_eq(&self, other : &Self) -> bool {
    self.0.bucket_elem_eq_keb(&other.0)
  }
}

// TODO switch to trait with constant over hash_size and bucket_size when possible
pub struct KNodeTable<E : KElem> {
  us: E,
  hash_size: usize, // TODO should be associated constant of KElem
  bucket_size: usize,
  bintree: Vec<usize>,
  buckets: Vec<Option<KBucket<E>>>,
  rem_ix: Vec<usize>,
}

struct KBucket<E : KElem> {
  data: Vec<E>, // TODO trait of storage with a few more operation (similar to route)!!!
  size: usize,
}


impl<E : KElem> KNodeTable<E> {
  pub fn new(us : E, bucket_size : usize, hash_size : usize) -> KNodeTable<E> {
    assert!(bucket_size > 1);
    let mut buckets = Vec::new();
    let mut bintree = Vec::new(); // TODO allocate

    bintree.push(0);
    bintree.push(0);
    buckets.push(Some(KBucket::new()));
    KNodeTable {
      us: us,
      hash_size: hash_size,
      bucket_size: bucket_size,
      bintree: bintree,
      buckets: buckets,
      rem_ix: Vec::new(),
    }
  }

  #[cfg(test)]
  fn debug_bucket(&self) {
    let mut ix = 0;
    for i in self.buckets.iter() {
      if i.is_some () {
      println!("Bucket : {} , contains {} elem", ix, i.as_ref().map(|a|a.get_size()).unwrap());
      } else {
      println!("Bucket : {} , nothing",ix);
      };
      ix += 1
    }
  }

  #[cfg(test)]
  fn assert_rep(&self, lens : &[usize]) {
    let l = self.buckets.len();
    let ll = lens.len();
    assert!(ll >= l, "result len less than actual buck : {},{}",ll,l);
    for m in l..ll {
      assert!(lens[m] == 0)
    }
    for i in 0..l {
      if self.buckets[i].is_some(){
        let bsize = self.buckets[i].as_ref().map(|b|b.get_size()).unwrap();
        assert!(bsize == lens[i], "mismatch at bucket {} , {} instead of {}", i, bsize, lens[i])
      }else{
      assert!(lens[i] == 0, "mismatch, none bucket at {} expecting 0 instead of {}", i, lens[i])
      }
    }
    
  }

  /// return distance index, index of the bucket, and the bucket
  fn resolve_bucket<'a>(&'a self, key: &BitVec) -> (usize, usize, &'a KBucket<E>) {
    let dist = bitxor(self.us.bits_ref(), key);
    let mut ix = 0;
    let mut distix = 0;
    loop {
      if let Some(&Some(ref bucket)) = self.buckets.get(ix) {
        return (distix,ix,bucket);
      } else {
        if dist[distix] {
          // walk right
          ix = self.bintree[ix * 2 + 1];
        } else {
          // walk left
          ix = self.bintree[ix * 2];
        };
        distix += 1;
      };
    };
  }

  /// return parent bucket ix and bucket ix
  fn resolve_bucket_parent<'a>(&'a self, key: &BitVec) -> (usize, usize) {
    let dist = bitxor(self.us.bits_ref(), key);
    let mut ix = 0;
    let mut parix = 0;
    let mut distix = 0;
    loop {
      if let Some(&Some(_)) = self.buckets.get(ix) {
        break
      } else {
        parix = ix;
        if dist[distix] {
          // walk right
          ix = self.bintree[ix * 2 + 1];
        } else {
          // walk left
          ix = self.bintree[ix * 2];
        };
        distix += 1;
      };
    };
    return (parix, ix)
  }



  /// return distance index, index of the bucket, and the bucket
  fn resolve_bucket_mut<'a>(&'a mut self, key: &BitVec) -> (usize, usize, &'a mut KBucket<E>) {
    let dist = bitxor(self.us.bits_ref(), key);
    let mut ix = 0;
    let mut distix = 0;
    loop {
      if let Some(&mut Some(_)) = self.buckets.get_mut(ix) {
        break
      } else {
        if dist[distix] {
          // walk right
          ix = self.bintree[ix * 2 + 1];
        } else {
          // walk left
          ix = self.bintree[ix * 2];
        };
        distix += 1;
     };
   };
   (distix,ix,self.buckets[ix].as_mut().unwrap())
  }

  // add an element in table, if already in refresh its position in table to next
  pub fn add(&mut self, elem: E) {
    assert!(!elem.bucket_elem_eq(&self.us));
    let osplited = {
      let current_bsize = self.bucket_size;
      let us = self.us.bits_ref().clone(); // unclean TODO add ref to us in bucket ??
      let (distix,ix,bucket) = {
        let br = elem.bits_ref();
        let bv : &BitVec = br.borrow();
        self.resolve_bucket_mut(bv)
      };
      let newsize = bucket.add(elem);
      if newsize > current_bsize {
        Some((distix,ix,bucket.split(&us)))
      } else {
        None
      }
    };
    if let Some((distix,ix,(left,right,ixsplit))) = osplited {
      let dist = if ixsplit > distix {
        bitxor(&left.get_closest(1)[0].bits_ref(), &self.us.bits_ref())
      } else {
        BitVec::new()
      };
      // remove xisting bucket
      self.buckets[ix] = None;
      let mut leftix = 0;
      let mut rightix = 0;
      let mut i = ix;
      for l in distix..ixsplit + 1 {
        let (is_r, tmp_left, tmp_right) = if l == ixsplit {
          (false,None,None)
        } else {
          if dist[l] {
            (true,Some(KBucket::new()),None)
          } else {
            (false,None,Some(KBucket::new()))
          }
        };
 
        // add left
        if let Some(newix) = self.rem_ix.pop() {
          leftix = newix;
          self.buckets[newix] = tmp_left;
          self.bintree[i * 2] = newix;
        } else {
          let newix = self.buckets.len();
          leftix = newix;
          self.buckets.push(tmp_left);
          self.bintree.push(0);
          self.bintree.push(0);
          self.bintree[i * 2] = newix;
        };
        // add right
        if let Some(newix) = self.rem_ix.pop() {
          rightix = newix;
          self.buckets[newix] = tmp_right;
          self.bintree[i * 2 + 1] = newix;
        } else {
          let newix = self.buckets.len();
          rightix = newix;
          self.buckets.push(tmp_right);
          self.bintree.push(0);
          self.bintree.push(0);
          self.bintree[i * 2 + 1] = newix;
        };
        if is_r {
          i = self.bintree[i * 2 + 1];
        } else {
          i = self.bintree[i * 2];
        };

      };
      self.buckets[leftix] = Some(left);
      self.buckets[rightix] = Some(right);
    };

  }

  /// might be better to use than get_closest sometimes
  pub fn get_closest_one_bucket<'a>(&'a self, id: &BitVec, nb: usize) -> Vec<&'a E> {
    let (_,_,bucket) = {
      self.resolve_bucket(id)
    };
    bucket.get_closest(nb)
  }
 
  /// very inefficient when nb is higher than bucket size
  /// TODO does not work : need paper for design... : id is just switch one bit at a time (dist + 1
  /// ) but issue is when next it goes bellow dist: we do not allways go up (need some history)
  pub fn get_closest<'a>(&'a self, id: &BitVec, nb: usize) -> Vec<&'a E> {
    let (_,ix,bucket) = {
      self.resolve_bucket(id)
    };
    let mut res = bucket.get_closest(nb);
    let mut nb = nb - res.len();
    if nb > 0 {
      // complete with random others
      let lbucket = self.buckets.len();
      if ix < lbucket {
        for i in ix + 1 .. lbucket {
          match self.buckets.get(i) {
            Some (&Some(ref b)) => {
              let r = b.get_closest(nb);
              nb = nb - r.len();
              res.extend(r);
              if nb < 1 {
                break
              }
            },
            _ => (),
          };
        }
      };
      if nb > 0  && ix > 0 {
        for i in 0 .. ix {
          match self.buckets.get(i) {
            Some (&Some(ref b)) => {
              let r = b.get_closest(nb);
              nb = nb - r.len();
              res.extend(r);
              if nb < 1 {
                break
              }
            },
            _ => (),
          };
        }
      };
    };
    res
  }
  fn has_content (&self, ix : usize) -> bool {
    match self.buckets.get(ix) {
      Some(&Some(ref bucket)) => {
        bucket.get_size() != 0
      },
      Some(&None) => {
        let i_b_left = self.bintree[ix * 2];
        let i_b_right = self.bintree[ix * 2 + 1];
        i_b_left != 0 || i_b_right != 0 // the first actually implies the second
      },
      None => {
        false
      },
    }


  }
  /// Return true if some elem was removed, false otherwise.
  pub fn remove(&mut self, elem: &E) -> bool {
    let (res,orem) = {
      let (distix,ix,bucket) = {
        let br = elem.bits_ref();
        let bv : &BitVec = br.borrow();
        self.resolve_bucket_mut(bv)
      };
      let res = bucket.remove(elem);
      // we always keep first bucket
      if bucket.get_size() == 0 && ix != 0 {
        (res,Some((distix,ix)))
      } else {
        (res,None)
      }
    };
    // TODO actually remove buckets
    orem.map(|(distix,ix)|{
      let mut ix = ix;
      // if brother has no content
      println!("us : {:?}",elem.bits_ref().borrow());
      let mut ixdst = distix;
      let mut first = true;
      loop {
      if ixdst == 0 {
        break
      };
      ixdst = ixdst - 1;
      let mut bro_bits = elem.bits_ref().borrow().clone();
      let nv = !bro_bits[ixdst];
      bro_bits.set(ixdst,nv);
      println!("br : {:?}",bro_bits);
      let (parix, ixbro) = self.resolve_bucket_parent(&bro_bits);
      if !self.has_content(ixbro) {
        // bro without content remove it 
        self.rem_ix.push(ixbro);
        if self.buckets[ixbro].is_some() {
          self.buckets[ixbro] = None;
        } else {
          self.bintree[ixbro * 2] = 0;
          self.bintree[ixbro * 2 + 1] = 0;
        };
        // set ourselves as parent
        self.rem_ix.push(ix);
        let bro = {
          let old_bro = self.buckets.get_mut(ix).unwrap();
          let mut bro = None;
          swap(&mut bro, old_bro);
          self.bintree[ix * 2] = 0;
          self.bintree[ix * 2 + 1] = 0;
          bro
        };
        self.buckets[parix] = bro;
        self.bintree[parix * 2] = 0;
        self.bintree[parix * 2 + 1] = 0;
      } else {
        if !first {
          break // TODO in fact we should sum left and right to allow merge
        };
        // set parent as bro
        self.rem_ix.push(ixbro);
        let bro = {
          let old_bro = self.buckets.get_mut(ixbro).unwrap();
          let mut bro = None;
          swap(&mut bro, old_bro);
          self.bintree[ixbro * 2] = 0;
          self.bintree[ixbro * 2 + 1] = 0;
          bro
        };
        self.buckets[parix] = bro;
        self.bintree[parix * 2] = 0;
        self.bintree[parix * 2 + 1] = 0;

        // remove ourselve
        self.rem_ix.push(ix);
        self.buckets[ix] = None;
        self.bintree[ix * 2] = 0;
        self.bintree[ix * 2 + 1] = 0;
      };
      first = false;
      ix = parix;
      }
      
    });
    
    res
 }

}
/*
#[inline]
fn distance<'a,E : KElem<'a>> (id1: &'a E, id2: &'a E) -> BitVec {
  bitxor(id1.bits_ref().borrow(), id2.bits_ref().borrow())
}*/

#[inline]
pub fn bitxor(us : & BitVec, other: & BitVec) -> BitVec {
  let mut res = us.clone();
  res.ex_union(other);
  res
}



impl<E : KElem> KBucket<E> {
  pub fn new() -> KBucket<E> {
    KBucket {
      data: Vec::new(),
      size: 0, // useless for vec impl
    }
  }

  pub fn get_size(&self) -> usize {
    return self.size
  }

  /// return the new size
  pub fn add(&mut self, elem: E) -> usize {
    // find TODO maybe change impl for larger bucket (eg using map of a key... then need to change
    // and not use eq but a getter to a ref to partialeq value associated type)
    match self.data.iter().position(|v|elem.bucket_elem_eq(v)) {
      // remove from vec 
      Some(ix) => {
        self.data.remove(ix); // TODO inefficient we only want to put it first consider biased swap_remove
      },
      None => {
        self.size += 1;
      },
    };
    self.data.push(elem);
    self.size
  }

  // empty current bucket and create two new one left (next bit at 0) and right (next bit at 1).
  pub fn split(&mut self, us : &BitVec) -> (KBucket<E>, KBucket<E>, usize) {
    let ilen = self.size;
    if ilen < 2 {
      panic!("trying to split bucket of less than two size");
    };
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut dists : Vec<BitVec> = self.data.iter().map(|e|
      bitxor(us, e.bits_ref()) // TODO dist in mem??
    ).collect();
    // find split ix
    let six = {
      let mut v = dists.get(0).unwrap(); // split should panic on bucket of size less than 2
      let mut max_xor = BitVec::new();
      for i in 1..ilen {
        let d = dists.get(i).unwrap();
        let xor = bitxor(v, d);
        if xor > max_xor {
          v = d;
          max_xor = xor;
        }
      }
      max_xor.iter().position(|b|b==true).unwrap_or(v.len())
    };
    // we do not sort/split as it loose priorities, so me simply empty origin and fill dest (pop,
    // push). Certainly not very efficient
    self.data.reverse();
    dists.reverse();
    while let Some(elem) = self.data.pop() {
      // distance could be stored in data TODO space or efficiency???
      let dist = dists.pop().unwrap();
      if dist[six] {
        right.push(elem)
      } else {
        left.push(elem)
      }
    }

    (
      KBucket{
        size :left.len(),
        data :left,
      },
      KBucket{
        size :right.len(),
        data :right,
      },
      six,
    )
  }

  /// Note we do not return &'a[u8] to allow other storage for bucket
  pub fn get_closest<'a>(&'a self, nb: usize) -> Vec<&'a E> {
    let res = if nb > self.size {
      &self.data[..]
    } else {
      &self.data[self.size - nb..]
    };
    res.iter().collect()
  }

  pub fn remove(&mut self, elem: &E) -> bool {
    let mut res = false;
    match self.data.iter().position(|v|elem.bucket_elem_eq(v)) {
      Some(ix) => {
        self.data.remove(ix); // TODO inefficient we only want to put it first consider biased swap_remove
        self.size -= 1;
        res = true;
      },
      None => (),
    };
    res
  }

}

#[cfg(test)]
impl KElem for BitVec {
  #[inline]
  fn bits_ref (& self) -> & BitVec {
    &self
  }
  #[inline]
  fn bucket_elem_eq(&self, other : &Self) -> bool {
    self == other
  }
}


#[cfg(test)]
#[derive(Clone,PartialEq,Eq)]
struct KeyConflict {
  key : BitVec,
  content : BitVec,
}

#[cfg(test)]
impl KElem for KeyConflict {
  #[inline]
  fn bits_ref (& self) -> & BitVec {
    &self.key
  }
  #[inline]
  fn bucket_elem_eq(&self, other : &Self) -> bool {
    self.content == other.content
  }
}



#[cfg(test)]
impl<'a> KElemBytes<'a> for Vec<u8> {
  // return ing Vec<u8> is stupid but it is for testing
  type Bytes = Vec<u8>;
  fn bytes_ref_keb (&'a self) -> Self::Bytes {
    self.clone()
  }
  fn bucket_elem_eq_keb(&self, other : &Self) -> bool {
    self == other
  }
}

#[cfg(test)]
impl<'a, 'b> KElemBytes<'a> for &'b [u8] {
  // using different lifetime is for test : for instance if we store a reference to a KeyVal
  // (typical implementation in route (plus need algo to hash))
  type Bytes = &'a [u8];
  fn bytes_ref_keb (&'a self) -> Self::Bytes {
     self
  }
  fn bucket_elem_eq_keb(&self, other : &Self) -> bool {
    self == other
  }
}


#[test]
fn bitxor_test () {
  let a   = 0b11000000;
  let b   = 0b01010000;
  let res = 0b10010000;

  let a = BitVec::from_bytes(&[a]);
  let b = BitVec::from_bytes(&[b]);
  let xor = bitxor(&a,&b);
  assert!(xor[0]);
  assert!(!xor[1]);
  assert_eq!(xor, BitVec::from_bytes(&[res]));
}

#[test]
fn bucket_tests () {

  let mut bucket = KBucket::new();
  assert!(bucket.get_size() == 0);
  //let elem = BitVec::from_bytes(&[0b01100000]);
  let us = BitVec::from_bytes(&[0b11000000]);
  let usv = BitVec::from_bytes(&[0b11100000]);
  let elem = BitVec::from_bytes(&[0b01100000]);
  let elem2 = BitVec::from_bytes(&[0b01000000]);
  let elem3 = BitVec::from_bytes(&[0b01010000]);
  bucket.add(elem.clone());
  assert!(bucket.get_size() == 1);
  bucket.add(elem2.clone());
  assert!(bucket.get_size() == 2);
  let (l1, mut r1, spix) = bucket.split(&us);
  assert!(l1.get_size() == 1);
  assert!(r1.get_size() == 1);
  assert!(spix == 2);

  assert!(bucket.get_size() == 2);
  r1.add(elem2.clone());
  r1.add(elem3.clone());
  assert!(r1.get_size() == 3);
  let (l3, mut r3, spix) = r1.split(&usv);
  assert!(l3.get_size() == 1);
  assert!(r3.get_size() == 2);
  assert!(spix == 2);
  let (l4, r4, spix) = r3.split(&us);
  assert!(l4.get_size() == 1);
  assert!(r4.get_size() == 1);
  assert!(spix == 3);
 
}

#[test]
fn simplekelem_tests () {
  let e1 : Vec<u8> = BitVec::from_bytes(&[0b01100001]).to_bytes();
  let e2mem : Vec<u8> = BitVec::from_bytes(&[0b01100001]).to_bytes();
  let e2 : &[u8] = &e2mem[..];
  let mut bucket1 = KBucket::new();
  let mut bucket2 = KBucket::new();
  bucket1.add(SimpleKElemBytes::new(e1));
  assert!(bucket1.get_size() == 1);
  bucket2.add(SimpleKElemBytes::new(e2));
 // assert!(bucket2.get_size() == 2);
}

#[test]
fn id_collision_tests () {

  let content1 = BitVec::from_bytes(&[0b01100001]);
  let key1 = BitVec::from_bytes(&[0b0110]);
  let content2 = BitVec::from_bytes(&[0b01100000]);
  let key2 = BitVec::from_bytes(&[0b0110]);
  let mut bucket = KBucket::new();
  bucket.add(KeyConflict{key : key1, content : content1});
  assert!(bucket.get_size() == 1);
  let kc2 = KeyConflict{key : key2, content : content2};
  bucket.add(kc2.clone());
  assert!(bucket.get_size() == 2);
  assert!(bucket.remove(&kc2));
  assert!(!bucket.remove(&kc2));
  assert!(bucket.get_size() == 1);

  // TODO check knodetable add and remove
}

#[test]
fn ktable_tests_si () {
  let bucket_size = 2; // small bucket for testing
  let hash_size = 8; // using only two bytes for testing
  let us = BitVec::from_bytes(&[0b01010101]);
  let mut ktable = KNodeTable::new(us, bucket_size, hash_size);

  let e1 = BitVec::from_bytes(&[0b00000000]);
  let e2 = BitVec::from_bytes(&[0b10000000]);
  let e3 = BitVec::from_bytes(&[0b01000000]);
  // on empty ktable
  assert!(!ktable.remove(&e1));
  assert!(0 == ktable.get_closest_one_bucket(&e1,5).len());
  ktable.assert_rep([0].as_ref());
  ktable.add(e1.clone());
  ktable.debug_bucket();
  ktable.assert_rep([1].as_ref());
  ktable.add(e1.clone());
  ktable.assert_rep([1].as_ref());
  ktable.add(e2.clone());
  ktable.assert_rep([2].as_ref());
  ktable.add(e3.clone());
  ktable.assert_rep([0,2,1].as_ref());
 // ktable.remove(&e2);
  //ktable.assert_rep([0,2,0].as_ref());


}

#[test]
fn ktable_tests_split_np () {
  let bucket_size = 2; // small bucket for testing
  let hash_size = 8; // using only two bytes for testing
  let us = BitVec::from_bytes(&[0b01010101]);
  let mut ktable = KNodeTable::new(us, bucket_size, hash_size);
  // one 
  let e1 = BitVec::from_bytes(&[0b01010000]);
  let e2 = BitVec::from_bytes(&[0b00100100]);
  let e3 = BitVec::from_bytes(&[0b00011000]);
  let e4 = BitVec::from_bytes(&[0b00111000]);
  ktable.add(e1.clone());
  println!("{:?}",ktable.bintree);
  ktable.add(e2.clone());
  println!("{:?}",ktable.bintree);
  ktable.add(e3.clone());
  println!("{:?}",ktable.bintree);
  ktable.assert_rep([0,0,0,1,2].as_ref());
  ktable.add(e4.clone());
  println!("{:?}",ktable.bintree);
  ktable.assert_rep([0,0,0,1,0,1,2].as_ref());
  let e5 = BitVec::from_bytes(&[0b10111000]);
  ktable.add(e5.clone());
  ktable.assert_rep([0,0,1,1,0,1,2].as_ref());
  ktable.remove(&e2);
  ktable.assert_rep([0,0,1,1,0,1,1].as_ref());
  ktable.remove(&e4);
  ktable.assert_rep([0,0,1,1,1,0,0].as_ref());
  ktable.add(e2.clone());
  ktable.assert_rep([0,0,1,1,2,0,0].as_ref());
  ktable.remove(&e2);
  ktable.remove(&e3);
  ktable.assert_rep([0,1,1,0,0,0,0].as_ref());
  ktable.add(e2.clone()); // recycle removed id
  ktable.assert_rep([0,2,1,0,0,0,0].as_ref());

}

#[test]
fn ktable_tests_nrem () {
  let bucket_size = 2; // small bucket for testing
  let hash_size = 8; // using only two bytes for testing
  let us = BitVec::from_bytes(&[0b01010101]);
  let mut ktable = KNodeTable::new(us, bucket_size, hash_size);
  // one 
  let e1 = BitVec::from_bytes(&[0b00010000]);
  let e2 = BitVec::from_bytes(&[0b00000100]);
  let e3 = BitVec::from_bytes(&[0b00011000]);
  ktable.add(e1.clone());
  println!("{:?}",ktable.bintree);
  ktable.add(e2.clone());
  println!("{:?}",ktable.bintree);
  ktable.add(e3.clone());
  println!("{:?}",ktable.bintree);
  ktable.assert_rep([0,0,0,0,0,0,0,2,1].as_ref());
  ktable.remove(&e2);
  println!("{:?}",ktable.bintree);
  ktable.assert_rep([2,0,0,0,0,0,0,0,0].as_ref());
  ktable.remove(&e1);
  ktable.remove(&e3);
  ktable.assert_rep([0,0,0,0,0,0,0,0,0].as_ref());
  ktable.add(e2.clone());
  ktable.assert_rep([1,0,0,0,0,0,0,0,0].as_ref());
  ktable.add(e1.clone());
  ktable.add(e3.clone());
  ktable.assert_rep([0,0,0,0,0,0,0,1,2].as_ref());
  println!("{:?}",ktable.bintree);
}

#[test]
fn ktable_get_closests () {
  let bucket_size = 2; // small bucket for testing
  let hash_size = 8; // using only two bytes for testing
  let us = BitVec::from_bytes(&[0b10101010]);
  let mut ktable = KNodeTable::new(us, bucket_size, hash_size);
  let e1 = BitVec::from_bytes(&[0b01010000]);
  let e2 = BitVec::from_bytes(&[0b00100100]);
  let e3 = BitVec::from_bytes(&[0b00011000]);
  let e4 = BitVec::from_bytes(&[0b00111000]);
  let e5 = BitVec::from_bytes(&[0b10111000]);
  ktable.add(e1.clone());
  ktable.add(e2.clone());
  ktable.add(e3.clone());
  ktable.add(e4.clone());
  ktable.add(e5.clone());
  println!("{:?}",ktable.bintree);
  ktable.assert_rep([0,1,0,0,1,2,1].as_ref());

  assert!(ktable.get_closest_one_bucket(&e1,100) == [&e1]);
  assert!(ktable.get_closest_one_bucket(&e2,100) == [&e2,&e4]);
  assert!(ktable.get_closest_one_bucket(&e2,1) == [&e4]);
  assert!(ktable.get_closest_one_bucket(&e3,100) == [&e3]);
  assert!(ktable.get_closest_one_bucket(&e4,100) == [&e2,&e4]);
  assert!(ktable.get_closest_one_bucket(&e5,100) == [&e5]);
 
  assert!(ktable.get_closest(&e1,1) == [&e1]);
  assert!(ktable.get_closest(&e1,2)[0] == &e1);
  assert!(ktable.get_closest(&e1,2).len() == 2);
  assert!(ktable.get_closest(&e4,100)[0] == &e2);
  assert!(ktable.get_closest(&e4,100)[1] == &e4);
  assert!(ktable.get_closest(&e4,100).len() == 5);
}
 

