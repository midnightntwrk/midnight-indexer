# Memory: allocator metrics and heap profiling

On Linux every indexer executable uses jemalloc as its global allocator (declared in each binary's
`main.rs` from the `Jemalloc` that `indexer-common` re-exports, as the node does in its binary).
This guide covers the two ways to look into its memory use: the allocator metrics that every build
exports, and heap profiles from a build with the `jemalloc-profiling` feature. Other platforms keep
the system allocator and export no allocator metrics.

## Allocator metrics

Every Linux build with `telemetry.metrics.enabled: true` exports `indexer_memory_allocator_bytes`
on the Prometheus endpoint, next to the component's own metrics. It is refreshed every 5 seconds
(`indexer-common/src/telemetry/allocator_metrics.rs`) and has one series per jemalloc statistic
(`stat` label):

| `stat`      | Meaning                                                                    |
|-------------|----------------------------------------------------------------------------|
| `allocated` | Bytes in live allocations requested by the process.                        |
| `active`    | Bytes in pages that hold live allocations (`allocated` plus page slack).   |
| `metadata`  | Bytes jemalloc uses for its own bookkeeping.                               |
| `resident`  | Bytes jemalloc holds in physical memory. The allocator's share of RSS.     |
| `mapped`    | Bytes in chunks jemalloc has mapped from the OS.                           |
| `retained`  | Bytes of virtual memory jemalloc kept after returning the pages to the OS. |

How to read them when memory grows:

- `allocated` grows: the process holds more and more live data, either from a leak or from a
  cache with no bound (the ledger arena, the contract state cache, wallet sessions). A heap
  profile will show where it is allocated.
- `allocated` is flat and `resident` grows: the memory is fragmented, or jemalloc has not
  returned freed pages to the OS yet. A heap profile won't show anything, so look at allocator
  tuning instead.
- Process RSS grows but `resident` does not: the memory is outside jemalloc, so it was mapped
  directly and not allocated with `malloc`. Examples are thread stacks (`thread_stack_size` per
  thread, as far as they have been touched) and memory-mapped files (`RssFile` in
  `/proc/<pid>/status`).

The indexer's jemalloc also replaces the C `malloc`, so memory allocated by C code linked into
the process is counted in these statistics and sampled by heap profiles. That includes SQLite's
page cache in standalone mode: when it grows, `allocated` grows.

## Heap profiles

### Build

The feature lives on `indexer-common`, so enable it through that crate whichever executable you
build:

```bash
CARGO_PROFILE_RELEASE_DEBUG=line-tables-only \
  cargo build --release -p chain-indexer --features cloud,indexer-common/jemalloc-profiling
```

The feature builds jemalloc with `--enable-prof`. Without debug line tables the profile shows
bare addresses. They don't slow the binary down, but they make it a lot larger.

Profiling is compiled in but off. A binary built with the feature and run without `MALLOC_CONF`
behaves like a normal build, apart from a little sampling bookkeeping. Default builds and the
Docker images are unaffected.

### Run

Enable profiling through `MALLOC_CONF`. The indexer's jemalloc is unprefixed, so the variable is
`MALLOC_CONF`, not `_RJEM_MALLOC_CONF`.

```bash
mkdir -p /data/heap
MALLOC_CONF=prof:true,prof_final:true,lg_prof_interval:30,prof_prefix:/data/heap/chain-indexer \
  ./target/release/chain-indexer
```

- `prof:true` turns sampling on. Allocations are sampled about once every 512 KiB
  (`lg_prof_sample:19`, the default), which costs a few percent of throughput.
- `lg_prof_interval:30` writes a dump every 2^30 bytes (1 GiB) allocated in total. Lower it for
  more frequent dumps. Dumps are named `<prefix>.<pid>.<seq>.i<n>.heap`.
- `prof_final:true` writes a last dump when the process exits normally, which includes a
  graceful shutdown on `SIGTERM`. It doesn't write one when the process is killed with `SIGKILL`
  or by the OOM killer.
- `prof_prefix` must point at a directory the process can write to. Without it, dumps go to the
  working directory as `jeprof.*.heap`. Dumps are small (tens to hundreds of KiB), but with a low
  `lg_prof_interval` there are a lot of them.

### Analyse

The build compiles its own `jeprof`. It is a Perl script, found at:

```bash
find target/release/build -path '*tikv-jemalloc-sys*' -name jeprof -type f
```

The `jeprof` that ships with a distribution's jemalloc 5.x package (`libjemalloc-dev` on
Debian/Ubuntu) works as well.

The most useful view for growth is the difference between an early and a late dump. It shows the
call stacks whose live memory grew in between:

```bash
jeprof --text --cum --base=/data/heap/chain-indexer.123.10.i10.heap \
  ./target/release/chain-indexer /data/heap/chain-indexer.123.80.i80.heap | head -40
```

Use `--cum`. In the default flat view almost all memory is attributed to jemalloc's own
`prof_backtrace_impl` frame, and the indexer's code only appears in the cumulative column.

To render the same comparison as a call graph:

```bash
jeprof --svg --base=early.heap ./target/release/chain-indexer late.heap > growth.svg
```

Pass the binary that wrote the dumps. A binary from another build resolves the addresses wrongly.

### Cautions

- The published Docker images are built without `jemalloc-profiling`, so they cannot write heap
  dumps. With `prof` options in `MALLOC_CONF` they print `<jemalloc>: Invalid conf pair` to
  stderr and run as usual. To profile, build the binary yourself as described above. Treat that
  build as a diagnostic one: run it to reproduce the problem, then go back to the standard image.
- Every Linux build reads `MALLOC_CONF`, with or without the profiling feature. It changes how
  the allocator behaves for the whole process, so set it deliberately, like any other deployment
  setting.
- Keep heap dumps internal. A dump is a small text file with the sampled call stacks as
  addresses and the process's memory map, which includes the file paths and load addresses of
  the binary and its libraries. It holds no heap contents.
