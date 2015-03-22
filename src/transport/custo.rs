// trying to run over udp : requires some info (close to tcp light) :
// - channel id (since we can get multiple connection (server and client) in parallel : got the
// right communicaton -> all message are started by chan id with length
// - message id (since no order in message, we need to allocate after getting a size and then only
// receiving mssg)
//
//
//Simple it is rerouted on receive by unlocking receive mutex and send via udp 


impl TransportStream for (IpAddr, chanid, sendsocket, mutex<buff...>, timeout...) {
fn streamwrite(&mut self, mess : String){
    let bytes = mess.as_bytes();
    let l : usize = mess.len();
    debug! ("sendlengt {:?}", l);
    self.write_le_uint(l); + add message id -> allocate mutex
wait mutex
    self.write(bytes); // in bytes there is an index and bytes -> receive until all ok -> can reask after some time
    // receive confirmation to deallocate otherwhise resend??
}

fn streamread(&mut self) -> IoResult<Vec<u8>>{
    wait on mutex over channel and ipaddr
  self.read_le_uint().and_then(|l|{
  debug!("rec len {:?}", l);
  new msid
  send (okmssage msid s
  wait on mutex with new size
  self.read_exact(l) // TODO more control (in case of long stream) -> here read and complete msg (id + all chunks)
  send confirmation
  })
}

// TODO transform for static dispatch
// badly named : in fact it reroute all receive request (depending on ipaddr and chanid)
fn receiveconnection<C> (p : Node, closure : &mut Fn(Self) -> ()){
    let mut listener = Udpbind::bind (SocketAddr{ip : p.address, port : p.port}).unwrap();
   for receve{
   look for ( chanid and ipaddr){
       // put msg in mutex (hmap on mutex)
       + unlock = receiv
   }
    }
   
   
  spawn( or not there is already a server side spawn)  wait on mutex + timeout this wait -> optional spawn here (we feed the mutex and thats all)
   self.receive ( length mssg id) -> allocate struct and reply with ok mssg
   self .receive (okmessage msg id chan id) -> unloc stream write mutext
    
}    
closure((mutex msg ...))
//                println!("Closing socket exchange : ");
//  println!("  - From {:?}", sp.socket_name());
//  println!("  - With {:?}", sp.peer_name());

 // s.close_read(); // TODO cannot do end operation , add it to trait as a function eg finalize
  // useless?

//  drop(sp);

            }
        }
    }
}

fn connectwith (p : Node, timeout : Duration) -> IoResult<Self>{
 init mutex and all 
return it 
}

}
