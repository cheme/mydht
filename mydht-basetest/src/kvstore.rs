//! test was move from base (it could not compile in base since its trait just change
//! (bidirectional dependency))
//! TODO seems pretty useless : remove??
use keyval::KeyVal;
use node::{Node,NodeID};
use readwrite_comp::{
  ExtRead,
  ExtWrite,
};

use peer::{Peer};
use std::cmp::Eq;
use std::cmp::PartialEq;

use keyval::{Attachment,SettableAttachment};

use serde::{Serialize, Serializer, Deserializer};



// Testing only nodeK, with key different from id
#[derive(Deserialize,Serialize,Debug,Clone)]
struct NodeK2(Node,String);


impl Eq for NodeK2 {}
impl PartialEq<NodeK2> for NodeK2 {

    fn eq(&self, other: &NodeK2) -> bool {
      other.0 == self.0 && other.1 == self.1
    }

}

impl KeyVal for NodeK2 {
  type Key = String;
   fn get_key(&self) -> NodeID {
        self.1.clone()
    }

    fn get_key_ref(&self) -> &NodeID {
        &self.1
    }
    noattachment!();
  }
  impl SettableAttachment for NodeK2 { }

  impl Peer for NodeK2 {
    type Address = <Node as Peer>::Address;
    type ShadowWMsg = <Node as Peer>::ShadowWMsg;
    type ShadowRMsg = <Node as Peer>::ShadowRMsg;
    type ShadowWAuth = <Node as Peer>::ShadowWAuth;
    type ShadowRAuth = <Node as Peer>::ShadowRAuth;
    #[inline]
    fn get_address(&self) -> &<Node as Peer>::Address {
      self.0.get_address()
    }
    #[inline]
    fn get_shadower_r_auth (&self) -> Self::ShadowRAuth {
      self.0.get_shadower_r_auth()
    }
    #[inline]
    fn get_shadower_r_msg (&self) -> Self::ShadowRMsg {
      self.0.get_shadower_r_msg()
    }
 
    #[inline]
    fn get_shadower_w_auth (&self) -> Self::ShadowWAuth {
      self.0.get_shadower_w_auth()
    }
    #[inline]
    fn get_shadower_w_msg (&self) -> Self::ShadowWMsg {
      self.0.get_shadower_w_msg()
    }

  }

