
use mydht_base::transport::{
  Registerable,
  Ready,
  Poll,
  Events,
};
use mydht_base::service::{
  Spawner,
  SpawnerYield,
  RestartOrError,
  Service,
  ServiceRestartable,
  SpawnChannel,
  SpawnSend,
  SpawnRecv,
  SpawnUnyield,
  SpawnHandle,
  LocalRcChannel,
  MioSend,
  MioRecv,
};
use mydht_base::mydhtresult::{
  Result,
};
use super::{
  UserPoll,
  UserEvents,
  UserEventable,
  poll_reg,
};



#[test]
fn test_dummy () {
  assert!(true);
}

/// test read registration
#[test]
fn two_services_talking () {
  // user poll
  let mut poll = UserPoll::new();

  // mesg -> talk1 -> talk2 -> assert in loop
  let mut spawner = RestartOrError;
  let (inp_s, inp_r) = LocalRcChannel.new().unwrap();

  let (sr,reg) = poll_reg();
  let mut inp_s = MioSend {
    mpsc : inp_s,
    set_ready : sr,
  };
  let tok1 = 1;
  assert_eq!(true, reg.register(&poll, tok1.clone(),Ready::Readable).unwrap());
  // TODO transform miochannel to do this code (create a trait behind poll_reg for it : trait would
  // replace poll_reg fn in mydhtcnof)
  let inp_r = MioRecv {
    mpsc : inp_r,
    reg : reg,
  };

  let (bet_s, bet_r) = LocalRcChannel.new().unwrap();
  let (sr,reg) = poll_reg();
  let bet_s = MioSend {
    mpsc : bet_s,
    set_ready : sr,
  };
  let tok2 = 2;
  let bet_r = MioRecv {
    mpsc : bet_r,
    reg : reg,
  };
  assert_eq!(true, bet_r.register(&poll, tok2.clone(),Ready::Readable).unwrap());



  let (out_s, out_r) = LocalRcChannel.new().unwrap();
  let (sr,reg) = poll_reg();
  let out_s = MioSend {
    mpsc : out_s,
    set_ready : sr,
  };
  let tokassert = 3;
  let mut out_r = MioRecv {
    mpsc : out_r,
    reg : reg,
  };
  assert_eq!(true, out_r.register(&poll, tokassert.clone(),Ready::Readable).unwrap());
 
  let mut events = UserEvents::with_capacity(2);
  let events = &mut events;
  let talk1 = Talk(0,8);
  let talk2 = Talk(0,4);
  let mut handle1 = spawner.spawn(talk1,bet_s,None,inp_r,0).unwrap();
  let mut handle2 = spawner.spawn(talk2,out_s,None,bet_r,0).unwrap();

  inp_s.send(1).unwrap();
  inp_s.send(2).unwrap();
  inp_s.send(3).unwrap();
 
  let mut assert_state = 0;
  let assert_val = [1,2,0];
  loop {
    poll.poll(events, None).unwrap();
    while let Some(event) = events.next() {
      match event.token {
        tok1 => {
          handle1.unyield().unwrap();
        },
        tok2 => {
          handle2.unyield().unwrap();
        },
        tokassert => {
          while let Some(v) = out_r.recv().unwrap() {
            assert_eq!(v, assert_val[assert_state]);
            assert_state += 1;
          }
          if assert_state >= assert_val.len() {
            return;
          }
        },
      }
    }
  }
}

#[test]
fn talk_service () {
  let talk = Talk(0,4);
  let mut spawner = RestartOrError;
  let (mut inp_s, inp_r) = LocalRcChannel.new().unwrap();
  let (out_s, mut out_r) = LocalRcChannel.new().unwrap();
  let mut handle = spawner.spawn(talk,out_s,None,inp_r,0).unwrap();
  inp_s.send(1).unwrap();
  handle.unyield().unwrap();
  inp_s.send(2).unwrap();
  inp_s.send(3).unwrap();
  handle.unyield().unwrap();
  assert_eq!(out_r.recv().unwrap(), Some(1));
  assert_eq!(out_r.recv().unwrap(), Some(2));
  assert_eq!(out_r.recv().unwrap(), Some(0));
  assert_eq!(out_r.recv().unwrap(), None);

}
/// add one on message quit at a treshold
pub struct Talk(usize,usize);
impl ServiceRestartable for Talk { }
/// No yield in service, event loop will be tested by using MioRecv or MioSend as input or output
/// channel
impl Service for Talk {
  /// an increment
  type CommandIn = usize;
  /// same increment or 0 if treshold reach
  type CommandOut = usize;

  fn call<S : SpawnerYield>(&mut self, req: Self::CommandIn, _async_yield : &mut S) -> Result<Self::CommandOut> {
    self.0 += req;
    if self.0 >= self.1 {
      println!("s0 ({},{})",self.0,self.1);
      Ok(0)
    } else {
      println!("s{:?}",req);
      println!("state{:?}",self.0);
      Ok(req)
    }
  }
}
