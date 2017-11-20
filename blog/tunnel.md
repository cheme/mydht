# Tunnel crate and MyDHT ?


In my previous post I described some aspects of MyDHT crate, basically a library to implement peer2peer application.

In this post I will describe another crates, tunnel; then try to define how it will be used within MyDHT.

[Tunnel](https://github.com/cheme/tunnel) is a crate for experimenting with different kind of multi hop layered ciphering of content (similar to tor, maybe more like a multihop vpn).  
It defines the various traits for operations associated with this problematics : route building, peer proxying, message sending, peer receiving, peer replying.  
It defines various implementations (after a redesign only 'Full' implementation was kept, but it defines multiple mode that should/could be transfer to their own implementation).  
It uses its own cache for routing (not use by all modes), and access a peer collection.  
Yet tunnel does not provide a route (find peers) or send message (transport usage), it only makes it easy to build frame from a peers collection : test cases all ran on a single thread without actual transport usage (just 'Cursor' streams usage).  

Layered cipher of tunnel are 'ExtRead' and 'ExtWrite' implementation that will later be composed, some others limiter 'ExtRead' or 'ExtWrite' are also use to show the ciphered content end. Those abstraction are defined in [readwrite_comp](https://github.com/cheme/readwrite-comp) crate and are the basis of all those tunnel implementation.

Using tunnel within MyDHT will be a good opportunity to test the library over network (with tcp transport), with multiple communication and in a multithreaded context.

Current basic tunnel mode are :
        - one time message
        - one time message with error (current error mode involve caching)
        - one time message with reply by caching proxying info for each peers 
        - one time message with reply by including reply frame in content
        - established route (not yet implemented) using cache

# Tunnel traits

Lets describe shortly tunnel traits and try to find how we can plug them in mydht :

## Peer

There is already an adapter between tunnel peer and mydhtpeer :
```rust
impl<P : MPeer> TPeer for P {
  type Address = P::Address;
  type ShadRead = P::ShadowRAuth;
  type ShadWrite = P::ShadowWAuth;

  fn get_address(&self) -> &Self::Address {
    self.get_address()
  }
  fn new_shadw(&self) -> Self::ShadWrite {
    self.get_shadower_w_auth()
  }
  fn new_shadr(&self) -> Self::ShadRead {
    self.get_shadower_r_auth()
  }
}
```

We can see that MyDHT shadower for authentication : this shadower is basically and asymetric cipher with public key associated with peer identity : for tunnel library it is the kind of cipher that is needed at peer level.  
Tunnel obviously use a symetric scheme for its content, this scheme is defined in the TunnelTrait and not related with the Peer implementation choice (in mydht a second scheme for content with a shared key is associated with peer but not use in this adapter).


## Full implementation trait usage

'Full' tunnel implementation is associated with multiple tunnel traits, they are grouped in a associated trait container :

```rust
pub trait GenTunnelTraits {
  type P : Peer;
  type LW : ExtWrite + Clone; // limiter
  type LR : ExtRead + Clone; // limiter
  type SSW : ExtWrite;// symetric writer
  type SSR : ExtRead;// seems userless (in rpely provider if needed)
  type TC : TunnelCache<(TunnelCachedWriterExtClone<Self::SSW,Self::LW>,<Self::P as Peer>::Address),TunnelCachedReaderExtClone<Self::SSR>>
    + TunnelCacheErr<(ErrorWriter,<Self::P as Peer>::Address), MultipleErrorInfo> + CacheIdProducer;
  type EW : TunnelErrorWriter;
  type RP : RouteProvider<Self::P>;
  type RW : TunnelWriterExt;
  type REP : ReplyProvider<Self::P, MultipleReplyInfo<<Self::P as Peer>::Address>>;
  type SP : SymProvider<Self::SSW,Self::SSR>;
  type TNR : TunnelNoRep<P=Self::P,W=Self::RW>;
  type EP : ErrorProvider<Self::P, MultipleErrorInfo>;
}
```

This could be use and customize internally for our implementation.
It seems that it is not justified anymore and is just an internal implementation detail now.

Originally this subset of associated trait was shared with another alternative implementation (close to 'Full' but without layered cyphering on the full frame : layered only on headers).

We can recognize the required trait implementation for a tunnel, the most relevant ones for mydht interaction should be : peer as shown before, tunnelcache, routeprovider, tunnelnorep and the providers extending tunnelnorep. 


## tunnelcache

```rust
pub trait TunnelCache<SSW,SSR> {
  fn put_symw_tunnel(&mut self, &[u8], SSW) -> Result<()>;
  fn get_symw_tunnel(&mut self, &[u8]) -> Result<&mut SSW>;
  fn has_symw_tunnel(&mut self, k : &[u8]) -> bool {
    self.get_symw_tunnel(k).is_ok()
  }
  fn put_symr_tunnel(&mut self, SSR) -> Result<Vec<u8>>;
  fn get_symr_tunnel(&mut self, &[u8]) -> Result<&mut SSR>;
  fn has_symr_tunnel(&mut self, k : &[u8]) -> bool {
    self.get_symr_tunnel(k).is_ok()
  }
}
```
This is a cache for storing symetric cyper (SSW and SSR) of a tunnel, SSW and SSR are CompExtW and CompExtR containing the state of the symetric cypher. Storage is indexed by a different bytes key (unique) for each peers (those key are generated by proxy peer and added to the proxyied content).
- put_symw get_symw are used for storing ssw when proxying content
- put_sym get_symr are used for writer (first peer) if we are expecting a reply 

Tunnel cache is mainly use to implement 
```rust
impl<TT : GenTunnelTraits> TunnelManager for Full<TT> {
```

Within 'Full' this cache is not use in all case. A mode insert all information for routing (even for a single reply) within the frame and no storage is needed, another mode (suitable for a persistent tunnel case) stores all reply routing info for each peer within this cache.

## TunnelNoRep

The base trait for a tunnel. In this crate we choose a design with a trait hierarchy, this is the most basic trait : a tunnel with no reply or error capability.

```rust
pub trait TunnelNoRep {
  type P : Peer;
  type W : TunnelWriterExt;
  type TR : TunnelReaderNoRep;
  type PW : TunnelWriterExt + TunnelReaderExt<TR=Self::TR>;
  type DR : TunnelReaderExt<TR=Self::TR>;
  fn new_reader (&mut self, &<Self::P as Peer>::Address) -> Self::TR;
  fn init_dest(&mut self, &mut Self::TR) -> Result<()>;
  fn new_writer (&mut self, &Self::P) -> (Self::W, <Self::P as Peer>::Address);
  fn new_writer_with_route (&mut self, &[&Self::P]) -> Self::W;
  fn new_proxy_writer (&mut self, Self::TR) -> Result<(Self::PW, <Self::P as Peer>::Address)>;
  fn new_dest_reader<R : Read> (&mut self, Self::TR, &mut R) -> Result<Self::DR>;
}
```
We see associated type correspondance with previous GenTrait: some peer (us alice), a writer for sending, a reader to check if receiving or proxying, a proxy for proxying and a dest reader for destination reading (dest bob and alice if from a reply with 'Tunnel' trait extension).

Some operation on stream are specific to tunnel :
```rust
pub trait TunnelWriterExt : ExtWrite {
  fn write_dest_info<W : Write>(&mut self, w : &mut W) -> Result<()>;
  fn write_dest_info_before<W : Write>(&mut self, w : &mut W) -> Result<()>;
}

pub trait TunnelReaderExt : ExtRead {
  type TR; 
  fn get_reader(self) -> Self::TR;
}

pub trait TunnelReaderNoRep : ExtRead {
  fn is_dest(&self) -> Option<bool>; 
  fn is_err(&self) -> Option<bool>; 
}

```
Various operation to write content at some points of the frame building timeline.  

ExtWrite and ExtRead are trait for composing over Read and Write with additional method for writing/reading cipher header and writing/reading cipher padding (cipher or limiter), for reference :

```rust
pub trait ExtRead {
  fn read_header<R : Read>(&mut self, &mut R) -> Result<()>;
  fn read_from<R : Read>(&mut self, &mut R, &mut[u8]) -> Result<usize>;
  fn read_end<R : Read>(&mut self, &mut R) -> Result<()>;
 ...
```

## Tunnel

This trait extend the 'TunnelNoRep' by adding reply capability.

```rust
pub trait Tunnel : TunnelNoRep where Self::TR : TunnelReader<RI=Self::RI> {
  type RI : Info;
  type RW : TunnelWriterExt;
  fn new_reply_writer<R : Read> (&mut self, &mut Self::DR, &mut R) -> Result<(Self::RW, <Self::P as Peer>::Address)>;
  fn reply_writer_init<R : Read, W : Write> (&mut self, &mut Self::RW, &mut Self::DR, &mut R, &mut W) -> Result<()>;
}
```
RI is a trait containning information used to build reply, the writer RW is the reply writer (proxy and dest reader for this reply are the same as for the query).  
RW is similar to the writer use to send a query, but it differs enough to use its own trait (query is done with a knowledge of the route to use while replying is done without such knowledge).


## TunnelError

Extension to 'TunnelNoRep' for error return capability.

In tunnel an error is simply a random usize which indicates at what point the failure occured (only alice knows all peers and their error code).
It is similar to reply but with a smaller content to send, which allow a lighter scheme. Also it applies to every peers, not only the dest.

```rust
pub trait TunnelError : TunnelNoRep where Self::TR : TunnelReaderError<EI=Self::EI> {
  type EI : Info;
  type EW : TunnelErrorWriter;
  fn new_error_writer (&mut self, &mut Self::TR) -> Result<(Self::EW, <Self::P as Peer>::Address)>;
  fn proxy_error_writer (&mut self, &mut Self::TR) -> Result<(Self::EW, <Self::P as Peer>::Address)>;
  fn read_error(&mut self, &mut Self::TR) -> Result<usize>;
}
```
Very similar to reply info. EW the error writer is similar to a reply writer but it only replies an id (so not ExtWrite), similarily there is no specific writer and proxy implementation (TunnelNoHop implementation need to manage those case) :

```rust
pub trait TunnelErrorWriter {
  fn write_error<W : Write>(&mut self, &mut W) -> Result<()>;
}
```
No content to write as the error id is contained in the writer. We can see here that some issue will occurs in mydht or any real use case : the error code is taken from the tunnel object (method 'get_current_error_info') and with multiple routing that is really not convenient, EI should be use as parameter if the TunnelError trait but we can keep it as a TODO until actual integration.


## TunnelManager

A Tunnel but with caching capability, 'Full' implementation requires this constraint. If splitting 'Full' implementation into its different mode, the mode with all info contained in its frame will not need it.

```rust
pub trait TunnelManager : Tunnel + CacheIdProducer where Self::RI : RepInfo,
Self::TR : TunnelReader<RI=Self::RI>
{
  type SSCW : ExtWrite;
  type SSCR : ExtRead;
  fn put_symw(&mut self, &[u8], Self::SSCW, <Self::P as Peer>::Address) -> Result<()>;
  fn get_symw(&mut self, &[u8]) -> Result<(Self::SSCW,<Self::P as Peer>::Address)>;
  fn put_symr(&mut self, Self::SSCR) -> Result<Vec<u8>>;
  fn get_symr(&mut self, &[u8]) -> Result<Self::SSCR>;
  fn use_sym_exchange (&Self::RI) -> bool;
  fn new_sym_writer (&mut self, Vec<u8>, Vec<u8>) -> Self::SSCW;
  fn new_dest_sym_reader (&mut self, Vec<Vec<u8>>) -> Self::SSCR;
}
```

A cache for peer is used and and symetric writer can be build from it for proxying plus dest multisim read. Notice the CacheId constraint a trait for producing unique cacheid between peers.


## TunnelManagerError

A tunnel with error return capability and a cache
```rust
pub trait TunnelManagerError : TunnelError + CacheIdProducer where  Self::EI : Info,
Self::TR : TunnelReaderError<EI = Self::EI>
{
  fn put_errw(&mut self, &[u8], Self::EW, <Self::P as Peer>::Address) -> Result<()>;
  fn get_errw(&mut self, &[u8]) -> Result<(Self::EW,<Self::P as Peer>::Address)>;
  fn put_errr(&mut self, &[u8], Vec<Self::EI>) -> Result<()>;
  fn get_errr(&mut self, &[u8]) -> Result<&[Self::EI]>;
}
```
Same as reply for cache.
with
```rust
pub trait TunnelReaderError : TunnelReaderNoRep {
  type EI;
  fn get_current_error_info(&self) -> Option<&Self::EI>;
}
```

## SymProvider

```rust
pub trait SymProvider<SSW,SSR> {
  fn new_sym_key (&mut self) -> Vec<u8>;
  fn new_sym_writer (&mut self, Vec<u8>) -> SSW;
  fn new_sym_reader (&mut self, Vec<u8>) -> SSR;
}
```
Used to create a new symetric reader or writer. In mydht this kind of scheme (symetric cyphering) could already be use post authentication as the message shadower (but its implementation is into 'Peer).  

## RouteProvider


```rust
pub trait RouteProvider<P : Peer> {
  fn new_route (&mut self, &P) -> Vec<&P>;
  fn new_reply_route (&mut self, &P) -> Vec<&P>;
}
```

Trait to initiate tunnel peers choice, it will probably need to be linked somehow with MyDHT peer KVStore.


# tunnel and mydht interaction??

From this point this post will certainly be gibberish, more prelimenary thoughts.

## considering only on time unidirectional message

We have seen that our mydht may contain a TunnelNoRep implementation.
For 'Full', full content is 
```rust
pub struct Full<TT : GenTunnelTraits> {
  pub me : TT::P,
  pub reply_mode : MultipleReplyMode,
  pub error_mode : MultipleErrorMode,
  pub cache : TT::TC,
  pub route_prov : TT::RP,
  pub reply_prov : TT::REP,
  pub sym_prov : TT::SP,
  pub tunrep : TT::TNR,
  pub error_prov : TT::EP,
  pub rng : ThreadRng,
  pub limiter_proto_w : TT::LW,
  pub limiter_proto_r : TT::LR,
  pub reply_once_buf_size : usize,
  pub _p : PhantomData<TT>,
}
```

problematics are :
- me : should be a Ref\<Peer\> (in mydht we use Ref\<P\>)
- cache : a cache can only be at one location so our Tunnel implementation needs to be at a single place
- route_prov : a route provider, same restriction as previous cache plus the fact that it needs to build its route from a source of peers

## usage of PeerRef

simply implement route peer for the peerref of peer : that way even without peerref in tunnel crate we use peerref from. 

Something like thise will be added to the adapter (I tried it in different context and expect it to be impossible in rust, we may simply implement for every Ref)
```rust
impl<P : MPeer,RP : Borrow<P>> TPeer for RP {

```

Edit : some bad ideas, finally I simply globally use a Ref\<Peer\> (the MyDHT associated RP type) as our Tunnel peer. This way tunnel crate does not change and still only manipulate peers but in mydht-tunnel context, those are reference to peer object.

## usage of peers from mydht to build route 

- from mydht mainloop peercache : only connected peers should be use to ensure that the message got a minimal chance to be send. This is higly unsecure as we know which peer connection we have established.
- from mydht default peerstorage : issue of unconnected peer
- from a global service where we exchange reachable peers : a store of peers initiated from connected peercache and communication of peers

## location of the tunnel

* out of mydht implementation, sending query to the global peer manager service, this tunnel is include in a transport.

* a route service communicating with peerstore and mainloop
  * use of mydht global service : so the mydht instance is only here to produce route (with peer exchange and management) and include in transport use by another mydht instance
  * use of a service spawn from transport : a tunnel transport over another transport : bad as peer exchange is done with this transport.

* in the global service : best but means that the mydht is specific to tunnel (an inner service can be use and/or sending receiving from outside), plus in mydht global service can easily send query to peerstore or mainloop (especially using a subset on peerstore : probably need to create a second variant of subset being more random : currently subset is a mix of random and not random depending on implementation , which broke a lot of tests).
        Still having to keep a local cache of connected peer in global service is painfull and racy (only the cache in mainloop is fine). 'RouteProvider' should be merge with the mainloop peer cache implementation, and use a synchronized connected peer cache (threading seems necessary).

## Tunnel as a transport

The first idea is to use tunnel as a transport as we could have implemented a transport between tor hidden services.
* what match with mydht transport

- we send queries with a destination
- connect could simply get the route


* what does not match

- we do not authenticate (mydht allows it)
- origin of a received query is hidden, we only got origin of proxying peer : not such a big issue, just that service implementation must be independant of origin.
- replying without an origin can only be done with frame content by spawning a reply writer with a dest peer which can be different (here we may need to borrow the inner write stream).
- proxying on read content requires to get the handle on a inner write stream (like if tunnel as a transport should include a mydht instance).

This all seems bad, it should be possible, doable with an established tunnel (not curently implemented in tunnel I think) : with an established tunnel the main problem is that origin is unknow -> so we need to run with no authentication mydht mode.
We could run with authentication too (peer transmit in ping is fine in this case). 

## MyDHT as tunnel reader/writer provider

tunnel is currently running over reader and writer : similart to mydht transport streams.
MyDHT could be route provider implementation (all tools are here).

This is true for sending, not for reading, we could use local service for this specific reading code. Therefore we also could put tunnel main struct into global service (single instance) : seems better.

## Conclusion

Tunnel should be into global service (only one cache and communication with possible route provider).  
Sending query is therefore done by global service on a new write stream or by a special message for write stream containing peers for route and inner service query : the message encoder used will call tunnel primitive.  
Proxing and replying will probably needs to be done from local service and by using special message decoder (command being reply or proxy).  

# Next steps 

First step should be to create a mydht-tunnel crate with tunnel used from a mydht global service (which can retrieve route and needed inner transport stream with two modes or one only) :  
  - borrowing stream : to avoid openning to much transport : this would require some specific mainloop implementation (similar to query of mainloop cache on some peerstore query) and may block the inner dht a lot
  - sending new write stream : more realist : this would be done through global service
Read of message would be done in local service (require that local service got access to read stream or to put specific processing in MsgEnc trait)
  - if dest then local service read content and send it to global service
  - if proxy then local service open a new connection (borrowing not for initial implementation)
  - a specific message dec/enc will be use : it will allow to build frame from write stream and to read if proxy or query local command from read stream
Proxying is also done in localservice (localservice need to get a write stream from mainloop) after querying for state (by queryid to get shadower) in global service or by geting stream writer from mainloop (no queryid : in query).
An inner service for replying could be called from global or local depending on the fact that reply is possible by being include in frame (local service) or by using tunnelcache (global service).

Second step would be to implement transport from this implementation (this transport will spawn new route at each message sending : very slow but nice).

This design means that local service must have access to read stream, an alternative would be to let local service send a mainloop new command with its read token and forward this command to the right write service (proxy done in write service), from this point let mainloop close the read service (as if command end was received) and append its stream in the write command (a special trait is needed similar to the one use to make local query in mainloop).  
Something like MainLoopCommand::EndReadProxyWriteWithStream(p::address,option\<write_token\>,localservicecommand).  
Then the write special serializer will have a message containing the readstream and making it possible to inline proxy. That seems better and can apply to reply to (in this case the reply need to be include in the command from localservice). This seems better ordered than waiting on connection and write stream from reader.

Edit : last method was use, with the difference that borrowing of read stream was done in read service (instead of mainloop), and read service switch to a stale state (could theorically be reuse if read stream is send back).  

