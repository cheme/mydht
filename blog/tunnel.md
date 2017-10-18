# Using tunnel crate with mydht Mydht was the subject of my previous post, a library to implement peer2peer application.  
Tunnel is a crate for experimenting with different kind of multi hop enciphering of content (similar to tor or system like that).
Yet tunnel does not give a way to route and send message, it only makes it easy to build frame from a peers collection.
Current basic tunnel mode are :
        - one time message
        - one time message with error
        - one time message with reply by caching proxying at each peers 
        - one time message with reply by including reply frame in content
        - established route (may not be implemented yet) using cache

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

The main point is that auth shadower is use : maybe a new shadower should be define for mydhtpeer as it is fine with a mydht public authenticated scheme only.
The point is that shadow scheme for tunnel peer must be an asymetric scheme (internal symetric scheme can be define in TunnelTrait).

Lets describe shortly tunnel traits and try to find how we can plug them in mydht :

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
That is the required trait implementation for a tunnel the relevant ones for mydht interaction are mainly peer as shown before, tunnelcache, routeprovider, tunnelnorep and the provider to extend tunnelnorep. 


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
This is a cache use to store symetric cyper (SSW and SSR) for tunnel, SSW and SSR are CompExtW and CompExtR containing the state of the symetric cypher. Storage is indexed by a bytes key unique between two peers.
- put_symw get_symw are used for storing ssw when proxying content
- put_sym get_symr are used for writer (first peer) if we are expecting a reply 

Tunnel cache is mainly use to implement 
```rust
impl<TT : GenTunnelTraits> TunnelManager for Full<TT> {
```

Full being a tunnel scheme (the only implemented at the time) that implements all the tunnel type, and therefore the target for usage in mydht.

## TunnelNoRep

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
We see associated type correspondance with previous GenTrait, shortly some peer, a writer for sending, a reader to check if receiving or proxying, a proxy for proxying and a dest reader for destination reading.

