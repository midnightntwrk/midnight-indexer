# Using Redis Pub/Sub from Rust

## Context and Problem Statement

To use Redis Pub/Sub in production environments, we need a solution that gracefully handles connection issues like dropped connections.

The most popular – according to [crates.io](https://crates.io/) – Rust library [redis](https://docs.rs/redis/latest/redis/) generally does support asynchronous and non blocking IO, but unfortunately in the latest version 0.25.4 subscription streams obtained via [`redis::aio::PubSub::on_message`](https://docs.rs/redis/latest/redis/aio/struct.PubSub.html#method.on_message) silently terminate in the event of a network issue, i.e. they no longer yield messages. With the current API there seems to be no way to work around this issue, e.g. get notified about any issues and reconnect manually.

## Decision Drivers

* It must be possible to handle connection issues
* Asynchronous and non blocking solutions are preferable

## Considered Options

* Use latest nightly version of the redis library
* Build our own solution on top of the redis library
* Fork/patch the redis library
* Use alternative libraries
* Use the synchronous and blocking API of the redis library

## Decision Outcome

While the latest nightly version of the redis library has made some progress wrt automated connection management for Pub/Sub, it still does not re-subscribe to topics. Looking at the respective issues and PRs it seems unlikely that full support for automated connection management for Pub/Sub will arrive in a release in the next months.

At first glance it seemed promising to build our own solution on top of the redis library, but unfortunately some important and substantial pieces of code are not public.

Going further, even forking and patching did not turn out feasible, because of the huge difference between the so far used simple `Connection`s for Pub/Sub and the need to replace these by `ConnectionManager`s which offer automated connection management.

There are only few other Rust libraries for Redis and most are in a state which cannot be considered production ready. We have also investigated the seemingly most promising one, [fred](https://docs.rs/fred/latest/fred/), but it shows similar issues like the redis library that cannot be worked around easily.

So finally – and luckily – the synchronous and blocking API of the redis library offers a simple way to observe issues: [`redis::PubSub::get_message`](https://docs.rs/redis/latest/redis/struct.PubSub.html#method.get_message). As shown the [spike](../../spikes/spike-redis-pubsub/Cargo.toml), it is straightforward to implement a Tokio [mpsc::Receiver](https://docs.rs/tokio/latest/tokio/sync/mpsc/struct.Receiver.html) that is "endlessly" sourced by a blocking friendly and error handling task making use of `redis::PubSub::subscribe` and `redis::PubSub::get_message`. The respective code can be found in the `spikes/spike-redis-pubsub` directory as of [PR #4](https://github.com/input-output-hk/midnight-indexer/pull/4).

### Consequences

* Good, because we can use Redis Pub/Sub from Rust in production environments with very little effort.

### Confirmation

See above explanation and respective spike code.

## Pros and Cons of the Options

See above explanation of the investigations.
