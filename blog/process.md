


Lastly I got some time in my hand (ended my previous job and looking to more low level programming), and took it to refactor some pretty terrible old project of mine.

Even if the refactor is far from complete, the design and its primo implementation is.


# Previous design

One of those is MyDHT, the purpose was to design a library to run a distributed hash table, and as it evolves to key value peer to peer info exchange with multi-hop request proxying (query proxyied to none connected peers).
So it is finally a HashTable only if using a right Routing implementation. 
Final goal was at the time to implement web of trust application upon this technical brick.


At the time I was less aware of asynch transport and design my lib upon standard rust api only :
        - a peer abstraction (related to transport for listening address obviously)
        - a keyval abstraction for content to be share (peer being obviously a keyval)
        - the transport trait was similar to tcp synch, but some very odd design let me run it on udp with dirty tricks (reader containing the udp frame and read thread running from start on every one). I also put some attempt to use mio (asynch transport) but using this design it had very less advantages and I dropped when the coroutine crate I used at the time becames deprecated.
                The important point here is that we got a read stream and a write stream and that both does not need to be spawn at the same time : we allow unidirectionnal transport resulting in one socket to send to peer two and one other socket to receive from peer two, but also using the same socket depending on transport implementation : therefore we dissociate sending and receiving process (also true in new design).
        - the encoding/decoding of Key Value was using Rust standard serialize which I switch to serde with my refacto. It allows many type of exchange (mainly use to test with json and binary (bincode crate)).
        - authentication scheme was broken (udp implementation was allowed by it) : with only two messages called ping and pong leading to a potential replay attack (new scheme run with ping - pong - pong).
        - rules were overly complicated and ill designed (it stay true but limited to kvstore service now)
        - some shadowing mechanism where implemented (refacto was done to use my 'readwrite-comp' lib which was totally inspired by this) allowing some forms of encryption over transport.
        - keyval and peer storage trait with cache trait and query cache trait, quite a mess by looking back, still have to redesign that
        - Route trait to decide which peers to query.



The schema bellow gives an idea of the running threads :


Communication between thread used standard Mpsc, with Arc over peer and KeyVal, the sender being group in a 'RunningProcess' clonable struct it was pretty flexible and basically every thread could send message to every thread except the read and write threads which were driven by transport for the first and message proxied by peer management thread for the second.

All thread running a single static function call (with a lot of parameter and generic type (generic type were group in few single generic trait by using associated trait later but it still look prety bad)).


Quickly it felt bad to open so many system thread and I put some dirty 'ClientMode' and 'ServerMode' enum (and their associated handle) to run in a new thread or in a batch of thread or in the same thread.
Client and Server function call became fastly unintelligible (what I thought when rereading the project), and it even lead to change of 'Transport' trait by introducing a complicated 'start' function using a closure over the server thread called on reception of new connection.

 https://github.com/cheme/mydht/blob/8d2ff89d48ea9ef993524aabed7b666ba91f8160/src/procs/server.rs line 160 to 220 is a good illustration.


# New design

Being totally annoyed (it is a library and I do not want to make those choice definitive), by use of thread everywhere ClientMode and ServerMode were added, but looking back at it, it is close to unmaintanable and only there for Server and Client process.

So this redesign follows a few goals :
        - dependency upgrade : serde usage being the most obvious (all dependency were upgrade but a lot of subcrate are broken now)
        - actual usage of asynch transport
        - usage of future lib to simplify the thread model

Investigating the future lib lead me to tokio-core and tokio-service, I almost go this way but some of my plan at the time did not match (looking back at my final service abstraction it should have match exept for a few details) especially the idea that I wanted a more stream oriented transport trait (I want to plug my 'tunnel' lib other the transport but by using MyDHT internal peer management). I also had quite some fun using Mio (epoll for rust lib) directly and really liked the changes from when I tested it before.
So I decide to run bare mio (no abstraction as I thought it would be a quick job) with very basic state and service (ideally service should simplify what was run with 'ClientHandle' and 'ServerHandle' before).

