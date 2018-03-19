
# Roadmap for myDHT as a webassembly webrtc p2p


MyDHT design is quite low level, not using late rust server/network promoted paradigm : no Future abstraction, 

At some point, in regard to the maturity of the project, it seems counterproductive and project like rust-libp2p or codes from parity looks way better.

MyDHT is also a bit overgeneric, and totally not flexible (adding an new service is a pain).

Yet some nice things from this approach are :
        - no buffers : serializing and encrypting messages is always done the Reader/Writer way (see crates [readwriteext](https://github.com/cheme/readwrite-comp): read_from and write_into).
        - tunnel impl (thinking of a webrtc tunnel makes me smile).
        - generic component everywhere making it easy to switch parts. (and long and painfull to init an application due to heavily intricated types and lot of message trait to implement : almost no trait objects).
        - easy to get
        - Message passing between service : trying to reduce the blocking code (not a lot of exposed mutex and the one being use are here because I was too lazy to use a service abstraction) 
        - explicitly suspendable service : all service (thread or coroutine) are suspended when no message. It is the same with future or other abstraction, but it is easy to see here.

Having a good view on all the component, let me think about an interesting thing todo with this lib : 
having it run into a web browser.


## Things that are not compatible

- threads : threading in webassembly is a no
- files : no filesystem
- network : no tcp
- mio : no inner epoll or windob alternative


## Things todo

### threads
Do not use them (using js worker is an alternative but the cost and the difficulty to implement channel between them makes it a secondary objective, yet needed for voting machine poc at least for communication between the two DHT instance). Use coroutine for all service.

#### Coroutine

MyDHT do not use new stackless coroutine (generator like) added to the language and requires stackfull one (some inner service state could be in trait implementation : yield during a serde inline serializing for instance). The coroutine [crate](https://github.com/rustcc/coroutine-rs) is really nice, but uses x86 assembly c++ boost context assembly for obvious performance (and maintanability maybe) reason.  
So the task here would be to make this crate usable with webassemby : that is a really nice task and in fact targets the [context](https://github.com/zonyitoo/context-rs) crate. But the lack of goto (jump instruction) for webassembly makes it quite difficult.
An idea could be to use this kind of [relooper](https://github.com/WebAssembly/binaryen/wiki/Compiling-to-WebAssembly-with-Binaryen#cfg-api) approach, setting static branch label to replace pc jumps : this seems out of scope regarding my goals (need to include those branch label at a compiler level).  
So for now lets keep an eye on [#796](https://github.com/WebAssembly/design/issues/796) and future webassembly design change and fallback to another approach.  

I would truely like webassembly to support goto, but I also understand that the implementation for browser compiler becomes complicated (js jit compiler would not be that easily reusable).

#### Restartable service


Another possibility would be to use 'RestartOrError' spawner which can only be use for restartable service.  
This is a spawner that simply return the service state when yielding and relaunch the service when unyielding (good for service that only yield on their message channel input and that do not need to run in a different thread).  
It means that globalservice and localservice will need to be restartable (if they do not yield it is ok).  
It also means (more problematic) that our existing client and server service need to be restartable. Both only yield during when reading and writing in the transport stream. Meaning that serialization and deserialization need to be restartable, which is currently not the case.  

We could switch our trait interface to allow a state :
for instance MsgEnc 
```rust
  fn decode_from<R : Read,ER : ExtRead,S : SpawnerYield>(&mut self, &mut R, &mut ER, &mut S) -> MDHTResult<ProtoMessage<P>>;
```
  will become something like
```rust
  fn decode_from<R : Read,ER : ExtRead,S : SpawnerYield>(&mut self, &mut R, &mut ER, &mut S, &mut ProtoMessageDecodeState<P>) -> MDHTResult<Either<ProtoMessage<P>,ProtoMessageDecodeState<P>>>;
```

And similar (but easiest) adjustment for but for attachments (simply current file and write/read ix to store in the service variables).


An idea could be to use some serialization that can suspend by returning its current state. That is totally not the case with serde and is not that easy.  
After some thinking, I may simply use the pragmatic way :
The state would be some composition (see readwritecomp crate) reader and writer. Those adapter other reader and writer will allow encode and decode restartable : reader will use a double buff (costy) and read from it on restart before consuming new byte from the transport stream; while writer will keep trace of number of written bytes and skip some actual write on restart. It means that reader and writer will cost a lot more. For attachment, the code could be cleaner and an inner state will be use.

This functionality will be gated behind a restartable const in MDHTConf.


### Files

The kv store abstraction : need an implementation using indexeddb or localstorage.  

Attachment is currently a pathbuf, switching to a string could allow multiple attachment type (filestore, file, webstore).  

Anyway attachment read and write depends on MsgEnc trait implementation only : MsgEnc trait should be split in two : MsgEnc and Attachment (with read_from and write_into as trait method) to avoid using different encoding depending on the target and having attachment outside of encoding (json msgenc crate already include some byte only attachment code and it doesnot make sense to transmit attachment with something else than direct byte content.  
### Network

Data over webrtc backend, see for instance torrent implementation in js -> require a server implementation here (managing user connection redirection).

That may be the first thing to test and the funniest one, especially seeing the cost of connection and deconnection.

### event loop

Mio could not be use in webassembly : need to trait abstract its usage then have a rust userspace epoll implementation https://idndx.com/2014/09/01/the-implementation-of-epoll-1/

### Openssl

Could not use openssl and will need to update some old implementation with rust-crypto (I already use rust crypto for ciphering canvas in webassembly and it went pretty smoothly).  
Here we can see that webassembly ABI is another important functionality in the webassembly roadmap (I do not think that compiling openssl to webassembly is easy or a good idea). Thread functionality and jump (goto instruction for coroutine) are also some very relevant functionality to port myDHT. 


