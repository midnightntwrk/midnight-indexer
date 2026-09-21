# `segment-split` — guaranteed/fallible segment-split fixture

A custom Compact contract whose only purpose is to put a contract Call on chain
in the state **"guaranteed transcript applied, own fallible segment failed"**,
so the e2e suite can assert how the indexer represents it.

Used by `qa/tests/tests/e2e/contract-actions-partial-success.test.ts`.

## Why the contract looks the way it does

A Midnight transaction has one guaranteed segment (id `0`) and N fallible
segments. A single Call carries **both** a guaranteed and a fallible transcript,
split by `kernel.checkpoint()`. Segment 0 is all-or-nothing; a fallible segment
rolls back on its own. So a Call can legitimately have had a real effect on
chain (its guaranteed transcript applied) while its own fallible segment failed.

Two details are load-bearing, and both were established empirically:

1. **The failure must happen at ledger-apply time, not proof time.** A circuit
   that fails while being proved never reaches a block, so there is no
   transaction result at all. The trigger used here is a stale-state arithmetic
   underflow: each `fuse*` counter starts at 1, two calls are built against the
   *same* on-chain state snapshot, and the second one underflows when the ledger
   re-runs its transcript against the state as it actually is at apply time.

2. **The expensive `ballast` writes are required.** The ledger picks the
   *largest* guaranteed prefix whose cost fits `guaranteed_budget`
   (`ledger/src/construct.rs`, step 4c). Without the ballast the whole circuit
   fits, everything lands in the guaranteed phase, and the node rejects the
   stale call from the mempool ("guaranteed execution would fail") instead of
   including it — so no partially successful transaction is produced.

The two circuits differ only in where the expensive section sits:

| Circuit | Pre-checkpoint | Resulting shape |
|---|---|---|
| `burnWithGuaranteed` | one cheap `Counter` increment | `guaranteed_transcript: Some(..)` — the regression case |
| `burnWithoutGuaranteed` | nothing | `guaranteed_transcript: None` — nothing applied at all |

The test verifies both shapes with the toolkit's own `show-transaction`
*before* asserting anything about the indexer, so "the indexer reported it" can
never be confused with "the fixture never produced a guaranteed transcript".

## How it gets compiled

Only two files are committed: `segment-split.compact` and its toolkit-js
`segment-split.config.ts`. The compiled output — generated JS, ZKIR and prover
keys, close to a megabyte of binary nobody can review in a diff — is **not** in
the repository. The test builds it on the fly:

1. `utils/compact/compact-compiler.ts` builds `compact-toolchain:<version>`
   from `utils/compact/compact-toolchain.Dockerfile` (first use only; a Docker
   image cache hit afterwards), which installs the pinned compactc release.
2. It copies both committed files into
   `.tmp/compact/segment-split-<digest>/` and runs `compact compile` there.
   The digest covers the compiler pin and the bytes of both inputs, so editing
   the contract compiles into a fresh directory instead of reusing stale
   output, and an unchanged fixture never recompiles.
3. `ToolkitWrapper` mounts that directory as the custom contract.

Nothing but Docker is needed on the host, and the compile takes about a second
once the image exists.

## The compiler pin

`COMPACT_COMPILER_VERSION` in `utils/compact/compact-compiler.ts` defaults to
**0.30.0**, and it is a pin rather than "latest" for a reason: compiled output
declares the `@midnight-ntwrk/compact-runtime` version it needs — 0.30.0 emits
`checkRuntimeVersion('0.15.0')` — and the `midnight-node-toolkit` image bundles
only a fixed set of those runtimes.

Which runtime the toolkit loads is not simply "the one in the image". Toolkit
2.x bundles several — one per compactc variant workspace, plus a hoisted root
copy — and resolves `compact-js` / `compact-runtime` imports through a hook
that picks a workspace from the **`COMPACTC_VERSION`** environment variable,
with no default. `ToolkitWrapper` therefore sets `COMPACTC_VERSION` from the
compiler pin; left unset, resolution falls through to the root copy and loading
the contract config dies with `Version mismatch: compiled code expects 0.15.0,
runtime is 0.18.0-rc.1`. Toolkit 1.x has no such hook and ignores the variable.

`ToolkitWrapper.assertCompactRuntimeSupported` then checks the image can supply
the version the freshly compiled contract declares, and how sharply depends on
whether the dispatch rule is known. On a 2.x image the `compact-<version>/`
workspace that `COMPACTC_VERSION` selects is authoritative, so its runtime is
compared directly. On 1.x, variants are keyed on the ledger version
(`/toolkit-js/v8`) and resolved internally by the toolkit; that rule is not
modelled, so the check only asks whether any tree carries the runtime.

The permissive fallback is deliberate — a false failure blocks a configuration
that works, whereas a missed one still shows up on the first call as
`Version mismatch: ...`.

(Variant directories are named inconsistently across releases: `/toolkit-js/v8`
on toolkit 1.x, `compact-0.30` on 2.0.x, `compact-0.30.0` on 2.1.x. The check
globs for the package rather than assuming a layout.)

### Toolkit 2.1.x is not usable yet

Setting `COMPACTC_VERSION` gets a 2.1.x image as far as loading the contract,
but `generate-txs` then rejects the intent: `expected header tag
'midnight:intent[v9]...', got 'midnight:intent[v6]...'`. Its Rust side wants a
ledger-v9 intent, which needs compactc 0.33.x — the only release in that
image's supported set that is **not** published (the public ladder is 0.29.0,
0.30.0, 0.31.0, 0.31.1, then 0.34.0, and 0.34.0 targets runtime 0.19.0, which
no workspace in the image provides). Until 0.33.x is reachable, run this suite
on node and toolkit `1.0.0`.

So when a toolkit bump drops 0.30.0, the run fails with an actionable message
and the fix is one environment variable:

```bash
COMPACT_COMPILER_VERSION=0.31.0 TARGET_ENV=undeployed bun run test:e2e
```

Once a new pin is confirmed to still produce the guaranteed/fallible split the
test relies on (the two fixture self-checks are exactly what proves that),
change the default.

Other overrides: `COMPACT_MANAGER_VERSION` pins the `compact` installer release,
and `COMPACT_TOOLCHAIN_IMAGE` points at a pre-built image instead of building
one.
