mydht-base
==========

[![Build Status](https://travis-ci.org/cheme/mydht-base.svg?branch=master)](https://travis-ci.org/cheme/mydht-base)


MyDHT library base component, this is split from mydht to allow lighter dependency for implementing component.

It contains basic interface (traits) but no implementation logit.
It may contain some struct too (when no genericity for some concepts), notably some simple implementation (without dependencies) like simplecache (implementations over HashMap).
It also contains some utilities.

Implementation of trait should only depend on this crate, some method should therefore be added here from mydht depending on what is needed.

Build
-----

Use [cargo](http://crates.io) tool to build and test.

Status
------

WIP

