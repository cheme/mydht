mydht-userpoll
==============

[![Build Status](https://travis-ci.org/cheme/mydht-userpoll.svg?branch=master)](https://travis-ci.org/cheme/mydht-userpoll)


MyDHT compatible simple poll in user space.

To be use to replace mio for synch transport or asynch transport where mio usage is not possible (wasm).
-> edge poll and no threads -> can be seen as a queue

Build
-----

Use [cargo](http://crates.io) tool to build and test.

Status
------

WIP, mainly for test purpose and experimentation with webassembly/webrtc (no perf focus)


