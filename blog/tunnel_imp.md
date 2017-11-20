# Implementing MyDHT tunnel

Last post, we gave some thoughts on tunnel and mydht interaction,concluding that a mydht specific implementation specific for tunnel should be build (and in a second time maybe use it in a transport).

That was done for a single mode of 'Full' tunnel : the mode that does not require caching. Cache usage mode and error reporting implementation are still incomplete but shall follow similar steps.

This post will describe some of the change upon tunnel and mydht, mainly needed to run within multithreaded environment with non blocking transport stream.

The new crate is called [mydht-tunnel](https://github.com/cheme/mydht/tree/master/mydht-tunnel).

# tunneling use case

For our use case, we use a variable length tunnel (Full tunnel) in a mode that does not require any caching.

Alice send a query to Bob by transmitting it over n peer (on schema two), each peer find reference (address) to next peer after reading frame header (asymetric ciphering), then each peer proxy content (symetric ciphering) to next peer. Last peer (Bob) can read the query and will reply using a payload that was inserted by Alice. Depending on config, the payload will route in the same route as for sending or in a different route (choosed by Alice).

So all is decided by Alice, other peers will only decipher the frame and proxy or reply.

Basically the frame sent by Alice for two hop with reply tunnel will look like that :

TODO insert schema

Each Encipher layer contains an asymetrically ciphered header (with a symetric key for the content and the next peer address) and symetrically enciphered content.
The content read is the payload to transmit to next peer or the query plus the reply payload for dest (Bob).  
The reply on the schema is obviously not the reply (written by alice, not bob), but the header to use for replying to alice : 

TODO insert schema

Here each layer contains next peer address and a symetric key (for each reply proxy peer), for Bob those two infos are in its query header. The last peer (alice) will not receive a single key for proxying the content but allkeys needed to read it.   
The route choice is made by Alice (same as for query), this implies that to build it our 'Full' tunnel implementation contains another 'Full' tunnel implementation (a variant without reply), leading to an iteresting type as seen in this debug trace (to big to be easily readable).

TODO link to site head image

Bob reply by sending the reply payload containing all symetric keys, plus its actual reply encoded with its own reply symetric key.

TODO insert schema

Each proxy hop will read (similar as query) its header and will proxy the included content of payload. Next, proxy the symetrically enciphered reply content from previous peer by symetrically enciphering it (sym key provided in header). Note that at least two limiter are in use here for first part and second part (a bit more in fact).

At the end alice will receive : 

TODO insert schema

Finally Alice read her header to read back all the reply keys, view its a reply to its a reply to her read all reply keys from header and with those n+1 key can access Bob reply.

We note that alice could have store those keys instead of sending them back to herself; in a same way proxying peer (case with same reply route only) could have store reply key when proxying the query : that is the scheme with caching (no need for the big reply payload) that need a lot of routing unique random ids and is not in my current use case.

In fact its a bit more complex and limiter are used to see the end of frames (it is or will be documented in tunnel crate).

# implementation

## inner service

Tunneling is fine but it took place both in local and global service of this MyDHT instance. Still we probably want to run a service. Their is two possibility :

- with another mydht instance : another mydht is running on another listening address (possibly on another transport), but the same peers are used (with an adapter to switch address). The mydht instance will be use from this other instance to send content. The non tunnel mydht instance will be use to manage peers and should send its update to the other instance tunnel services (tunnel are build from connected peers). It will also be use to run the service specific code.
- by including the service into tunnel service : probably the best solution if we do not want to run two transports, simply put the service to run into GlobalTunnel service and into LocalTunnel service.

So MyDHTTunnel contain an inner service that can run on both local and global tunnel specific services :
```rust
pub trait MyDHTTunnelConf : 'static + Send + Sized {
...
  type InnerCommand : ApiQueriable + PeerStatusListener<Self::PeerRef>
    + Clone
    + Send;
  type InnerReply : ApiQueriable
    + Send;
  type InnerServiceProto : Clone + Send;
  type InnerService : Service<
    CommandIn = GlobalCommand<Self::PeerRef,Self::InnerCommand>,
    CommandOut = GlobalTunnelReply<Self>,
  >
... 
```
Note that currently the same service is use for local and global, it could be necessary to have two separate service.

```rust

pub struct LocalTunnelService<MC : MyDHTTunnelConf> {
...
  pub inner : MC::InnerService,
```
and 
```rust
pub struct GlobalTunnelService<MC : MyDHTTunnelConf> {
  pub inner : MC::InnerService,
``` 

## Tunnel access

Tunnel object (contains cache) is located in global service of MyDHT, but with this use case we do not want to use the global service for every proxy or reply (theorically only required for sending query).

For this purpose we change tunnel Api to create a clonable tunnel partial implementation which could be send to our Read thread.  
This lightweight implementation of tunnel use the same prototypes but with optional value (and in some specific case less parameters).

So we add this lightweight clonable tunnel in our Read service and when receiving a tunnel frame, if the methods to get proxy writer (or reply writer for bob) returns a value we send directly a message to the sender (through mainloop), otherwhise we send a message to global service where we run the method on the main tunnel object (with cache) and then the main tunnel object sends it to the writer (through mainloop).


So a new method is added to TunnelNoRep to instantiate the clonable lightweight tunnel :
```rust
  fn new_tunnel_read_prov (&self) -> Self::ReadProv {
```
The lightweight tunnel is define in two traits (with and without reply) :
```
pub trait TunnelNoRepReadProv<T : TunnelNoRep> {
  fn new_tunnel_read_prov (&self) -> Self;
  fn new_reader (&mut self) -> <T as TunnelNoRep>::TR;
  fn can_dest_reader (&mut self, &<T as TunnelNoRep>::TR) -> bool;
  fn new_dest_reader<R : Read> (&mut self, <T as TunnelNoRep>::TR, &mut R) -> Result<Option<<T as TunnelNoRep>::DR>>;
  fn can_proxy_writer (&mut self, &<T as TunnelNoRep>::TR) -> bool;
  fn new_proxy_writer (&mut self, <T as TunnelNoRep>::TR) -> Result<Option<(<T as TunnelNoRep>::PW, <<T as TunnelNoRep>::P as Peer>::Address)>>;
}
```
 and 
```rust
pub trait TunnelReadProv<T : Tunnel> : TunnelNoRepReadProv<T> where
 <T as TunnelNoRep>::TR : TunnelReader<RI=T::RI>,
 <T as TunnelNoRep>::ReadProv : TunnelReadProv<T>,
 {
  fn reply_writer_init_init (&mut self) -> Result<Option<T::RW_INIT>>;
  fn new_reply_writer<R : Read> (&mut self, &mut T::DR, &mut R) -> Result<(bool,bool,Option<(T::RW, <T::P as Peer>::Address)>)>;
}
```
Note some additional function linc 'can_proxy_writer' that are here to avoid consuming the read stream (otherwhise runing the operation again in global service will fail). Same thing must be taken care of when implementing function like 'new_reply_writer' : if content is read from stream a state must be store in the ExtReader if returning None.

## Special msgenc

Having this new tunnel object for our Read service is fine, but it need to read from stream. That is not possible in local service : local service only have a message as input. The produced tunnel writers also need to be use from WriteService, but their is no call to local service in write.  
Local service does not seem suitable for tunnel read and write operations.

Global service will need to read (see next part on borrowed stream), but it is fine.


The way it is done here is to use the MsgEnc trait to do those operation. MsgEnc is use in both Read and Write service and have access to their stream.  
A special MsgEnc implementation is used to do it (contains an actual MsgEnc for inner service content and the lightweight clonable tunnel) :
```rust
pub struct TunnelWriterReader<MC : MyDHTTunnelConf> {
  pub inner_enc : MC::MsgEnc,
    ...
  pub t_readprov : FullReadProv<TunnelTraits<MC>>,
}
```

On decoding (read service) it will read header of frame then inner content with its inner encoder if the peer is dest(Bob or Alice on reply).
On encoding (write service) it will either run proxy operation or reply with inner reply to encode.

## Borrowing read stream


Among the tunnel operations two are really interesting, the proxy that forward its inner content and the dest (bob) that reply by forwarding the reply payload.

The interesting point is that in both case we read info and proxy it.

Reading header in memory is fine (next peer address, symetric key...). Putting the reply payload or content to forward in memory is not. By putting the payload to proxy in memory all become totally unscallable (message can be pretty huge with long tunnel plus query and reply can contains any size of content). With our current design we need to store the content to proxy on a file (in memory is not an option) in read service then read from this file in write service to proxy : totally uncall for.

The only way to proxy those variable length random (ciphered) bytes is by having both stream in the same process (only a read buffer in memory).  
All tunnel crate code is based upon this bufferized approach (by composing 'readwrite_comp' crates ExtWrite and ExtRead implementation).

```rust
TODO readext writeext
```

also interesting is MultiExt to keep implementation efficient (and avoid the defect of too much composition) 

```rust
TODO multireadext and multiwriteext
```

So with mydht running read and write in different service the only way to use correctly tunnel is to move the readstream (tcp socket in our test) into the forward write service (another peer tcp socket in our test). We could also move the write stream into the read stream but it is wrong as write stream to use is define by reading read stream and selected by address in mainloop.

So if we borrow readstream proxy is easy
```rust
        let mut readbuf = vec![0;MC::PROXY_BUF_SIZE];
        let mut y2 = y.opt_clone().unwrap();
        let mut ry = ReadYield(rs,y);
        let mut reader = CompExtRInner(&mut ry,rshad);
        proxy.read_header(&mut reader)?;

        let mut wy = WriteYield(w,&mut y2);
        let mut w = CompExtWInner(&mut wy,wshad);
        proxy.write_header(&mut w)?;
        // unknown length
        let mut ix;
        while  {
          let l = proxy.read_from(&mut reader, &mut readbuf)?;
          ix = 0;
          while ix < l {
            let nb = proxy.write_into(&mut w, &mut readbuf[..l])?;
            ix += nb;
          }
          l > 0
        } {}
        proxy.read_end(&mut reader)?;
        proxy.write_end(&mut w)?;
        proxy.flush_into(&mut w)?;
 
```
Note that we could use 'write_all_into' for shorter/cleanest code (written this way for debugging purpose).

## readstream in message

ReadStream and WriteStream are 'Send' (they were put in mpsc messages in my previous mydht design).

So we borrow the readstream by putting it in the message that will be send to the write service.

```rust
          let is_bor = mess.is_borrow_read();
        ... 
          if is_bor {
            // put read in msg plus slab ix
            let shad = replace(&mut self.shad_msg,None).unwrap();
            let stream = replace(&mut self.stream,None).unwrap();
            mess.put_read(stream,shad,self.token,&mut self.enc);
          }
```

We can see that we not only borrow the read stream but also the shadow use for reading (MyDHT allow definition of Shadow/Ciphering between peers). We also pass a reference to MsgEnc allowing us to take some internal state object (the current dest reader in some config)

This is done into read service by using a specific trait on message :
```rust
pub trait ReaderBorrowable<MC : MyDHTConf> {
  #[inline]
  fn is_borrow_read(&self) -> bool { false }
  #[inline]
  fn is_borrow_read_end(&self) -> bool { true }
  #[inline]
  fn put_read(&mut self, _read : <MC::Transport as Transport>::ReadStream, _shad : <MC::Peer as Peer>::ShadowRMsg, _token : usize, &mut MC::MsgEnc) {
  }
}
```
The trait must be implemented by local service command.


After borrow, the read service is in a borrow state (actually we also should send a end message to mainloop) that do nothing until it get its stream back (input channel message buffer size limit is require here).


## Sending Peer to tunnel cache

Connected peers stored in global service need to be synchronized with mainloop connected peer cache.
This is achieved by the new trait 'PeerStatusListener'.


```rust
impl<MC : MyDHTTunnelConf> PeerStatusListener<MC::PeerRef> for GlobalTunnelCommand<MC> {
  const DO_LISTEN : bool = true;
  fn build_command(command : PeerStatusCommand<MC::PeerRef>) -> Self {
    match command {
      PeerStatusCommand::PeerOnline(rp,_) => GlobalTunnelCommand::NewOnline(rp),
      PeerStatusCommand::PeerOffline(rp,_) => GlobalTunnelCommand::Offline(rp),
    }
  }
}
```

The global service command if implementing this trait will receive new peers on connection (hardcoded in MyDHT mainloop), that is the purpose of the above implementation.

This seems a little hacky, and may be change (still may be use by other GlobalService case).


## handling asynch

At this point all was fine : tunnel operation in MsgEnc, a inner service, a inner msgenc, even borrowing read.

Yet our test only ran fine for maximum 8 peers tunnels (6 proxy only). The problem was that I forget that borrowed read stream is a non blocking transport implementation (non blocking tcp socket) that with big message (8 peers) will return a 'WouldBlock' error. In previous blog we have seen that non blocking transport are managed by using ReadYield or WriteYield composer to suspend service on such errors, problem those composer use an '&mut asyncYield' to suspend but this object is not clonable.

Having the need to access '&mut asynch_yield' from two stream at the same time is bad.
As a pragmatic way of doing it an opt_clone method was added to 'SpawnerYield' trait. 

First idea, passing yield in message : not easy, main issue is that 'Service' trait is not associated to 'SpawnYield' (parameter type of 'Service' inner 'call' method) and me would need to define it as a trait object in the message (Read service do not know the type of this SpawnYield even if we know it will be call with the one define in MyDHTConf).  

Second more successful idea, passing yield in 'MsgEnc', as seen before 'MsgEnc' is used for new means (not exclusively encoding) and passing yield as parameter makes the interface even more MyDHT specific, yet it let us use our 'SpawnerYield'.

Next we need to choose : 
  - make 'spawneryield' cloneable. Currently there is '&'a mut Coroutine' implementation that does not allow it.
  - remove opt_clone and use yield in only Write or Read at the same time, doable in most of code except 'reply_writer_init' function where for 'Full' tunnel a copy is done of the reply payload : we need to change this interface and simply add the possibility to copy bytes up to the end of proxy (will be done manually like in proxying). This could be part of a good redesign of tunnel to use a generic state machine for encoding/decoding (currently having trait function called at specific points shows its limit).


# A better design

Another possibility would have been to run proxy and reply (the copy of the reply payload) directly from the Tunnel Local service. Meaning that read stream is still borrowed in message (but only to be use in local service) and that write stream is send from mainloop to local service (through read service) instead of spawning a write service.  
What is the gain ? We avoid spawning a Write service in those two case (pretty big actually).
What does it involves ?
- mainloop on connect will send its write stream to read (very specific)
- special msgenc could be simplier as async_yield reference usage will be deported to local service
- yielding on those operation of local service involve specific mainloop code (either send a message to read service that will yield local service or directly yield local service if its spawner allows it (not our use case because we read only one message so local service should not spawn a thread and be local (no indirect yield for local))).

All in all, and having already done previous implementation, this design is a bit more impactant for mydht base code and I choose not to follow it yet. 
To lower write service spawn impact, a thread pool service spawner should be use. Thinking back, the idea to run proxy in local service got the little overhead that unyield is done through sending a message to read service 

To sumup (and maybe make this text more intelligible) service usage :
- write service : probably better in its own thread (cpu pool usage is nice as we 'SendOnce'), can still run in a coroutine service if using a non blocking transport.
- global service : depends on its innerservice, out of the inner service it is mostly a cache (no reading or writing operation was a implementation choice (required some change on tunnel api on some future change for other tunnel mode)) and could run on the mainloop thread.
- read service : only reading operation (not as heavy as write), so depends on inner service in localservice : if innerservice is costy we should probably put read in its own thread. For brocking transport it must run in its own thread anyway.
- Local service (spawn from read service) : because read service is running mostly once, their is no reason to spawn local service in its own thread (it is read service that should run in its own thread). With the alternative design we could still choose to run read service in the same  thread as mainloop and to put local service in a new thread : that way unyielding from mainloop could directly address the readservice (with a clonable spawn unyield that need to be transmit to mainloop) making it potentially a bit better (if reading message is small and yielding requring : seems contradictory but it could not be depending on transport).

Thinking again, the message passing on Unyield for this 'better design' is really bad, due to the way unyield was define : a costless function that could be call on wrong service state numerous time (and when debugging it is) without big impact. So this better design involves also the compromise of local read service and threaded local service or to change local service to be a function call instead of a service spawned from read service (could be ok in a way).


# Transport Connection Issue

Connection management does not fit the initial MyDHT use case.

First idea was to use standard mydht connection, one sending and one receiving with each connected peers. Problem is that sending content to peer depends only on a random routing choice and we may route a lot through a single peer (communication to many peers having first peer of route being the same peer). The way proxying or sending is done we need to send all content before switching to another message. But some content may be send for minutes or more (full streamed messaging with possible file attachment), so when proxying content to a peer for whom we already proxy another content we are totally blocking it : that is bad.

The easiest way to circumvent this issue is simply to use a new transport stream for each proxy message :
Single usage connection are used (new 'SendOnce' mode). That is really bad as **observing connection and deconnection in the network will directly reveals the routes used** (with a global network connecting observation) used. So next step should be to reuse connection by managing an open connection pool, this is not as easy (problematic of choosing between waiting for connection to be available or opening a new connection and when to release aconnection). Another idea is to multiplex at transport level (transport keeping connection openned), that may be simplier but a bit to specific.  

The single message per connection choice is also a lot easier for implementation as a miss of write_end or read_end from ExtRead or ExtWrite is less impacting.

Also, tests were done with NoAuth mode, with auth the single use of connection is a significant overhead (huge).


# Frame size issue

With this tunnel scheme we got a good idea of the place of the peer in tunnel by looking at frames size : on query it gets shorter for each proxying and on reply it gets fatter for each proxying. This is an issue need to be fix at tunnel level (in tunnel lib), some random filler may be added to the tunnel protocol (if not already the case).


# Conclusion

Use of tunnel has been only plugged for Full tunnel with reply in frame without error handling (I want to use it in a POC voting protocol). Our test case 'test_ping_pong_mult_hop' can be use for variable length tunnel (test with more than a hundred hops is fine but test case is plugged on 25).  

This test case was very usefull, and an initial issue was me forgetting to use 'ReadYield' on borrowed stream. That leads to a fun debugging session with this test case on localhost running only up to a 8 peer tunnel (6 proxy peer only) and hangging silently otherwhise. Usage of the 'SpawnerYield' like describe before was not as smooth as I thought it will be (some api changes looks bad).  

Some major issue makes the implementation highly insecure (connection management need to be uncoupled from single message transmission).

For other 'Full' tunnel modes (global service cache containing info for routing), the implementation looks doable (some of the service message routing to global service cache is already in place (initialy reply was send to global service before realizing it was useless)).

Yet we will have a problem : the reply dest is currently cached in tunnel by reading its address from its reader (stream), with MyDHT transport trait it is bad because we cannot say that we will reply to the address use by the connected stream (port in tcp socket differs from listening one).  
Two possibilities here :
- put each peer reply address in the tunnel frame, next to the error id or the previous peer cache id (second one seems better because it could be added by ecah proxy peer)
- change mydht trait to give access to the listening address from the read stream : bad that is the purpose of the MyDHT authentication (update of peer info). So simply use this scheme only with a MyDHT authentication and put the connected peer reference in the message sent to global service.


------------------------


# test case

# fun debug/test case

The test initially ran on 8 peer but not on more and sometime hang on 8 peer.

I suspect the read unyield when borrowed to write service (I had spot a racy situation and the need to unyield when switching polling from read to write (I finally use reregister of readstream on the write slab ix but not sure if I did call unyield on write to avoid this racy possibility)).

Note that trace is bad, I intend to use slog latter and I currently use nothing in mydht-tunnel (initial implementation focus on one tunnel type to run a future vote protocol POC).

## 8 peer ok with yield

8 peer but 6 proxy peer.
```
running 1 test
new route : 8
start a proxying
end a proxying
start a proxying
end a proxying
start a proxying
end a proxying
start a proxying
end a proxying
start a proxying
end a proxying
start a proxying
end a proxying
Replying!
start a proxying
start a proxying
end a proxying
start a proxying
end a proxying
end a proxying
start a proxying
start a proxying
end a proxying
start a proxying
end a proxying
end a proxying
test test::test_ping_pong_mult_hop ... ok
```

trace clean with only proxy
```
running 1 test
new route : 8
read reregister
aread unyield
read reregister
start a proxying
end a proxying
aread unyield
read reregister
start a proxying
aread unyield
end a proxying
aread unyield
read reregister
start a proxying
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
end a proxying
read reregister
start a proxying
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
end a proxying
read reregister
start a proxying
aread unyield
aread unyield
end a proxying
aread unyield
read reregister
start a proxying
aread unyield
aread unyield
aread unyield
end a proxying
read reregister
**awrite unyield**
Replying!
**awrite unyield**
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
read reregister
start a proxying
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
read reregister
start a proxying
end a proxying
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
read reregister
start a proxying
end a proxying
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
read reregister
end a proxying
start a proxying
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
read reregister
start a proxying
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
end a proxying
read reregister
start a proxying
aread unyield
aread unyield
aread unyield
end a proxying
aread unyield
aread unyield
end a proxying
aread unyield
test test::test_ping_pong_mult_hop ... ok
```
trace with yield info (streams and service unyield)
No read/write yield in this case (info is likely send all at once)

## 18 peer ok with yield

```
running 1 test
new route : 18
start a proxying
start a proxying
start a proxying
start a proxying
start a proxying
start a proxying
start a proxying
start a proxying
start a proxying
start a proxying
start a proxying
start a proxying
start a proxying
start a proxying
start a proxying
start a proxying
Replying!
start a proxying
start a proxying
start a proxying
start a proxying
start a proxying
^C


```
```
running 1 test
new route : 18
read reregister
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
read reregister
start a proxying
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
read reregister
start a proxying
aread unyield
aread unyield
read reregister
start a proxying
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
read reregister
start a proxying
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
read reregister
start a proxying
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
read reregister
start a proxying
aread unyield
aread unyield
aread unyield
read reregister
start a proxying
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
read reregister
start a proxying
aread unyield
aread unyield
aread unyield
read reregister
start a proxying
aread unyield
aread unyield
aread unyield
read reregister
start a proxying
aread unyield
aread unyield
aread unyield
aread unyield
read reregister
start a proxying
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
read reregister
start a proxying
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
read reregister
start a proxying
aread unyield
aread unyield
aread unyield
aread unyield
read reregister
start a proxying
aread unyield
aread unyield
aread unyield
aread unyield
read reregister
start a proxying
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
read reregister
start a proxying
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
read reregister
**awrite unyield**
Replying!
**awrite unyield**
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
read reregister
start a proxying
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
read reregister
start a proxying
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
aread unyield
read reregister
start a proxying
aread unyield
aread unyield
read reregister
start a proxying
aread unyield
aread unyield
aread unyield
read reregister
start a proxying
aread unyield
**a read yield**
**a read yield**
aread unyield
^C

```

Confirming read yield issue : 

  type ReadSpawn = ThreadBlock;
  //type ReadSpawn = ThreadPark;

-> yield do not suspend thread in this case : it still block : a write issue instead of a read one (yield but it is fine) !!! -> no yield in some case : wrong issue!!
eg 
```
running 1 test
awrite unyield during connect 1
awrite unyield during connect 1
awrite unyield during connect 1
awrite unyield during connect 1
awrite unyield during connect 1
awrite unyield during connect 1
awrite unyield during connect 1
awrite unyield during connect 1
awrite unyield during connect 1
awrite unyield during connect 1
awrite unyield during connect 1
awrite unyield during connect 1
awrite unyield during connect 1
awrite unyield during connect 1
awrite unyield during connect 1
awrite unyield during connect 0
awrite unyield during connect 2
awrite unyield during connect 4
awrite unyield during connect 6
awrite unyield during connect 8
awrite unyield during connect 10
awrite unyield during connect 12
awrite unyield during connect 14
awrite unyield during connect 16
awrite unyield during connect 18
awrite unyield during connect 20
awrite unyield during connect 22
awrite unyield during connect 24
awrite unyield during connect 26
awrite unyield during connect 28
awrite unyield during connect 30
awrite unyield during connect 32
awrite unyield during connect 1
new route : 18
read reregister : 34
awrite unyield during connect 34
awrite unyield during connect 3
awrite unyield during connect 1
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
read reregister : 4
awrite unyield during connect 4
awrite unyield during connect 3
start a proxying
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
awrite unyield during connect 3
read reregister : 4
awrite unyield during connect 3
awrite unyield during connect 4
start a proxying
aread unyield 2
aread unyield 2
aread unyield 2
read reregister : 4
awrite unyield during connect 3
awrite unyield during connect 4
start a proxying
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
awrite unyield during connect 3
read reregister : 4
awrite unyield during connect 3
awrite unyield during connect 4
start a proxying
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
read reregister : 4
awrite unyield during connect 3
awrite unyield during connect 4
start a proxying
aread unyield 2
aread unyield 2
aread unyield 2
awrite unyield during connect 3
read reregister : 4
awrite unyield during connect 3
awrite unyield during connect 4
start a proxying
aread unyield 2
aread unyield 2
-> a read yield
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
read reregister : 4
awrite unyield during connect 3
awrite unyield during connect 4
start a proxying
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
awrite unyield during connect 3
read reregister : 4
awrite unyield during connect 3
awrite unyield during connect 4
start a proxying
aread unyield 2
aread unyield 2
aread unyield 2
read reregister : 4
awrite unyield during connect 3
awrite unyield during connect 4
start a proxying
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
awrite unyield during connect 3
read reregister : 4
awrite unyield during connect 3
awrite unyield during connect 4
start a proxying
aread unyield 2
aread unyield 2
aread unyield 2
read reregister : 4
awrite unyield during connect 3
awrite unyield during connect 4
start a proxying
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
awrite unyield during connect 3
read reregister : 4
awrite unyield during connect 3
awrite unyield during connect 4
start a proxying
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
read reregister : 4
awrite unyield during connect 3
awrite unyield during connect 4
start a proxying
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
awrite unyield during connect 3
read reregister : 4
awrite unyield during connect 3
awrite unyield during connect 4
start a proxying
aread unyield 2
aread unyield 2
aread unyield 2
read reregister : 4
awrite unyield during connect 3
awrite unyield during connect 4
start a proxying
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
awrite unyield during connect 3
read reregister : 4
awrite unyield during connect 6
awrite unyield during connect 4
start a proxying
aread unyield 5
aread unyield 5
aread unyield 5
read reregister : 7
awrite unyield during connect 6
awrite unyield during connect 7
awrite unyield 7
Replying!
awrite unyield 7
aread unyield 5
aread unyield 5
aread unyield 5
aread unyield 5
aread unyield 5
aread unyield 5
aread unyield 5
aread unyield 5
aread unyield 5
awrite unyield during connect 6
read reregister : 7
awrite unyield during connect 9
awrite unyield during connect 7
start a proxying
aread unyield 8
read reregister : 10
awrite unyield during connect 6
awrite unyield during connect 10
start a proxying
aread unyield 5
awrite unyield during connect 6
aread unyield 2
^C

```

a good one, goes up to reply with some write unyield which could indicate reregister behave correctly but it seems occurs on reply so not that sure.

Note that proxy even in query never end, the fact is that we use a inneficient limiter for many hop and big frame (nice one for testing because it adds a lot (increasing over length) of end frame content).

It is nice to notice that reply can occurs before all query frame is read in fact proxy uses this read frame so it is normal.

At this point the only thing sure is that the issue is with read service yield during proxying (only significative difference between both test cases). Note than in other case the yield can occurs way before (one only sometime a lot but never write unyield except for reply!).


Similar trace with slab ix :
```
read reregister : 4
start a proxying
aread unyield 3
aread unyield 3
aread unyield 3
read reregister : 5
awrite unyield 5
Replying!
awrite unyield 5
aread unyield 5
aread unyield 5
aread unyield 5
aread unyield 5
aread unyield 5
aread unyield 5
aread unyield 5
read reregister : 7
start a proxying
aread unyield 5
aread unyield 5
aread unyield 5
aread unyield 5
read reregister : 7
start a proxying
aread unyield 5
aread unyield 5
aread unyield 5
read reregister : 7
start a proxying
aread unyield 6
-> a read yield
-> a read yield
aread unyield 2
```
we see some unyield 5 be fore reregister 7 (+2 indicate next connect so may be on same peer), then readyield and no awrite unyield occurs : if correct the yield is while proxying.
We also see that after reregister we never unyield on the new slab index.

Note that on reply it seems to behave :
```
aread unyield 3
read reregister : 5
awrite unyield 5
Replying!
awrite unyield 5
```

What is different between write and proxy ? write reply may be slower after reregister and no race, it is a bit different but not significatively (need to init from read). 
The major difference is that for reply, the reply writer use a value from global service for init, so it is slower (message send from local to global then to mainloop then to write service : in fact it is a design mistake as we do not require to use global in this case (TODO have 'reply_writer_init_init' optionally accessible from 'TunnelNoRepReadProv' and route directly to service if result (keep routing for case no result)). At first glance it should not impact, yet it delays the sending to write (not the reregister), this is odd when fix retesting could be an idea...
Guess it is a nb_hop issue : with more hop : it yields block before reply so might be independant. With 2 hop, it also write unyield : 

with check of unyield (not done) when in under connection state :
```

read reregister : 7
awrite unyield during connect 6
awrite unyield during connect 7
awrite unyield 7
Replying!
aread unyield 5
...
awrite unyield 7

```
```
read reregister : 7
awrite unyield during connect 7
awrite unyield during connect 6
start a proxying
-> a read yield
```
so the reregister seems fine, just no event after yield on read
The only fact is that it does not yield while replying.



# ok it is not a yield issue

using threadblock (yield simply redo the command) convince me.

lets see if proxy does proxy the rights lenth (at each hop it should be shorter frame by a constant (not totally) proportion : no filler I hope)).
Indeed there is a constant 80 bit between each proxying (no variance since limiter not counted), and it is stuck at :
```
        println!("end a proxying{:?}", total);
        proxy.read_end(&mut reader)?;
        println!("end a proxying2");
        proxy.write_end(w)?;
        println!("end a proxying3");
        proxy.flush_into(w)?;
        println!("end a proxying");
 
```


```
running 1 test
new route : 18
start a proxying
start a proxying
do not w 0 lenth l!!!
end a proxying802
start a proxying
start a proxying
do not w 0 lenth l!!!
end a proxying722
start a proxying
start a proxying
do not w 0 lenth l!!!
end a proxying642
start a proxying
start a proxying
do not w 0 lenth l!!!
end a proxying562
start a proxying
start a proxying
do not w 0 lenth l!!!
end a proxying482
start a proxying
start a proxying
do not w 0 lenth l!!!
end a proxying402
start a proxying
start a proxying
do not w 0 lenth l!!!
end a proxying322
start a proxying
start a proxying
do not w 0 lenth l!!!
end a proxying242
Replying!
start a proxying
start a proxying
test test::test_ping_pong_mult_hop has been running for over 60 seconds

```
So we ar stuck in read_end, seeing that none has go to write end this is obvious : issue is simply that initial sender do not send its end message due to message to long (issue with limiter) ??

Switching proxy.write_end with proxy.read_end solve it!! but only in some case, while resulting in errors in other cases (addr parse errors or other deserialize issues) or blocked situation  -> globally it is worse and when it pass it is with a last 'end a proxying13496' which is huge
So proxy should defintely read its end state before writing his (do not remember why TODO if blog find reason in code)

Ok so an error was silently skipped since I do not have service error listening in mydht : 

  !!!error ending serviceError("End read missing padding", IOError, Some(Error { repr: Custom(Custom { kind: Other, error: StringError("End read missing padding") }) }))

  which is what I have seen with gdb the limiter need 128 byte but when calling read it got 0 byte and it happen for the first proxy read end.
  debugging with less peers (when fine), we got winrem at 0 (not 128) and therefore it run fine.
  hypothesis is that with many hop winrem should be 0 too

  issue seems to be related to thread/communication related, as it never happen in the same hop (not a tunnel issue but a mydht-tunnel one). and sometime we succeed

in success case there is no padding when proxying : TODO check symetry of our tunnels : why do we need padding in the reader?


start when ok 
```
awrite unyield during connect 1
awrite unyield during connect 1
new route : 18
awrite unyield during connect 0
awrite unyield during connect 2
awrite unyield during connect 4
awrite unyield during connect 6
awrite unyield during connect 8
awrite unyield during connect 10
awrite unyield during connect 12
awrite unyield during connect 1
awrite unyield during connect 14
awrite unyield during connect 16
awrite unyield during connect 18
awrite unyield during connect 20
awrite unyield during connect 22
awrite unyield during connect 24
awrite unyield during connect 26
awrite unyield during connect 28
awrite unyield during connect 30
awrite unyield during connect 32
      !!!error ending serviceError("connection refused", IOError, Some(Error { repr: Custom(Custom { kind: Other, error: IoError(Error { repr: Os { code: 111, message: "Connection refused" } }) }) }))
read reregister : 34
awrite unyield during connect 3
awrite unyield during connect 34
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
aread unyield 2
```
or
```
 a proxying
      !!!error ending serviceError("End as borrow is not expected to be back", EndService, None)
read reregister : 4
awrite unyield during connect 3
awrite unyield during connect 4

```
notice the awrite unyield of read reregister each time (first after a connect error which move the ix to 34 (it pools 16 peers * 2).
start when ko 
```
```

after checking tunnel code (very simple in this case), the remaining on read end should be 0 as we proxyied up to the limiter end normally (single limiter) -> meaning that we did not proxy all and proxy ended before the limiter returning Ok(0) : some read might have returned Ok(0) as suspected before


for instance
end a proxying784
winrem:250

end a proxying1105
winrem:478

end a proxying1327
winrem:256

winrem + end proxying != total, could be lot of others winrem after
end a proxying2406 for ok
end a proxying2406


write unyield during connect 4
bef tunwe write end
aft tunwe write end
start a proxying
reda 24
reda 6


Fuck found, borrowed read stream do not use ReadYield (ReadYield should globaly encapsulate read stream in mydht : that is not the case, it is used when needed because it required a yield reference : so we need this yield reference in our msgenc too). -> not that simple : need ref to asynch yield in msgenc of write
## why is it funny

funny because it illustrates the way the tunnel streams content from end to end : the way proxy is done:
when stuck all is stuck in proxying (no end proxying for any peer) even if it is stuck at 8th or more peer.
