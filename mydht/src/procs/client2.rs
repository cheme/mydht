//! Client service
use std::cmp::min;
use procs::OptInto;
use std::borrow::Borrow;
use peer::Peer;
use peer::{
  PeerMgmtMeths,
};
use std::collections::VecDeque;
use super::api::{
  ApiQueryId,
};
use keyval::{
  KeyVal,
  GettableAttachments,
};
use transport::{
  Transport,
};
use readwrite_comp::{
  ExtWrite,
};
use msgenc::MsgEnc;
use utils::{
  SRef,
  SToRef,
};
use super::{
  MyDHTConf,
  ShadowAuthType,
  MCCommand,
  MCCommandSend,
  PeerRefSend,
};
use std::cell::Cell;
use msgenc::send_variant::{
  ProtoMessage,
};


#[cfg(feature="restartable")]
use service::ServiceRestartable;

#[cfg(feature="restartable")]
use std::io::{
  Cursor,
  Read,
};

use service::{
  Service,
  SpawnerYield,
  WriteYield,
};
use mydhtresult::{
  Result,
  Error,
  ErrorKind,
};
use std::io::{
  Write,
};
use super::api::ApiReply;

// TODO put in its own module
pub struct WriteServiceInner<MC : MyDHTConf> {
  stream : <MC::Transport as Transport<MC::Poll>>::WriteStream,
  enc : MC::MsgEnc,
  from : MC::PeerRef,
  with : Option<MC::PeerRef>,
  peermgmt : MC::PeerMgmtMeths,
  token : usize,
  read_token : Option<usize>,
  /// shadow to use when auth is fine
  shad_msg : Option<<MC::Peer as Peer>::ShadowWMsg>,
  /// temporary for write once : later should use cleaner way to exit
  shut_after_first : bool,
  /// store message when not available (while authenticating or other)
  /// and when the input could not be use (would change message ordering)
  buff_msgs : VecDeque<MCCommand<MC>>,
  /// Restartable state (empty val if feature restartable is not activated).
  state : WriteRestartableState,
}

pub struct WriteService<MC : MyDHTConf> {
  inner : WriteServiceInner<MC>,
  /// idem
  call : RestartableFunction,
}




#[cfg(not(feature="restartable"))]
pub type WriteRestartableState = ();


#[cfg(feature="restartable")]
pub struct WriteRestartableState {
  progress : usize, // TODO smaller u (u8 or u16 probably)
  buffed_message : Cursor<Vec<u8>>, // TODO optional?
  tmp_buff_ix_st : usize,
  tmp_buff_ix_end : usize,
  // TODO make size configurable
  tmp_buff : [u8;128],
}

#[cfg(feature="restartable")]
fn initial_w_state() -> (WriteRestartableState,RestartableFunction) {

  (WriteRestartableState {
    progress : 0,
    buffed_message : Cursor::new(Vec::new()),
    tmp_buff_ix_st : 0,
    tmp_buff_ix_end : 0,
    tmp_buff : [0;128],
  },
  RestartableFunction::Nothing)
}
#[cfg(not(feature="restartable"))]
fn initial_w_state() -> (WriteRestartableState,RestartableFunction) { ((),()) }

#[cfg(not(feature="restartable"))]
pub type RestartableFunction = ();

#[cfg(feature="restartable")]
pub enum RestartableFunction {
  Ping,
  Pong(Option<Vec<u8>>,Option<Vec<u8>>),
  // more exactly forward_proto function
  Service,
  Nothing,
}

pub struct WriteServiceSend<MC : MyDHTConf> 
  where MC::LocalServiceCommand : SRef,
        MC::GlobalServiceCommand : SRef {
  stream : <MC::Transport as Transport<MC::Poll>>::WriteStream,
  enc : MC::MsgEnc,
  from : PeerRefSend<MC>,
  with : Option<PeerRefSend<MC>>,
  peermgmt : MC::PeerMgmtMeths,
  token : usize,
  read_token : Option<usize>,
  shad_msg : Option<<MC::Peer as Peer>::ShadowWMsg>,
  shut_after_first : bool,
  buff_msgs : VecDeque<MCCommandSend<MC>>,
  state : WriteRestartableState,
  call : RestartableFunction,
}

