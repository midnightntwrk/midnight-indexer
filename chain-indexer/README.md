# Chain Indexer

The Chain Indexer connects to the Node, i.e. the Midnight blockchain, to fetch data, e.g. Midnight Transactions, and stores it in its database.

## Node requirements

The node must run with `--state-pruning archive`. Blocks are sourced through the new JSON-RPC spec
(`archive_v1_*`, `chainHead_v1_*`, `chainSpec_v1_*`) only, and the `archive_v1_*` methods exist only
on archive nodes; the Chain Indexer refuses to start without them, naming the missing methods.

## Block sourcing

Blocks are sourced in chunks ahead of indexing, verified, and decoded on a dedicated CPU pool; see
[Block sourcing](../docs/architecture.md#block-sourcing). The settings, with their defaults:

| Setting | Default | |
|---|---|---|
| `infra.node.source_chunk_size` | 64 | Heights per chunk; keep it at least 8 times `decode_cpu_threads`. |
| `infra.node.source_chunks_ahead` | 8 | Chunks in progress, and chunks waiting for decode. |
| `infra.node.rpc_batch_size` | 64 | Calls per JSON-RPC batch; public endpoints accept 64. |
| `infra.node.rpc_batches_in_flight` | 16 | Batches in flight on the node connection. |
| `application.decode_cpu_threads` | cores − 1 | Threads decoding blocks. |

Blocks held in memory are bounded by about `2 × source_chunks_ahead × source_chunk_size` plus a
few chunks.

## Tools

- `cargo run -p chain-indexer --features standalone --example source -- --node <url> --from <height> --count <n>`
  prints sourced and decoded blocks.
- `just source-throughput <url> [from] [count]` measures block sourcing and decoding against a
  node, from genesis to the finalized height by default. Env vars select the settings;
  `DECODE_CPU_THREADS=0` measures sourcing alone. See `tests/pipelines/source_throughput.rs`.
