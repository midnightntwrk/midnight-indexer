# Upgrading the Ledger

How to move the indexer to a new `midnight-ledger` release.

## Why this is fiddly

The indexer supports **two ledger majors at once**, so it can index chains that
straddle a protocol upgrade:

| Ledger | Protocol versions | Source                                    |
| ------ | ----------------- | ----------------------------------------- |
| v8     | `0.22`, `1.0`     | crates.io                                 |
| v9     | `2.0`, `2.1`      | crates.io, plus git tags the node patches |

The mapping is `ProtocolVersion::ledger_version` in
`indexer-common/src/domain/protocol_version.rs`. Both stacks come in side by
side via suffixed aliases in `[workspace.dependencies]` - e.g.
`midnight-ledger_v8` (`midnight-ledger`) and `midnight-ledger_v9`
(`midnight-ledger-v9`). Crates that changed major between the two
(`coin-structure`, `onchain-runtime`, `onchain-state`, `storage`,
`transient-crypto`, `zswap`) get one alias per major; `base-crypto`,
`serialize` and `storage-core` are shared.

## How the versions are pinned

Every ledger crate in `[workspace.dependencies]` carries an `=` pin to the
version the matching
[`midnightntwrk/midnight-node`](https://github.com/midnightntwrk/midnight-node)
release resolves in its `Cargo.lock`. `v8-to-v9-state-translation` in
`indexer-common/Cargo.toml` is a git dependency, pinned to the same
[`midnightntwrk/midnight-ledger`](https://github.com/midnightntwrk/midnight-ledger)
tag as the node. When the node patches a ledger crate that is not yet on
crates.io to a git tag, the root `Cargo.toml` carries the same entry under
`[patch.crates-io]`. The fork-boundary block compares the arena root the
indexer derives against the node's, so any drift from the node breaks that
block.

The comment block above the ledger aliases in the root `Cargo.toml` is the
source of truth - read it first.

## Steps

1. **Update the pins.** Set each ledger alias's `=` version, the
   `v8-to-v9-state-translation` tag and the `[patch.crates-io]` entries to what
   the matching node release uses. Remove a patch entry once the node drops it.

2. **Update the lockfile** for *only* those crates - never a bare `cargo
   update`:

   ```bash
   cargo update -p midnight-ledger-v9 -p midnight-zswap ...
   ```

   Then compare every `midnight-*` and `v8-to-v9-state-translation` entry
   against the node's `Cargo.lock`. Crates the workspace depends on only
   transitively (e.g. `midnight-ledger-static`, `midnight-zkir`) are pinned by
   the lockfile alone.

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

6. **Re-sync the v8 → v9 state-translation table** if the node's table changed.
   `indexer-common/src/domain/ledger/state_translation_v8_to_v9.rs` is a manual
   re-port of the node's `ledger/helpers/src/state_translation_v8_to_v9.rs` and
   must stay semantically identical to it — the ledger-8→9 fork-boundary block
   re-derives `ledger_state.root()` and compares it to the node's on-chain
   migration output, so any drift breaks that block permanently. If the node's
   table or the v9 `INITIAL_PARAMETERS` it seeds change:

   - re-port the file and keep the provenance header's node commit current, and
   - regenerate the golden-root fixture
     `indexer-common/tests/golden_v8_to_v9_empty_root.raw` (currently a
     self-consistency baseline; capture the node's authoritative
     `ledger_state_root` at `apply + 1` once the 2.1.0 fork is runnable).

   Drift is caught by `test_translate` (characterization guard) and the ported
   module's `table_is_closed` / `table_tags_match_types` tests.

7. **Run** `just all-all`.

## Adding a new ledger major

When a chain needs a third concurrent ledger:

1. Add suffixed aliases (`midnight-ledger_v10`, ...) in
   `[workspace.dependencies]`.
2. Extend `LedgerVersion` and the `ProtocolVersion -> LedgerVersion` mapping in
   `protocol_version.rs`.
3. Add per-major match arms wherever ledger types are dispatched in
   `indexer-common/src/domain/ledger/`.

## See also

- [Upgrading the node version](./updating-node-version.md) - usually the same
  PR, since a protocol bump moves both.
- [Re-indexing from genesis](./re-indexing.md) - this guide assumes both stores
  survive the upgrade, which is the normal case; that one covers the changes
  where they cannot.
- [Creating a release](./releasing.md)