impl<MC : MyDHTConf> SRef for WriteService<MC>
  where
        <MC::Transport as Transport<MC::Poll>>::ReadStream : Send,
        <MC::Transport as Transport<MC::Poll>>::WriteStream : Send,
        MC::LocalServiceCommand : SRef,
        MC::GlobalServiceCommand : SRef {
  type Send = WriteServiceSend<MC>;
  fn get_sendable(self) -> Self::Send {
    let WriteService {
      inner : WriteServiceInner {
      stream, enc,
      from,
      with,
      buff_msgs,
      peermgmt, token, read_token, shad_msg, shut_after_first, state,
      },
      call,
    } = self;
    WriteServiceSend {
      stream, enc,
      from : from.get_sendable(),
      with : with.map(|w|w.get_sendable()),
      buff_msgs : buff_msgs.into_iter().map(|c|c.get_sendable()).collect(),
      peermgmt, token, read_token, shad_msg, shut_after_first, state, call,
    }
  }
}

impl<MC : MyDHTConf> SToRef<WriteService<MC>> for WriteServiceSend<MC>
  where
        <MC::Transport as Transport<MC::Poll>>::ReadStream : Send,
        <MC::Transport as Transport<MC::Poll>>::WriteStream : Send,
        MC::LocalServiceCommand : SRef,
        MC::GlobalServiceCommand : SRef {
  fn to_ref(self) -> WriteService<MC> {
    let WriteServiceSend {
      stream, enc,
      from,
      with,
      buff_msgs,
      peermgmt, token, read_token, shad_msg, shut_after_first, state, call,
    } = self;
    WriteService {
      inner : WriteServiceInner {
      stream, enc,
      from : from.to_ref(),
      with : with.map(|w|w.to_ref()),
      buff_msgs : buff_msgs.into_iter().map(|c|c.to_ref()).collect(),
      peermgmt, token, read_token, shad_msg, shut_after_first, state,
      },
      call,
    }
  }
}


impl<MC : MyDHTConf> WriteService<MC> {
  //pub fn new(token : usize, ws : <MC::Transport as Transport>::WriteStream, me : PeerRefSend<MC>, with : Option<PeerRefSend<MC>>, enc : MC::MsgEnc, peermgmt : MC::PeerMgmtMeths) -> Self {
  pub fn new(token : usize, ws : <MC::Transport as Transport<MC::Poll>>::WriteStream, me : MC::PeerRef, with : Option<MC::PeerRef>, enc : MC::MsgEnc, peermgmt : MC::PeerMgmtMeths, shut_after_first : bool) -> Self {
    let (state,call) = initial_w_state();
    WriteService {
      inner : WriteServiceInner {
        stream : ws,
        enc : enc,
        from : me,
        with : with,
        peermgmt : peermgmt,
        token : token,
        read_token : None,
        shad_msg : None,
        shut_after_first,
        buff_msgs : VecDeque::new(),
        state,
      },
      call,
    }
  }
}


pub fn get_shad_auth<MC : MyDHTConf>(from : &MC::PeerRef,with : &Option<MC::PeerRef>) -> <MC::Peer as Peer>::ShadowWAuth {
//pub fn get_shad_auth<MC : MyDHTConf>(from : &PeerRefSend<MC>,with : &Option<PeerRefSend<MC>>) -> <MC::Peer as Peer>::ShadowWAuth {
  match MC::AUTH_MODE {
    ShadowAuthType::NoAuth => {
      unreachable!()
    },
    ShadowAuthType::Public => {
      from.borrow().get_shadower_w_auth()
    },
    ShadowAuthType::Private => {
      match with {
        &Some (ref w) => w.borrow().get_shadower_w_auth(),
        &None => unreachable!(), // See previous setting of self.with 
          // return Err(Error("No dest in with for private network, could not allow pong".to_string(),ErrorKind::Bug,None)),
      }
    },
  }
}

/// Warning even if label are in syntax, they must be consecutive
#[cfg(not(feature="restartable"))]
macro_rules! state_switch_secq {
  ( $self:ident, $rec_call:expr, $( $label:expr,$elemt:block ) , * ) => {
    {
    $($elemt)*
    }
  }
}
#[cfg(feature="restartable")]
macro_rules! state_switch_secq {
  ( $self:ident, $rec_call:expr, $( $label:expr, $elemt:block ) , * ) => {
    {
      match ($self.state.progress) {
        $(
          $label => {
            $elemt
            $self.state.progress += 1;
            $rec_call
          },
        )*
        _ => unreachable!(),
      }
    }
  }
}


#[cfg(not(feature="restartable"))]
macro_rules! get_dest {($self:ident) => { &mut $self.stream }}
#[cfg(feature="restartable")]
macro_rules! get_dest {($self:ident) => { &mut $self.state.buffed_message }}




impl<MC : MyDHTConf> WriteServiceInner<MC> {

