# Connecting to the Node via JSON RPC from Rust

## Context and Problem Statement

The Node exposes a JSON RPC API based upon [Substrate Remote Procedure
Calls](https://docs.substrate.io/build/remote-procedure-calls/) with HTTP and WebSocket transports.

The current Scala implementation of the Indexer employs a handwritten WebSocket implementation that
does not properly leverage the features of JSON RPC, in particular the correlation between requests
and responses is not established via the call ID, but instead via some particular usage of pooled
connections.

For the Rust implementation we are looking for an easier and more idiomatic way to access the JSON
RPC API of the Node.

## Decision Drivers

* Easy to use and understand
* Effective resource (e.g. connections) usage
* Use existing libraries if possible

## Considered Options

* Use a Rust WebSocket library like
  [tokio-tungstenite](https://github.com/snapview/tokio-tungstenite)
* Use the [jsonrpsee](https://github.com/paritytech/jsonrpsee) Rust JSON RPC library
* Use the [subxt](https://github.com/paritytech/subxt) Substrate library

## Decision Outcome

jsonrpsee has been created by Parity, the creators of Substrate, which is the fundamental framework
used for the Node. According to crates.io and GitHub it is quite popular and well maintained. It
offers a convenient high level API that makes it easy to use. jsonrpsee offers HTTP and WebSocket transports, utilizing bullet proof libraries like [hyper](https://github.com/hyperium/hyper) under the hood.

Another library created by Parity is subxt. It offers an even higher level "pure Rust", i.e. no HTTP or WebSocket, API. Like jsonrpsee it is very popular and it is even already used by the Node team. A particular advantage over jsonrpsee is its comprehensive domain abstraction, i.e. for many relevant aspects there are Rust functions and structs. E.g. one can subscribe to finalized blocks and get a stream of `Block` values; no need to deserialize to hand written structs.

While both jsonrpsee and subxt meet the above requirement, subxt seems even easier to use and hence more productive. Therefore we choose subxt for connecting to the Node from the Rust Indexer.
