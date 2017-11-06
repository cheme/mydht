# Implementing MyDHT tunnel

In last post we give some thought on tunnel and mydht interaction and conclude that a mydht implementation specific to tunnel should be build (and in a second time use it in a transport).

- make peer update sendable to global service (see traint PeerStatusListener).
- write test case first

# fun debug/test

the test ran on 8 peer but not on more and sometime hand on 8 peer.

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
