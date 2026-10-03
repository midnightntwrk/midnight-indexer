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

Three details are load-bearing, and all three were established empirically:

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

3. **The ballast keys are primed before the stale pair is built.** Each burn
   circuit has a matching `prime*` circuit that inserts the same 48 ballast
   keys, and the test applies it, against fresh state, before taking the
   snapshot. Without it the first call of the pair *inserts* those keys and
   grows the contract state between the snapshot the stale call is proven
   against and the state it is applied to. A transcript's declared gas is its
   cost × 1.2 measured at build time, **bytes written and deleted included**,
   and ledger v9 charges a transcript for the state bytes it rewrites and
   rejects it with `OutOfGas` above that bound
   (`onchain-runtime/src/context.rs`, `query`). On the grown state the stale
   call's three-op guaranteed transcript (`idxp`, `addi 1`, `insc 1`) runs out
   of its declared gas, and the node rejects the whole transaction from the
   mempool — `guaranteed execution would fail: ran out of gas budget`, surfaced
   as `INVALID_TRANSACTION … custom error: 104` — before any partial success
   can happen. With the keys primed, both calls overwrite existing entries, so
   the guaranteed transcript costs the same against the snapshot and against
   the state it applies to. Ledger v8 accepted the unprimed pair; priming is
   harmless there.

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

1. `utils/compact/compact-compiler.ts` builds `compact-toolchain:<version>-<digest>`
   from `utils/compact/compact-toolchain.Dockerfile` (first use only; a Docker
   image cache hit afterwards), which installs the pinned compactc release. The
   digest covers the Dockerfile and the toolchain-manager pin, so editing either
   builds a fresh image rather than reusing one cached under the same version.
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

### Pinning a pre-release

`COMPACT_COMPILER_VERSION` accepts a pre-release such as `0.33.0-rc.2`. The
toolchain manager only offers stable releases — `compact list` jumps from
0.31.1 to 0.34.0 — so the toolchain image falls back to fetching the
pre-release archive from the compiler repo's GitHub releases, which has the
same layout the manager unpacks. This matters because a newer toolkit can
require a runtime no stable compiler emits: toolkit 2.1.x wants compact-runtime
`0.18.0-rc.1`, which only compactc 0.33.x produces (0.30.0 → 0.15.0, 0.31.0 →
0.16.0, and the next stable, 0.34.0 → 0.19.0, overshoots).

### Status on toolkit 2.1.x

Usable with the pre-release compiler pin:

```bash
NODE_TAG=2.1.0-rc.2 NODE_TOOLKIT_TAG=2.1.0-rc.2 \
  COMPACT_COMPILER_VERSION=0.33.0-rc.2 TARGET_ENV=undeployed bun run test:e2e
```

runs both scenarios green. The default pin (0.30.0) does not work there: it
emits a ledger-v8 intent, and the 2.1.x toolkit refuses to read it
(`expected header tag 'midnight:intent[v9](…)', got 'midnight:intent[v6](…)'`).

Before the ballast was primed (detail 3 above), `burnWithGuaranteed` failed on
2.1.x with `custom error: 104`. That was first put down to the ballast being
tuned for the ledger-v8 guaranteed budget, but decoding the stale call on
ledger v9 shows the split is as intended — only the increment is guaranteed —
and the node's own log names the cause: the guaranteed transcript ran out of
its declared gas on the state the first call had grown.

The ballast size is still a hand-picked constant. Ledger v9 partitions it as
intended today, but a future cost model may not; a calibration-aware fixture
that derives the split at runtime instead is tracked separately.
