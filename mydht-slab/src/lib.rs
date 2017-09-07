extern crate mydht_base;
extern crate slab as innerslab;
#[cfg(test)]
extern crate mydht_basetest;



pub mod slab {
  use innerslab::{
    Slab as InnerSlab,
  };
  use mydht_base::kvcache::{
    SlabCache,
  };


  pub struct Slab<T>(InnerSlab<T>);

  impl<T> Slab<T> {
    pub fn new() -> Self {
      Slab(InnerSlab::new())
    }
    pub fn with_capacity(s : usize) -> Self {
      Slab(InnerSlab::with_capacity(s))
    }

  }

  impl<T> SlabCache<T> for Slab<T> {
    fn insert(&mut self, val: T) -> usize {
      self.0.insert(val)
    }
    fn get(&self, k : usize) -> Option<&T> {
      self.0.get(k)
    }
    fn get_mut(&mut self, k : usize) -> Option<&mut T> {
      self.0.get_mut(k)
    }
    fn has(&self, k : usize) -> bool {
      self.0.contains(k)
    }
    fn remove(&mut self, k : usize) -> Option<T> {
      if self.has(k) {
        Some(self.0.remove(k))
      } else {
        None
      }
    }
  }
}


