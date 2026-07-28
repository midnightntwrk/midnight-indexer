# Upgrading the Ledger

How to move the indexer to a new `midnight-ledger` release.

## Why this is fiddly

The indexer supports **two ledger majors at once**, so it can index chains that
straddle a protocol upgrade:

| Ledger | Protocol versions | Source |
| ------ | ----------------- | ----------------------------------- |
| v8     | `0.22`, `1.0`     | crates.io                           |
| v9     | `2.0`             | git tags (RCs not yet on crates.io) |

The mapping is `ProtocolVersion::ledger_version` in
`indexer-common/src/domain/protocol_version.rs`. Both stacks come in side by
side via suffixed aliases in `[workspace.dependencies]` - `midnight-ledger_v8`
(registry `"8"`) and `midnight-ledger_v9` (git).

## How the versions are pinned

RCs are **not on crates.io**, so v9 is wired through `[patch.crates-io]` in the
root `Cargo.toml`, each crate pinned to a git tag in
[`midnightntwrk/midnight-ledger`](https://github.com/midnightntwrk/midnight-ledger).
Two patch shapes, and the difference matters:

- **No `package`/`version`** (e.g. `midnight-base-crypto`, `midnight-serialize`,
  `midnight-zkir`) - replaces the registry crate *outright*; for crates shared
  within their own major, or v9-only ones.
- **With `package` + `version`** (e.g. `midnight-coin-structure_v3`,
  `midnight-onchain-runtime_v4`) - patches *only that major*, leaving v8's
  registry version intact. This is what lets the two coexist.

The comment block above `[patch.crates-io]` is the source of truth - read it
first.

## Steps

1. **Update the pins.** Bump each git `tag` in `[patch.crates-io]` to the new
   release set; if a major's number changed, bump its `[workspace.dependencies]`
   alias `version` too.

2. **Update the lockfile** for *only* those crates - never a bare `cargo
   update`:

   ```bash
   cargo update -p midnight-ledger-v9 -p midnight-zswap ...
   ```

3. **Build both features** (either can break alone):

   ```bash
   just feature=cloud check
   just feature=standalone check
   ```

4. **Fix domain code if the API moved.** Ledger types are consumed in
   `indexer-common/src/domain/ledger/` (`ledger_state.rs`, `transaction.rs`,
   `contract_state.rs`); changes surface as compile errors there.

5. **Regenerate fixtures if the wire format moved.** The binary fixtures
   `indexer-common/tests/{genesis_state,tx_1_2_2,tx_1_2_3}.raw` come from a
   node/toolkit built against the matching ledger:

   ```bash
   just generate-txs        # writes the .raw fixtures from a running node
   ```

   If the new ledger bumps wire tags ahead of the node that emits them (the
   common RC case), the affected e2e / round-trip tests are `#[ignore]`d until a
   node built against that ledger ships - note this in the PR.

6. **Run** `just all-all`.

## Adding a new ledger major

When a chain needs a third concurrent ledger:

1. Add suffixed aliases (`midnight-ledger_v10`, ...) in
   `[workspace.dependencies]`, plus `[patch.crates-io]` tags if needed.
2. Extend `LedgerVersion` and the `ProtocolVersion -> LedgerVersion` mapping in
   `protocol_version.rs`.
3. Add per-major match arms wherever ledger types are dispatched in
   `indexer-common/src/domain/ledger/`.

## See also

- [Upgrading the node version](./updating-node-version.md) - usually the same
  PR, since a protocol bump moves both.
- [Creating a release](./releasing.md)