with :
```rust
pub trait TunnelWriterExt : ExtWrite {
  fn write_dest_info<W : Write>(&mut self, w : &mut W) -> Result<()>;
  fn write_dest_info_before<W : Write>(&mut self, w : &mut W) -> Result<()>;
}

pub trait TunnelReaderExt : ExtRead {
  type TR; 
  fn get_reader(self) -> Self::TR;
}

ExtWrite and ExtRead being trait for composing over Read and Write with additional method for writing/reading cipher header and writing/reading cipher padding (cipher or limiter).

```rust
pub trait ExtRead {
  fn read_header<R : Read>(&mut self, &mut R) -> Result<()>;
  fn read_from<R : Read>(&mut self, &mut R, &mut[u8]) -> Result<usize>;
  fn read_end<R : Read>(&mut self, &mut R) -> Result<()>;
 ...
```

## Tunnel

a TunnelNoRep with reply
```rust
pub trait Tunnel : TunnelNoRep where Self::TR : TunnelReader<RI=Self::RI> {
  type RI : Info;
  type RW : TunnelWriterExt;
  fn new_reply_writer<R : Read> (&mut self, &mut Self::DR, &mut R) -> Result<(Self::RW, <Self::P as Peer>::Address)>;
  fn reply_writer_init<R : Read, W : Write> (&mut self, &mut Self::RW, &mut Self::DR, &mut R, &mut W) -> Result<()>;
}
```
So the missing component to reply, RI is a info a trait containning information used to build reply, the writer is the reply writer (proxy and dest reader for reply are the same as for query). 
With : 
```rust
pub trait TunnelReaderNoRep : ExtRead {
  fn is_dest(&self) -> Option<bool>; 
  fn is_err(&self) -> Option<bool>; 
}
```

## TunnelError

a tunnel with error return capability
```rust
pub trait TunnelError : TunnelNoRep where Self::TR : TunnelReaderError<EI=Self::EI> {
  type EI : Info;
  type EW : TunnelErrorWriter;
  fn new_error_writer (&mut self, &mut Self::TR) -> Result<(Self::EW, <Self::P as Peer>::Address)>;
  fn proxy_error_writer (&mut self, &mut Self::TR) -> Result<(Self::EW, <Self::P as Peer>::Address)>;
  fn read_error(&mut self, &mut Self::TR) -> Result<usize>;
}
```
very similar to reply info (except that error writer do not return content but an error id only, plus proxying is done with a specific writer, and error reading also).
with : 
```rust
pub trait TunnelErrorWriter {
  fn write_error<W : Write>(&mut self, &mut W) -> Result<()>;
}
```

## TunnelManager

a Tunnel but with caching capability

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
with 
```rust
pub trait TunnelReader : TunnelReaderNoRep {
  type RI;
  fn get_current_reply_info(&self) -> Option<&Self::RI>;
}
```

a cache for peer is used and and symetric writer can be build from it for proxying plus dest multisim read. Notice the CacheId constraint a trait for producing unique cacheid between peers.

## TunnelManagerError

a tunnel with error return capability on cache
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
use to create new symetric reader writer. In mydht this kind of scheme could be use post authentication as the message shadower and is instantiated from peer implementation, therefore I do not think that both could be shared.

## RouteProvider

Finally this may be the only trait that require 


# tunnel and mydht interaction??

## considering only on time unidirectional message

We have seen that our mydht may contain a TunnelNoRep implementation ('Full' object as it is currently the only impl).
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

problematics on are :
- me : should be a Ref<Peer> (in mydht we use Ref<P>)
- cache : a cache can only be at one location so our Tunnel implementation needs to be at a single place
- route_prov : a route provider, same restriction as previous cache plus the fact that it needs to build its route from a source of peers

## usage of peerref

simply implement route peer for the peerref of peer : that way even without peerref in tunnel crate we use peerref from 

Something like will be added to the adapter (I tried it in different context and expect it to be impossible in rust, we may simply implement for every Ref)
```rust
impl<P : MPeer,RP : Borrow<P>> TPeer for RP {
```

## usage of peers from mydht to build route 

- from mydht mainloop peercache : only connected peers good to ensure that the message got a minimal chance to be send, but higly unsecure as we know which peer are connected
- from mydht default peerstorage : issue of unconnected peer
- from a global service where we exchange reachable peers : a store of peers initiated from connected peercache and communication of peers


## location of the tunnel

* out of mydht, sending query to the global peer manager service, this mydht include in a transport.

* a route service communicating with peerstore and mainloop
  * use of mydht global service : so the mydht instance is only here to produce route (with peer exchange and management) and include in transport use by another mydht instance
  * use of a service spawn from transport : a tunnel transport over another transport : bad as peer exchange is done with this transport.

* in the global service : best but means that the mydht is specific to tunnel (an inner service can be use and/or sending receiving from outside), plus in mydht global service can easily send query to peerstore or mainloop (especially using a subset on peerstore : probably need to create a second variant of subset being more random : currently subset is a mix of random and not random depending on implementation , which broke a lot of tests).

## with error mgmt

This is fine to have a transport using it this way (error if send is in the transport interface)


## considering unidirectional message mwith reply no cache

Reply on the other hand is in no transport interface.
If we consider that the tunneled message is opened in localstream, we could only reply from this local stream (the reply is done using next read bytes as header and we should not copy it to global service).
So we need to make global service launchable from localservice (a shared inner service for tunnel content) so that we can directly reply (a query for the stream to mainloop is still needed) if possible (and proxy query to global otherwhise).
This is close to the same use case as proxying.

## considering unidirectional message with cached reply

Cached reply means that proxy of reply required to query the globalservice for a shadower that have been insered when query has been proxied : ok (yet there is a possible race issue so global service will need to store those query?)

## Tunnel as a transport


The first idea is to use tunnel as a transport as we could have implemented a transport between tor hidden services.
* what match with mydht transport

- we send queries with a destination
- connect could simply get the route


* what does not match

- we do not authenticate (mydht allows it)
- origin of a receive query is hidden, we only got origin of proxying peer : not such a big issue, just that service implementation must be independant of origin.
- replying without an origin can only be done with frame content by spawning a reply writer with a dest peer which can be different (here we may need to borrow the inner write stream).
- proxying on read content requires to get the handle on a inner write stream (like if tunnel as a transport should include a mydht instance).


All this seems bad, it should be possible, doable with an established tunnel (not curently implemented in tunnel I think) : with an established tunnel the main problem is that origin is unknow -> so we need to run with no authentication mydht mode.
We could run with authentication too (peer transmit in ping is fine in this case). 

## MyDHT as tunnel reader/writer provider

tunnel is currently running over reader and writer : similart ot mydht transport streams.
MyDHT could be route provider implementation (all tools are here).

This is true for sending but for reading it is not and we could use local service for this specific reading code, therefore we also could put tunnel into global service it seems better.

## Conclusion

tunnel should be into global service (only one cache and communication with possible route provider).  
Sending query is therefore done by global service on an new write stream or by a special message for write stream containing peers for route and inner service query : the message encoder used will call tunnel primitive.  
Proxing and replying will probably needs to be done from local service and by using special message decoder (command being reply or proxy).  


# Next steps 


First step should be to create a mydht-tunnel crate where tunnel is located into a mydht global service which can retrieve route and needed inner transport stream with two modes :
  - borrowing stream : to avoid openning to much transport : this would require some specific mainloop implementation (similar to query of mainloop cache on some peerstore query) and may block the inner dht a lot
  - sending new write stream : more realist : this would be done through global service
Read of message would be done in local service (require that local service got access to read stream)
  - if dest then local service read content and send it to global service
  - if proxy then local service open a new connection (borrowing not for initial implementation)
  - a specific message dec/enc will be use : it will allow to build frame from write stream and to read if proxy or query local command from read stream
Proxying is also done in localservice (localservice need to get a write stream from mainloop) after querying for state (by queryid to get shadower) in global service or by geting stream writer from mainloop (no queryid : in query).
An inner service for replying could be called from global or local depending on the fact that reply is possible by being include in frame (local service) or by using tunnelcache (global service).

Second step would be to implement transport from this implementation (this transport will spawn new route at each message sending : very slow but nice).



This design means that local service must have access to read stream, an alternative would be to let local service send a mainloop new command with its read token to proxy command to the right write service and from this point let mainloop close the read service (as if command end was received) and append its stream in the write command (a special trait is needed similar to the one use to make local query in mainloop).  
Something like MainLoopCommand::EndReadProxyWriteWithStream(p::address,option<write_token>,localservicecommand).  
Then the write special serializer will have a message containing the readstream and making it possible to inline proxy. That seems better and can apply to reply to (in this case the reply need to be include in the command from localservice). This seems better ordered than waiting on connection and write stream from reader.