So started with transport trait redesign, I quickly felt uneasy as before : I should use an Arc here, this should run in another thread but not in all case... And I use a 'Service' thread which was inspired by service design and a suspend/yield mechanism inconsciously inspired by some of the future concept (especially the fact that when experimenting with future I add to unpark future thread at some point by keeping a thread handle).

The point at first was to stop running those previous procs functions.
Next it quickly lead to a service based approach (model has change quite a bit from the start but seems ok now (except the dissiciation of channel and service leading to very verbose things)). At end, it look similar to what remember of erlang OTP.


  * Spawner

  * Blockin spawn

  * Handle

  Yes it feels bad, I wanted to get rid of those Client and Server Handle and have associated Service Handle now... At least this is reusable and not as specific to a context as before.

  * Synch object

  In my previous implementation, having done a few haskell, I tend to confuse a reference with an immutable object and force the Sync trait at many place (on peer and keyval at least) by considering those value to be read only (they are and a change in Peer address should be propagated to all process (currently it looks more robust on this point)).

  In this design we do not anymore use explicit keyval but as global service with messages and no such restriction as previously.

# configuring thread

In a test case, 'test/mainloop.rs' test_connect_all_th, a simple test running a local service proxying the message to the globalservice (useless except for testing) : and a global service implementation with a few touch command. The test run simple auth, simple touch service and a simple peer query.

Globally 'ThreadPark' spawner is used everywhere with an Mpsc channel and using ArcRef over Peer (Arc).

'test_connect_local' will try to run as much as possible over a single thread, though implementing MyDHTConf trait for 'TestLocalConf'.
So I copy implementation with threads and will adapt.

First thing no need to use Arc if we run locally, therefor we will use Rc through RcRef (implements SRef and Ref<P>).

```rust
  type PeerRef = RcRef<Self::Peer>;
  //type PeerRef = ArcRef<Self::Peer>;
```

Compiling it will obviously break everywhere : there is no way that our 'MpscChannel' (which is only a wrapper over standard rust mpsc sender and receiver) will run with some Rc<Peer> which is not Send (Send trait is a rust marker for type that could be send to other traits, Rc as a counted reference cannot).

4 nice errors appears all similar to :
```
error[E0277]: the trait bound `std::rc::Rc<mydht_basetest::node::Node>: std::marker::Send` is not satisfied in `procs::server2::ReadService<test::mainloop::TestLocalConf>`
   --> src/test/mainloop.rs:658:6
    |
658 | impl MyDHTConf for TestLocalConf {
    |      ^^^^^^^^^ `std::rc::Rc<mydht_basetest::node::Node>` cannot be sent between threads safely
    |
    = help: within `procs::server2::ReadService<test::mainloop::TestLocalConf>`, the trait `std::marker::Send` is not implemented for `std::rc::Rc<mydht_basetest::node::Node>`
    = note: required because it appears within the type `mydht_base::utils::RcRef<mydht_basetest::node::Node>`
    = note: required because it appears within the type `procs::server2::ReadService<test::mainloop::TestLocalConf>`
    = note: required because of the requirements on the impl of `mydht_base::service::Spawner<procs::server2::ReadService<test::mainloop::TestLocalConf>, procs::server2::ReadDest<test::mainloop::TestLocalConf>, mydht_base::service::DefaultRecv<procs::server2::ReadCommand, std::sync::mpsc::Receiver<procs::server2::ReadCommand>>>` for `mydht_base::service::ThreadPark`
```

At this point we could change all threaded spawner, but their is the use case where we transmit peers as RcRef or CloneRef (for instance if peers is a single ip address) and still use threading. 

