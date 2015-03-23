mydht
=====

[![Build Status](https://travis-ci.org/cheme/mydht.svg?branch=master)](https://travis-ci.org/cheme/mydht)

A generic library for implementing DHT (Distributed Hash Table).

Written in rust, it is designed to be as flexible as possible to implement and experiment with various DHT usecases.

DHT is a bit of language misuse, since it could be route over hashing but in most of my use case it will be route other trust or in very small peer groups. We should name it Distributed Trust Table for many use cases.

DHT should allow using Web of trust with signing of peers but also of values.

Build
-----

Use [cargo](http://crates.io) tool to build and test.

Status
------

WIP

Currently this is Unstable and on a lot of functionalities still a WorkInProgress (see issue tracker which is also a roadmap in fact).

TODO add travis-ci , rust-ci for doc


Overview
--------

Several abstraction trait are :

* route : routing table, implemented examples are 
  - "inefficientmap" a toy implementation with close to no strategy for choice of nodes.
  - "btkad" a classic kademlia implementation (external library fork used).
* transport : transport to use and needed additionnal synchro :
  - "tcp" transport based on tcp, almost native, connected service
  - "udpcusto" TODO transport based on udp, with a lot synchro multiplexing 
  - "udp" for non connected dht only (no thread are kept to synchro the connexion and proxy is not usable)
  - "tor-i2p..." TODO
* kvstore : the keyvalue persistence to use

* msgenc : type of encoding, it also covers decoding of frame, some dht property may be directly forced by being set on decode and not include in msg : encode may force some configuring as their is the common serialize interface and a prior filter on protomessage to a more efficient message
  - "json" : highly inefficient, greet for testing and standard rust library
  - "bincode" : binary encoding from external library.
  - "bencode" : bittorent encoding from external library.
  - TODO protobuf, cap'n'proto - pb is generation conf with parsing info

Some feature are (see rust doc) :
- direct routing
- direct reply asynch
- proxyied routing
- mix routing
- TODO tunneled routing
For pseudo anonimous feature, random hop number may be used but not on direct reply (requester in frame) and not with trace of previous hop (use for non Hash base routing to avoid loops).
For good anonimous please use/implement a custo transport on top of an anonymous network (eg i2p or tor).

Some example of DHT applications are :
- sample filemanager for sharing files. some starting/demo code in examples
- TODO some others application for sharing content or concept based on trust groups (at a peer level but also between concepts).