  fn pong_inner<S : SpawnerYield>(&mut self, chal : Option<&Vec<u8>>, option_chal2 : Option<&Vec<u8>>, async_yield : &mut S) -> Result<()> {
    state_switch_secq!(self, self.pong_inner(None,None,async_yield), 0, {
        let sig = self.peermgmt.signmsg(self.from.borrow(), &chal.as_ref().unwrap()[..]);
        {
        let ch = &chal.unwrap();
        let pmess : ProtoMessage<MC::Peer> = ProtoMessage::PONG(self.from.borrow(),ch,sig,option_chal2);
        // once shadower
        let mut shad = get_shad_auth::<MC>(&self.from,&self.with);
        shad.write_header(&mut WriteYield(get_dest!(self),async_yield))?;

        self.enc.encode_into(get_dest!(self), &mut shad, async_yield, &pmess)?;
        if let Some(ref att) = self.from.borrow().get_attachment() {
          self.enc.attach_into(get_dest!(self), &mut shad, async_yield, att)?;
        }

        shad.write_end(&mut WriteYield(get_dest!(self),async_yield))?;
        shad.flush_into(&mut WriteYield(get_dest!(self),async_yield))?;
        } // end limit pmess from borrow
    },1,{
      self.empty_buffed_message(async_yield)?;
        // debuf message here (service could get stuck on yield while containing msg otherwhise)
        // Warning pretty bad design : the fact that we got buf is related to the fact that we
        // initiate auth so it is the pong where we validate dest (second pong).
        // If we where to send message and the auth was initiated by other peer, this received pong
        // is the pong where he auth us but we did not auth him and it would be broken (sending msg
        // before full auth). This is probably something that will happen when mainloop will handle 
        // under connection/auth state and send msg (challenge cache is already here).
        // TODO indeed mainloop now have challenge_cache : time to move to a more straight forward
        // design where the under connect buffs of message is handle in mainloop. (next_msg would
        // become this buff msg).
        // TODO also refacto so we got a third auth message (split PONG is good for code readability).
        self.debuff_msgs(async_yield)?;

        return Ok(());
    }
    )
  }

      

// &mut self.stream
// ->  &mut self.state.buffed_message : note that could yield but will not
  #[cfg(not(feature="restartable"))]
  #[inline]
  fn clear_restartable_state(&mut self) {}

  #[cfg(feature="restartable")]
  fn clear_restartable_state(&mut self) {
    // init state 
    self.state.buffed_message.set_position(0);
    self.state.tmp_buff_ix_st = 0;
    self.state.tmp_buff_ix_end = 0;
    self.state.buffed_message.get_mut().clear();
  }

  #[cfg(feature="restartable")]
  fn empty_buffed_message<S : SpawnerYield>(&mut self, async_yield : &mut S) -> Result<()> {
    {
      let mut dest = WriteYield(&mut self.stream, async_yield);
      loop {
        if self.state.tmp_buff_ix_st < self.state.tmp_buff_ix_end {
          let w = dest.write(&self.state.tmp_buff[self.state.tmp_buff_ix_st..self.state.tmp_buff_ix_end])?;
          self.state.tmp_buff_ix_st += w;
        } else {
          self.state.tmp_buff_ix_st = 0;
          self.state.tmp_buff_ix_end = 0;
          let r = self.state.buffed_message.read(&mut self.state.tmp_buff[..])?;
          if (r == 0) {
            break;
          }
          self.state.tmp_buff_ix_end = r;
        }
      }
    }
    self.clear_restartable_state();
    Ok(())
  }
  #[cfg(not(feature="restartable"))]
  #[inline]
  fn empty_buffed_message<S : SpawnerYield>(&mut self, _async_yield : &mut S) -> Result<()> { Ok(()) }

  // TODO make restartable as pong
  fn ping<S : SpawnerYield>(&mut self, chal : Vec<u8>, async_yield : &mut S) -> Result<()> {
      debug!("Client service sending ping command");
      let sign = self.peermgmt.signmsg(self.from.borrow(), &chal);
      // we do not wait for a result
      let pmess : ProtoMessage<MC::Peer> = ProtoMessage::PING(self.from.borrow(), chal.clone(), sign);
      let mut shad = get_shad_auth::<MC>(&self.from,&self.with);
      shad.write_header(&mut WriteYield(&mut self.stream,async_yield))?;

      self.enc.encode_into(&mut self.stream, &mut shad, async_yield, &pmess)?;
      if let Some(ref att) = self.from.borrow().get_attachment() {
      debug!("ping put att");
        self.enc.attach_into(&mut self.stream, &mut shad, async_yield, att)?;
      }
      shad.write_end(&mut WriteYield(&mut self.stream,async_yield))?;
      shad.flush_into(&mut WriteYield(&mut self.stream,async_yield))?;
      Ok(())
  }

