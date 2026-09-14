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

## Regenerating the compiled output

The compiled artefacts are committed because the test environment has no Compact
toolchain. To rebuild them:

```bash
compact update 0.30.0
~/.compact/versions/0.30.0/*/compactc.bin segment-split.compact managed
```

**Use compactc 0.30.0, not a later release.** The compiler must emit code for
the Compact runtime that the node toolkit bundles: 0.30.0 targets runtime
`0.15.0` and `ledger-8.0.2`, which is what `/toolkit-js/v8` inside the
`midnight-node-toolkit` image provides. Compiling with 0.31.0 produces
`checkRuntimeVersion('0.16.0')` and the toolkit fails to load the contract with
`Version mismatch: compiled code expects 0.16.0, runtime is 0.15.0`.