So the solution is to copy the Rc to the new thread, that is what SRef implemnetation of RcRef does : give a sendable type for Rc<Peer> (simply cloning Peer) and then in the spawn thread put back this to a RcRef (putting it back may not be suitable but RcRef<Peer> is the only kind of peer define in the conf and transmit to service commands).
This sending of SRef could not be achieve with MpscChannel but another Channel implementation is doing it (cloning/unwraping on write and putting back to Ref on recv) : 'MpscChannelRef'
```
pub struct MpscChannelRef;
pub struct MpscSenderRef<C : SRef>(MpscSender<C::Send>);
pub struct MpscReceiverRef<C : SRef>(MpscReceiver<C::Send>);
pub struct MpscSenderToRef<CS>(MpscSender<CS>);
```
It simply wrap an MpscSender of the Send inner typer of our Ref<P> as an SRef, implementation is straightforward.

Changing all our channels (ApiChannelIn, PeerStoreServiceChannelIn, MainLoopChannelIn, MainLoopChannelOut, ReadChannelIn, WriteChannelIn, PeerMgmtChannelIn, GlobalServiceChannelIn) for localproxy we already use a 'NoChannel' as input (received message is build from frame in Read service) and a non threaded spawner : see 'localproxyglobal' macro.
After manually implementing SRef for a lot of Command and Reply struct (a macro is really needed here see for a simple example TODO add link to commit) we obtain :
```
error[E0277]: the trait bound `std::rc::Rc<mydht_basetest::node::Node>: std::marker::Send` is not satisfied in `procs::server2::ReadService<test::mainloop::TestLocalConf>`
   --> src/test/mainloop.rs:658:6
    |
658 | impl MyDHTConf for TestLocalConf {
    |      ^^^^^^^^^ `std::rc::Rc<mydht_basetest::node::Node>` cannot be sent between threads safely
    |
    = help: within `procs::server2::ReadService<test::mainloop::TestLocalConf>`, the trait `std::marker::Send` is not implemented for `std::rc::Rc<mydht_basetest::node::Node>`
    = note: required because it appears within the type `mydht_base::utils::RcRef<mydht_basetest::node::Node>`
    = note: required because it appears within the type `procs::server2::ReadService<test::mainloop::TestLocalConf>`
    = note: required because of the requirements on the impl of `mydht_base::service::Spawner<procs::server2::ReadService<test::mainloop::TestLocalConf>, procs::server2::ReadDest<test::mainloop::TestLocalConf>, mydht_base::service::DefaultRecv<procs::server2::ReadCommand, mydht_base::service::MpscReceiverRef<procs::server2::ReadCommand>>>` for `mydht_base::service::ThreadPark`
```
Obviously changing the channel was not enough since its type does not match our ThreadPark spawn, looking back at our thread park spawner it requires content to be Send, of course our inner mpsc channel use send content but their is another thing : when spawning a service a command can be use as first parameter (optional command input in spawn function) : this let us run some use case as in 'localproxyglobal' macro where the service is running with a dummy Channel input and for a single iteration like a function call. Service must be restart at every call with its next command as parameter (handle indicate it is finished and service state is reuse from handle); this kind of service usage is possible only if service is call with an input command (not allways the case) and if restart of finished service is implemented (actually I may have been lazy on many restart it is ok with write service and local service at least).
This is also true for the Service itself, so SRef must also be implemented for our service, it makes sence as many service use a ref of current peer and other inner content which could be a ref.

So the ThreadPark spawn need to use a Send command and similarily to what we did with channel we will use a 'ThreadPark' variant, 'ThreadParkRef' which is going to spawn thread over the associated type of RcRef<P> as an SRef which is Send (P being Mydht Peer, P is Send). Then before looping over the iteration of our service call, our type is wrapped back as an Rc.

After changin to ThreadParkRef it compiles and test run.

In fact I did cheat on order of errors, first is Spawner then Channel, but it was simplier explaining in this order.

So what did we do, just change MyDHT usage of Arc to use of Rc but staying in a highly multithreaded environment. 

Next step will be easiest : changing some service to run in the same thread as the main loop service. Please note that this is only possible if the transport is asynchronous, for synch transport there is a lot of thread restriction and trick that I may describe in a future post.