  fn service<S : SpawnerYield>(&mut self, command : MCCommand<MC>, async_yield : &mut S) -> Result<WriteReply<MC>> {
        if MC::AUTH_MODE != ShadowAuthType::NoAuth && self.with.is_none() {
          self.buff_msgs.push_front(command);
          return Ok(WriteReply::NoRep);
        }
        self.debuff_msgs(async_yield)?;
        debug!("Client service proxying command, write token {}",self.token);
        self.forward_proto(command,async_yield)?;

        if self.shut_after_first {
          return Err(Error("Write once service end".to_string(), ErrorKind::EndService,None));
        }
        Ok(WriteReply::NoRep)
  }

}

impl<MC : MyDHTConf> WriteService<MC> {
  #[cfg(not(feature="restartable"))]
  fn pong<S : SpawnerYield>(&mut self, rp : MC::PeerRef, chal : Vec<u8>, read_token : usize, option_chal2 : Option<Vec<u8>>, async_yield : &mut S) -> Result<()> {
        self.inner.read_token = Some(read_token);
        // update dest : this ensure that on after auth with is initialized! Remember that with may
        // contain initializing shared secret for message shadow.
        self.inner.with = Some(rp);
        self.inner.pong_inner(Some(&chal),option_chal2.as_ref(),async_yield)
  }

  #[cfg(feature="restartable")]
  fn pong<S : SpawnerYield>(&mut self, rp : MC::PeerRef, chal : Vec<u8>, read_token : usize, option_chal2 : Option<Vec<u8>>, async_yield : &mut S) -> Result<()> {


        self.inner.read_token = Some(read_token);
        // update dest : this ensure that on after auth with is initialized! Remember that with may
        // contain initializing shared secret for message shadow.
        self.inner.with = Some(rp);
        self.call = RestartableFunction::Pong(Some(chal.clone()),option_chal2.clone());
        self.inner.pong_inner(Some(&chal),option_chal2.as_ref(),async_yield)
  }
 
}

impl<MC : MyDHTConf> Service for WriteService<MC> {
  type CommandIn = WriteCommand<MC>;
  type CommandOut = WriteReply<MC>;

  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, async_yield : &mut S) -> Result<Self::CommandOut> {
    match req {
      WriteCommand::Pong(rp,chal,read_token,option_chal2) => {
        self.inner.clear_restartable_state(); // useless ?? here for error case but on error restarting makes no sens (comm is desynch)
        self.pong(rp,chal,read_token,option_chal2,async_yield)?;
      },
      WriteCommand::Ping(chal) => {
  //        return Ok(WriteReply::MainLoop(MainLoopCommand::NewChallenge(self.token,chal)));
        self.inner.ping(chal,async_yield)?;

      },
      WriteCommand::Service(command) => {
        return self.inner.service(command,async_yield);
      },
/*      WriteCommand::GlobalService(command) => {
        self.forward_proto(MCCommand::Global(command),async_yield)?;
      },*/
    }
    // default to no rep
    Ok(WriteReply::NoRep)
  }
}



#[cfg(feature="restartable")]
impl<MC : MyDHTConf> ServiceRestartable for WriteService<MC> {
  fn restart<S : SpawnerYield>(&mut self, async_yield : &mut S) -> Result<Option<Self::CommandOut>> {
    match self.call {
      RestartableFunction::Ping => unimplemented!(),
      RestartableFunction::Pong(ref chal,ref option_chal2) => {
        self.inner.pong_inner(chal.as_ref(),option_chal2.as_ref(),async_yield)?;
        Ok(Some(WriteReply::NoRep))
      },
      RestartableFunction::Service => unimplemented!(),
      RestartableFunction::Nothing => Ok(None),
    }
  }
}


impl<MC : MyDHTConf> WriteServiceInner<MC> {

  #[inline]
  fn debuff_msgs<S : SpawnerYield>(&mut self, async_yield : &mut S) -> Result<()> {
    while let Some(command) = self.buff_msgs.pop_back() {
      self.forward_proto(command,async_yield)?;
    }
    Ok(())
  }

