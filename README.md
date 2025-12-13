# Rust Bee

## Introduction

The topic of build scripts will never end. 
I did one for Java called [7Bee](https://github.com/drogatkin/7Bee). Then I started using it for building Rust projects too.
Soon, I decided to rewrite it in Rust. Maior disadvantage of 7Bee was using XML that made all scipts too bulky.
So one of the goals of Rust Bee was getting rid of XML, reduce the scripts footprint and make them easy readable.

## Rust Bee scripting language
The language is described [here](./doc/RUSTBEE.md).

## Bulding

It's a self building tool, however it needs a bootstrapping. [7Bee](https://github.com/drogatkin/7Bee) is used for the
[purpose](./bee-rust.xml) (see instructions inside).
You can use **RustBee** itself for building it after you built the starter version. 

The RustBee [script](./bee.7b) has one dependency [SimScipt](https://github.com/vernisaz/simscript).
Clone it first at its directory level as `rust_bee`. You can use the **install** target of the script for installing the tool.

### Dependencies
The following crates will be required to build the **RustBee**
- [SimTime](https://github.com/vernisaz/simtime)
- [SimZip](https://github.com/vernisaz/simple_rust_zip)
- [SimColor](https://github.com/vernisaz/simcolor)

Their repositories need to be checked out before building the **RustBee**.

## Version
The current version is **1.15.06**.

## Scripting examples

Some examples of using **RustBee** can be found [here](https://gitlab.com/tools6772135/rusthub/-/tree/master/src/script). 