  fn forward_proto<S : SpawnerYield, R : OptInto<MC::ProtoMsg>>(&mut self, command: R, async_yield : &mut S) -> Result<()> {
    if command.can_into() {


      if self.shad_msg.is_none() {
        let mut shad = match MC::AUTH_MODE {
          ShadowAuthType::NoAuth => {
            self.from.borrow().get_shadower_w_msg()
          },
          ShadowAuthType::Public | ShadowAuthType::Private => {
            match self.with {
              Some(ref w) => w.borrow().get_shadower_w_msg(),
              None => {
                return Err(Error("route return slab may contain write ref of non initialized (connected), a route impl issue".to_string(), ErrorKind::Bug,None));
              },
            }
          },
        };

        self.shad_msg = Some(shad);
      }

//      let mut selfstream = Cursor::new(Vec::new());
      let mut shad = self.shad_msg.as_mut().unwrap();

      shad.write_header(&mut WriteYield(&mut self.stream,async_yield))?;
 
      let mut pmess = command.opt_into().unwrap();

      self.enc.encode_msg_into(&mut self.stream, shad, async_yield, &mut pmess)?;

      if pmess.get_nb_attachments() > 0 {
        for att in pmess.get_attachments() {
          self.enc.attach_into(&mut self.stream, shad, async_yield, att)?;
        }
      }

      // we need to write end (eg for block cipher, buffers must be flushed)
      shad.write_end(&mut WriteYield(&mut self.stream,async_yield))?;
      shad.flush_into(&mut WriteYield(&mut self.stream,async_yield))?;

 
    }

    Ok(())
  }
}
/// command for readservice
pub enum WriteCommand<MC : MyDHTConf> {
  /// ping TODO chal as Ref<Vec<u8>>
  Ping(Vec<u8>),
  /// pong a peer with challenge and read token last is second challenge if needed (needed when
  /// replying to a ping not a pong)
  Pong(MC::PeerRef, Vec<u8>, usize, Option<Vec<u8>>),
  Service(MCCommand<MC>),
  //GlobalService(MC::GlobalServiceCommand),
}

impl<MC : MyDHTConf> WriteCommand<MC> {
  pub fn get_api_reply(&self) -> Option<ApiQueryId> {
    match *self {
      WriteCommand::Service(ref lsc) => lsc.get_api_reply(),
      _ => None,
    }
  }
}


pub enum WriteCommandSend<MC : MyDHTConf> 
  where MC::LocalServiceCommand : SRef,
        MC::GlobalServiceCommand : SRef,
  {
  /// Vec<u8> being chalenge store in mainloop process
  Ping(Vec<u8>),
  /// pong a peer with challenge and read token
  Pong(PeerRefSend<MC>, Vec<u8>, usize, Option<Vec<u8>>),
  Service(<MCCommand<MC> as SRef>::Send),
  //GlobalService(<MC::GlobalServiceCommand as SRef>::Send),
}

impl<MC : MyDHTConf> Clone for WriteCommand<MC> {
  fn clone(&self) -> Self {
    match *self {
      WriteCommand::Ping(ref chal) => WriteCommand::Ping(chal.clone()),
      WriteCommand::Pong(ref pr,ref v,s,ref v2) => WriteCommand::Pong(pr.clone(),v.clone(),s,v2.clone()),
      WriteCommand::Service(ref p) => WriteCommand::Service(p.clone()),
      //WriteCommand::GlobalService(ref p) => WriteCommand::GlobalService(p.clone()),
    }
  }
}
impl<MC : MyDHTConf> SRef for WriteCommand<MC>
  where MC::LocalServiceCommand : SRef,
        MC::GlobalServiceCommand : SRef,
  {
  type Send = WriteCommandSend<MC>;
  fn get_sendable(self) -> Self::Send {
    match self {
      WriteCommand::Ping(chal) => WriteCommandSend::Ping(chal),
      WriteCommand::Pong(pr,v,s,v2) => WriteCommandSend::Pong(pr.get_sendable(),v,s,v2),
      WriteCommand::Service(p) => WriteCommandSend::Service(p.get_sendable()),
//      WriteCommand::GlobalService(ref p) => WriteCommandSend::GlobalService(p.get_sendable()),
    }
  }
}
impl<MC : MyDHTConf> SToRef<WriteCommand<MC>> for WriteCommandSend<MC>
  where MC::LocalServiceCommand : SRef,
        MC::GlobalServiceCommand : SRef,
  {
  fn to_ref(self) -> WriteCommand<MC> {
    match self {
      WriteCommandSend::Ping(chal) => WriteCommand::Ping(chal),
      WriteCommandSend::Pong(pr,v,s,v2) => WriteCommand::Pong(pr.to_ref(),v,s,v2),
      WriteCommandSend::Service(p) => WriteCommand::Service(p.to_ref()),
      //WriteCommandSend::GlobalService(p) => WriteCommand::GlobalService(p.to_ref()),
    }
  }
}

/// TODO Api is not use : remove it and default to NoSend 
pub enum WriteReply<MC : MyDHTConf> {
  NoRep,
  Api(ApiReply<MC>),
}


