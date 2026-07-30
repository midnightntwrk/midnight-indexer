# Following the Node Through the Ledger 8 → 9 Hard Fork

How the indexer stays in lockstep with the node across the in-place ledger v8 → v9
hard fork.

This is the indexer design for **IDX-01**
([midnight-indexer#1282](https://github.com/midnightntwrk/midnight-indexer/issues/1282)),
part of the SOW-Q3 hard-fork migration
([shieldedtech/product#119](https://github.com/shieldedtech/product/issues/119)).
The node side is implemented in
[midnight-node#1925](https://github.com/midnightntwrk/midnight-node/pull/1925).

## TL;DR

The indexer is already ~90% ready: it carries the v8 and v9 ledger dependency trees
side by side, maps the node's `spec_version` to a `LedgerVersion`, and version-dispatches
every decode path (groundwork from
[#1346](https://github.com/midnightntwrk/midnight-indexer/pull/1346) and
[#1377](https://github.com/midnightntwrk/midnight-indexer/pull/1377)). Two things block a
live in-place 8 → 9 fork:

1. **The post-fork runtime is rejected.** The migration runtime ships `spec_version =
   2_001_000`, which `ProtocolVersion::try_from` treats as `Unsupported` on `main`. Recognizing
   it is **already in flight in
   [#1333](https://github.com/midnightntwrk/midnight-indexer/pull/1333)** (Phase 1), which adds a
   `V2_1` protocol version and a dedicated `v2_1_0` runtime module — not a widening of `V2_0`.
2. **`LedgerState::translate(V8 → V9)` is a stub** that returns
   `UnsupportedLedgerStateTranslation`. This is the single hook the fork hinges on, and its
   output must match the node's on-chain migration **byte for byte**.

The plan: implement `translate` by **re-porting the node's v8 → v9 translation table** into
`indexer-common`, and **validate it against a live fork** — a real node driven through the
upgrade — so byte-exactness is *measured against the node*, never reasoned about. This is
**independent of the post-fork runtime recognition (#1333)**: `translate` is keyed on
`LedgerVersion`, not `ProtocolVersion`, so it — and its golden-root test — land on their own,
directly on `main`. #1333 is required only to cross the fork *live* against the real `2.1.0`
runtime, i.e. so a `spec_version = 2_001_000` block is classified V9 and reaches `translate` at
all.

**The oracle is a live fork.** Validation is anchored on the fork overlay from
[#1364](https://github.com/midnightntwrk/midnight-indexer/pull/1364)
(`docker-compose.midnight-fork.yaml` + `docs/running-against-a-fork.md`, currently on branch
`feat/midnight-fork-overlay`, not yet on `main`). We clone a ledger-8 network — ideally from a
**mainnet snapshot** — start it on the existing `1.0.x` image, then force the runtime upgrade to
the `2.1.0` migration WASM via governance votes and watch the indexer cross `apply + 1`. The
node's own post-migration root is the ground truth for `translate`; the same run captures the
golden-root fixture. Because the overlay lives on its own branch, run it from a **git worktree**
while `translate` is implemented on a feature branch off `main`.

## Status & handoff (2026-07-30)

**Done** — on branch `feat/idx01-translate-v8-to-v9`:

- **Phase 2 — `translate(V8 → V9)` implemented.** The node's v8→v9 translation table is re-ported
  verbatim into `indexer-common/src/domain/ledger/state_translation_v8_to_v9.rs` (provenance:
  midnight-node #1925 @ `74a91156`; table is byte-identical to the node's), wired into
  `LedgerState::translate`, and driven to completion mirroring the node's one-shot host call.
- **Decision 2 resolved.** `midnight-storage` (`state-translation`) + `onchain-state` v8/v9 added;
  the framework compiles and shares the arena over `v1_1::LedgerDb`.
- **Decision 3.2 — golden root node-validated.** Over a *populated* ledger-8 devnet state
  (`node-0.22.0` genesis, `ledger-state[v13]`), the re-ported table produces the same v9 arena
  root as the node's own `StateTranslationTable` (midnight-node `fc39e708`, the 2.1.0 migration
  runtime), byte-for-byte. Frozen in `indexer-common/tests/*.raw`; `test_translate` asserts it.
- All gates green (check cloud+standalone, tests, `clippy -D warnings`, fmt).

**Next owner — the live local-environment crossing (Phase 3).** Two steps, in order:

1. **Recognize the post-fork runtime (Phase 1) — HARD PREREQUISITE, not yet done.** Until the
   indexer accepts `spec_version 2_001_000` *and* can subxt-decode the 2.1.0 envelope,
   chain-indexer rejects every post-fork block, so no live crossing is possible. This is the work
   of **[#1333](https://github.com/midnightntwrk/midnight-indexer/pull/1333)**: `ProtocolVersion::V2_1`
   + a **dedicated** `v2_1_0` subxt runtime module (the 2.1.0 envelope added `pallet_session`, so
   `v2_0_0` can *not* be reused — adding `NodeVersion::V2_1` forces new arms across
   `chain-indexer/src/infra/subxt_node/runtimes.rs`) + `.node/2.1.0/metadata.scale` + a
   `NODE_VERSIONS` bump. Land #1333 as-is if it's ready; otherwise build the module from the
   node's committed `metadata/static/midnight_metadata_2.1.0.scale`.
2. **Run the local-environment fork (Phase 3).** With step 1 in place, fork a ledger-8 network
   with the #1364 overlay (`just fork-up`) and drive the in-place upgrade to the 2.1.0 migration
   runtime with the node's `local-environment/` `full-upgrade` command, then assert the indexer
   crosses `apply + 1` with matching state/zswap roots and an agreeing from-genesis resync. This
   closes the one gap the golden-root fixture doesn't cover: the runtime `on_runtime_upgrade` +
   RPC `ledger_state_root` path end-to-end. **See Phase 3 for the exact `fork-up` + `full-upgrade`
   command sequence, the image-vs-WASM host-fn caveat, and the overlay/upgrade composition to
   resolve.**

**No published `2.1.0` image is required.** A node checkout at the migration runtime
(`spec_version 002_001_000`; e.g. midnight-node `fc39e708`) builds the node and already carries
`metadata/static/midnight_metadata_2.1.0.scale` for the subxt codegen and the ledger-8-era v8
genesis blobs (`git show node-0.22.0:res/genesis/genesis_state_*.mn`) used to seed a fork.

## Background: how the node forks

Everything the indexer must track about the fork comes down to five facts (all from
midnight-node#1925):

- **The trigger is `spec_version`, carried per block.** `pallet-version` writes the runtime
  `spec_version` into every block header as an `MNSV` consensus digest. There is *no* fork
  height. The ledger-version boundary is `spec_version >= 2_000_000`.
- **The first ledger-9 block is `apply + 1`.** At the runtime-upgrade "apply" block the
  stored WASM flips, but that block still *executes* under the old runtime, so its `MNSV`
  digest is still ledger-8. The first block that executes the new runtime, carries the new
  `MNSV`, and is ledger-9-classified is the one *after* apply.
- **The state migration is not a transaction.** It runs in Executive's `on_runtime_upgrade`
  (`pallet_midnight::migrations::v2::MigrateV1ToV2`) at the start of `apply + 1`, before that
  block's extrinsics. There is no extrinsic and no event to attribute the state change to —
  so replaying transactions alone cannot reproduce it.
- **What the migration rewrites** (`LedgerState` `ledger-state[v13]` → `[v18]`):
  - `bridge_receiving` map re-annotated `SizeAnn` → `NightAnn`;
  - `LedgerParameters` gains `min_block_price` and `TransactionLimits.max_contract_metadata_size`,
    and the cost model drops `parallelism_factor` and adds `validation`/`guaranteed`/`fallible`
    factors — all **seeded from the `ledger_v9::structure::INITIAL_PARAMETERS` compile-time
    constant** (the node's table marks each "placeholder; the production value should match the
    value chosen for the hardfork"), never derived from v8 values;
  - `ContractOperation` single verifier key → `{ v2, v3, ir }` (old key → `v2`, `v3`/`ir` empty);
  - `ContractMaintenanceAuthority` committee keys wrapped as `Schnorr(vk)`;
  - everything else (zswap, utxo, dust, treasury, replay-protection, unclaimed rewards) is
    tag-stable and passes straight through `recast`.
- **Wire tags flip at the boundary — and so does the Substrate envelope.** The inner *ledger*
  bytes change tag: transactions go `transaction[v9]/signature[v1]` →
  `transaction[v12]/signature[v2]`; the system-tx enum gains `UnlockToTreasury`,
  `DistributeReserve` changes tuple → struct, and `pay_block_rewards_to_treasury` no longer
  appears. Contrary to an earlier assumption, the **Substrate envelope is *not* unchanged** for
  the runtime that carries the fork: the node's `2.1.0` runtime also bundles `pallet_session`
  (plus `c2m_bridge` / `cnight_observation`), so subxt metadata does **not** decode against the
  `2.0.0` codegen — a dedicated `v2_1_0` runtime module is required (done in #1333), not a reuse
  of `v2_0_0`.

## How the indexer already follows the node

- **Dual ledger dependencies** (`Cargo.toml`): `midnight-ledger_v8` (crates.io) and
  `midnight-ledger-v9` (git tag `crate-ledger-9.1.0.0-rc.3`), sharing `serialize` /
  `base-crypto` / `storage-core` within-major.
- **`spec_version` → `LedgerVersion` mapping** in
  `indexer-common/src/domain/protocol_version.rs`: `V2_0 → LedgerVersion::V9`.
- **Content-vs-state runtime split** (#1346) in `chain-indexer/src/infra/subxt_node.rs`:
  block *contents* are decoded with the `MNSV` content version, while *state/RPC* reads use
  `block.spec_version()`. This is exactly the separation an enactment block needs, plus an
  `IncompatibleCodegen → retry-at-parent` guard in `runtimes/v2_0_0.rs`.
- **Version-switched decoders** for transaction, system-tx, ledger-state, contract-state, and
  zswap-root already exist in `indexer-common/src/domain/ledger/*`.
- **Schema is ledger-version-agnostic**: blocks and ledger-state rows store raw bytes plus an
  integer `protocol_version`; v8 and v9 rows coexist, and per-key retention/GC already tracks
  the ledger version. **No SQL migration is required.**

## The gaps

### Gap 1 — the post-fork runtime is not recognized

An in-place fork lands a `1.0.x` (ledger-8) chain directly on the migration runtime,
`spec_version = 2_001_000`. On `main`, `ProtocolVersion::try_from` accepts only
`2_000_000..2_001_000` (exclusive), and the unit test in `protocol_version.rs` asserts that
`2_001_000` is `Unsupported`, so the indexer cannot classify the post-fork blocks.

**This gap is closed by [#1333](https://github.com/midnightntwrk/midnight-indexer/pull/1333)**
(in flight): it adds a `ProtocolVersion::V2_1(u32)` variant over `2_001_000..2_002_000` mapping
to `LedgerVersion::V9` / `NodeVersion::V2_1`, a dedicated `runtimes/v2_1_0.rs` module with its
own subxt codegen, `.node/2.1.0/metadata.scale`, and the `NODE_VERSIONS` bump. Phase 2 neither
re-does nor depends on this: `translate` is keyed on `LedgerVersion`, so it lands independently;
#1333 is only what lets a live `2_001_000` block be classified V9 and reach `translate` in the
first place.

### Gap 2 — `translate(V8 → V9)` is a stub, and must be byte-exact

`indexer-common/src/domain/ledger/ledger_state.rs`:

```rust
pub fn translate(self, ledger_version: LedgerVersion) -> Result<Self, Error> {
    match (self, ledger_version) {
        (s @ LedgerState::V8 { .. }, LedgerVersion::V8) => Ok(s),
        (s @ LedgerState::V9 { .. }, LedgerVersion::V9) => Ok(s),
        (LedgerState::V8 { .. }, LedgerVersion::V9) => Err(
            Error::UnsupportedLedgerStateTranslation(LedgerVersion::V8, LedgerVersion::V9),
        ),
        (LedgerState::V9 { .. }, LedgerVersion::V8) => Err(
            Error::BackwardsLedgerStateTranslation(LedgerVersion::V9, LedgerVersion::V8),
        ),
    }
}
```

It is called for **every non-genesis block** in `chain-indexer/src/application.rs` (the `else`
branch of the genesis check):

```rust
let ledger_version = block.protocol_version.ledger_version();
ledger_state = if block.height == 0 {
    LedgerState::new(network_id.clone(), ledger_version)?   // genesis: seed fresh
} else {
    ledger_state.translate(ledger_version)?                 // every other block
};
```

At `apply + 1` the indexer holds an in-memory **V8** state (built by replay through the apply
block) and the block reports **V9**, so `translate` fires exactly once and currently errors —
the indexer stalls and cannot advance past the fork. This real V8 → V9 work happens only on the
in-place `1.0.x → 2.1.0` path; a chain already at ledger-9 (`2.0.0`, `spec 2_000_000`) upgrading
to `2.1.0` holds a V9 state already, so its boundary is a `V9 → V9` no-op — matching the node's
host fn, which no-ops when the state is already v9.

**The exactness constraint:** `application.rs` re-derives `ledger_state.root()` and compares it
to the node's `ledger_state_root` **at every block** (the check sits *outside* the genesis
branch), and also validates the zswap Merkle root. So `translate`'s output arena root must
equal the node's post-migration root bit-for-bit — including the placeholder v9 parameters.
Anything short of bit-identity fails the boundary block permanently. **This is precisely what the
live-fork oracle measures** (see Validation): we don't guess the target root, we read it off the
node at `apply + 1`.

## Design decisions

### Decision 1 — re-port the translation table into the indexer

The concrete v8 → v9 translation table exists today **only** in the node repo, at
`ledger/helpers/src/state_translation_v8_to_v9.rs` (~710 lines, itself ported from
midnight-ledger PR #539), inside the `publish = false` crate `midnight-node-ledger-helpers`.
The published ledger/storage crates expose only the **generic** framework
(`TranslationTable`, `TypedTranslationState`, `TaggedTranslationState`, `TranslationCache` in
`midnight-storage`'s `state_translation` module) — not the concrete table.

We will **copy the node's table into `indexer-common`** (e.g.
`indexer-common/src/domain/ledger/state_translation_v8_to_v9.rs`), remapping the crate aliases
onto the indexer's package aliases, exactly as the node did onto its own. This unblocks the
indexer with no external dependency.

> **The cost of this decision:** the node and indexer copies must stay bit-identical forever.
> If either side's table drifts, the boundary block's root check breaks. Decision 3 (the
> live-fork oracle plus its frozen golden-root guard) is the mandatory mitigation and is not
> optional.

### Decision 2 — pull the translation framework + onchain-state deps *(the one gate the live fork can't answer)*

The re-ported table imports symbols the indexer does not depend on directly yet:

- The `state_translation` framework and `merkle_patricia_trie` / `storable::SizeAnn` from the
  **full `midnight-storage`** crate, with its **`state-translation` feature** enabled. The
  indexer today depends on `midnight-storage-core` (via `v1_1::LedgerDb`), not the wrapper
  `midnight-storage`, so the wrapper must be added as a direct dep. The node declares it as
  `midnight-storage =2.0.1` with `features = ["parity-db", "state-translation"]` — the same
  version already in the indexer's `Cargo.lock`, confirming the feature exists.
- `midnight-onchain-state` for **both** majors (the node aliases these
  `onchain-state-ledger-8 =3.0.0` / `onchain-state-ledger-9 =4.0.0`). Both already resolve
  transitively in the indexer's `Cargo.lock` (3.0.0 from crates.io via ledger v8; 4.0.0 from the
  `[patch.crates-io]` rc tag); this step just declares direct workspace aliases.

**This is the single risk a live fork does *not* de-risk** — it is a pure build-graph question,
answered by compiling, not by running a node. The composition is *largely* pre-validated: the
lockfile carries a *single* `midnight-storage-core 1.2.0`, and the wrapper `midnight-storage
2.0.1`, `midnight-ledger-v9` (rc.3), and the indexer's `v1_1::LedgerDb` all sit on it — so the v9
`LedgerState` and the translation's arena resolve to the same storage instance (the node relies
on exactly this "one arena" property).

> **Spike result — RESOLVED.** The narrow spike was run: declare `midnight-storage =2.0.1`
> (`features = ["state-translation"]`) as a direct dep and drive
> `TypedTranslationState::<LedgerStateV8, LedgerStateV9, _, v1_1::LedgerDb>::start(..).run(..).result()`
> over the framework. `cargo check -p indexer-common --features cloud` **compiles cleanly**,
> proving (a) `v1_1::LedgerDb` satisfies `midnight_storage::db::DB` — the wrapper's `DB` is the
> same trait storage-core exposes (`pub use storage_core::*`), so the arena is shared — and (b)
> `LedgerStateV8/V9<v1_1::LedgerDb>` are `Storable` over that arena, so the whole driver
> monomorphizes. The `run(budget)`-to-completion drain loop typechecks too. Enabling
> `state-translation` cost only ~25 s incremental: the ledger crates depend on `storage-core`
> *without* that feature, so feature unification rebuilt only `midnight-storage` +
> `storage-core` + `indexer-common`, leaving the heavy ledger tree untouched. Only the direct
> `midnight-storage` dep (two manifest lines) is new; `onchain-state` v8/v9 already resolve
> transitively and become direct aliases only when the real table needs them.

### Decision 3 — the live fork is the oracle; a frozen golden-root guards drift (mandatory)

Because Decision 1 duplicates code that must match the node bit-for-bit, correctness is
*measured against the node*, not argued:

1. **The live fork is the source of truth.** The #1364 overlay drives a real node through the
   `1.0.x → 2.1.0` upgrade (Validation, below). At `apply + 1` the node emits its own
   post-migration `ledger_state_root`; the indexer's re-derived root must equal it bit-for-bit.
   This is the definitive check — it exercises the *actual* migration against a real (ideally
   mainnet-snapshot) state, so there is no "did we reason about the root correctly" gap.
2. **A frozen golden-root fixture is the always-on guard.** We check a serialized ledger-8
   `LedgerState` plus the expected post-migration ledger-9 arena root into `indexer-common/tests/`
   (alongside the other `*.raw` fixtures). A fast, hermetic unit test runs the re-ported
   `translate` over the v8 state and asserts the root equals the golden value — so CI catches
   table drift on every commit **without** needing a running node.

   **Status — node-validated (2026-07-29).** This is done, ahead of the live overlay. Using a
   populated ledger-8 devnet genesis (`ledger-state[v13]`, extracted from `node-0.22.0`), the
   re-ported table produces the arena root
   `005fe7719bfd91ba5c126e09a43e9d0c721b1612e33f9f18dbe15b6e7597b89afc`, and the **node's own
   `StateTranslationTable`** (built from `midnight-node` `fc39e708`, the 2.1.0 migration runtime)
   produces the byte-identical root over the same blob. So the fixture is authoritative for the
   translation itself — the node table and the port are proven equal over real populated state
   (contracts, bridge, treasury MPTs), not just structurally. Fixtures:
   `indexer-common/tests/{v8_genesis_devnet_0_22_0,golden_v8_to_v9_devnet_root,golden_v8_to_v9_empty_root}.raw`;
   the `test_translate` unit test asserts both roots. The one piece still owed to the live overlay
   is the `apply + 1` RPC-format / end-to-end crossing (Phase 3) — the state-translation
   correctness, the risky part, is now node-validated.
3. **A provenance header + re-sync note.** The re-ported file must carry a header naming the
   exact node source (`midnight-node ledger/helpers/src/state_translation_v8_to_v9.rs`) and the
   commit it was copied from (currently `74a91156`, the merge of node#1925), and
   `docs/upgrading-ledger.md` must gain a line: *"if the node's v8→v9 translation table changes,
   re-run the fork overlay to regenerate the golden-root fixture and re-sync
   `state_translation_v8_to_v9.rs`."*

## Implementation plan

### Phase 0 — prerequisites & validation harness
- Node `2.0.0` final and the migration runtime `2.1.0` published as images (indexer pins node
  images via `NODE_VERSIONS`).
- Bump the v9 `[patch.crates-io]` tags from `-rc.*` to final release tags per
  `docs/upgrading-ledger.md` (drop the patch entries if v9 reaches crates.io).
- **Stand up the #1364 fork overlay as the validation harness.** Check out
  `feat/midnight-fork-overlay` into a **git worktree** (it is not on `main` and is just
  `docker-compose.midnight-fork.yaml` + `docs/running-against-a-fork.md`), so the overlay can be
  driven independently of the `translate` feature branch. Confirm it can clone a ledger-8 network
  — ideally a **mainnet snapshot** — boot it on the `1.0.x` image, and accept a governance-driven
  runtime upgrade to the `2.1.0` migration WASM.

### Phase 1 — recognize the post-fork runtime  *(in flight in #1333)*
Landed by [#1333](https://github.com/midnightntwrk/midnight-indexer/pull/1333). The **live**
boundary crossing depends on it; Phase 2's `translate` does **not** (see Phase 2). What it does:
- Adds a `ProtocolVersion::V2_1(u32)` variant over `2_001_000..2_002_000` mapping to
  `LedgerVersion::V9` / `NodeVersion::V2_1`, and updates the `protocol_version.rs` unit test that
  asserted `2_001_000` is `Unsupported`.
- Adds node `2.1.0` to `NODE_VERSIONS`, `.node/2.1.0/metadata.scale`, and a **dedicated**
  `runtimes/v2_1_0.rs` module with its own subxt codegen. The earlier assumption that `v2_0_0`
  could be reused does **not** hold: the `2.1.0` runtime adds `pallet_session` (plus
  `c2m_bridge` / `cnight_observation`), changing the Substrate envelope.
- **Coordination check — now answered by construction:** confirm #1333's
  `.node/2.1.0/metadata.scale` was generated from a node build that includes node#1925's
  migration (that PR merged 2026-07-29; #1333 may predate it). A clean crossing on the live-fork
  overlay *is* that confirmation — if the metadata were stale, the overlay would fail to decode
  the boundary block.

### Phase 2 — implement `translate(V8 → V9)`  *(core of IDX-01; independent of #1333)*
`translate` is driven by `LedgerVersion`, not `ProtocolVersion`, so every step below lands on
plain `main` with no dependency on #1333 — including the golden-root test, which is a pure
function of a serialized v8 state. (On `main`, `2_000_000..2_001_000` already maps to V9, so a
V8 → V9 boundary at spec `2_000_000` would even exercise `translate` live today; #1333 only adds
recognition of the real fork's `2_001_000` runtime.)

1. **Compile/composition spike first (Decision 2) — DONE.** Adding `midnight-storage`
   (`features = ["state-translation"]`, `parity-db` not needed — the indexer's backend is
   `v1_1::LedgerDb`, not ParityDb) and driving `TypedTranslationState` over `v1_1::LedgerDb`
   compiles cleanly (see Decision 2's spike-result note). `midnight-onchain-state` v8/v9 need
   direct workspace aliases only when the re-ported table references them; both already resolve
   transitively. This gate — the only step no live-fork run can substitute for — is cleared.
2. Re-port `state_translation_v8_to_v9.rs` into `indexer-common` (Decision 1) with the provenance
   header citing node#1925 @ `74a91156` (Decision 3.3). Keep the v9 parameter seeding wired to
   `ledger_v9::structure::INITIAL_PARAMETERS` so it tracks the node's chosen values.
3. Replace the `Err(UnsupportedLedgerStateTranslation)` arm in `ledger_state.rs` with a real
   translation that drives the `state_translation` framework over the shared arena
   (`default_storage::<v1_1::LedgerDb>()`) and persists the v9 root, mirroring the node's
   `ledger/src/host_api/migration_8_to_9.rs`. Mirror how the node drains `run(budget)` **to
   completion** — a partial drain silently yields a wrong root, which the live-fork boundary check
   and the golden-root test both catch. Add an **idempotency guard**: the node's host fn no-ops on
   an already-v9 state (the `2.0.0 → 2.1.0` path), which on the indexer side is the existing
   `V9 → V9 ⇒ Ok(s)` arm.
4. Update the passthrough in `chain-indexer/src/domain/ledger_state.rs` and flip the
   negative-assertion test at `ledger_state.rs:~2604` (`ledger_state.translate(V9).is_err()`) to a
   positive round-trip once the real translation lands.

### Phase 3 — validation against the live fork  *(the acceptance spine)*
QA acceptance: *"wallets, indexers, dapp-connector clients resync across the fork without
divergence; a fresh wallet fully syncs from genesis post-fork."* The #1364 overlay is how we
prove it. All of the following run against the harness from Phase 0:

- **Golden-root fixture + unit test (Decision 3.2) — already done, node-validated.** The fixture
  is captured and cross-checked against the node's own table over a populated state, and
  `test_translate` asserts it in CI (see the Status & handoff and Decision 3.2 notes). The live
  crossing below no longer needs to *produce* the fixture — it validates the remaining runtime
  `on_runtime_upgrade` + RPC `ledger_state_root` path end-to-end.
- **8 → 9 boundary e2e — the missing coverage, with the concrete runbook.** Existing
  runtime-upgrade tests (`chain-indexer/tests/mainnet_runtime.rs`, `qa/scripts/test-runtime-upgrade.sh`)
  only cross *same-ledger* boundaries. The 8→9 crossing uses the #1364 overlay to run the indexer
  and the node's `local-environment/` **upgrade** commands to drive the fork:

  0. **Prerequisites.**
     - An indexer image that recognizes `2_001_000` (Phase 1 / #1333) — otherwise chain-indexer
       refuses every post-fork block. Build local images with `INDEXER_TAG=dev just
       build-docker-image chain-indexer` (and wallet-indexer / indexer-api).
     - A node checkout that has the upgrade commands (`full-upgrade`,
       `governance-runtime-upgrade`) — point `MIDNIGHT_NODE_DIR` at a 2.1.0-era checkout so
       `fork-up` uses that tooling instead of sparse-cloning an older tag.
     - The **2.1.0 migration node image** *and* the **2.1.0 runtime WASM**, both built from that
       checkout. **The image matters:** the migration's new host fn `migrate_state_v8_to_v9` lives
       in the node *binary*, so the node image must be rolled to 2.1.0 — a WASM-only upgrade
       against a `1.0.x` binary would miss the host fn and abort. Place the WASM under the node
       repo's `local-environment/artifacts/` (the `--wasm` path resolves there).
     - A **ledger-8 (`1.0.x`) snapshot** to fork from.

  1. **Fork a ledger-8 network and attach the indexer** (indexer repo, `feat/midnight-fork-overlay`):
     ```bash
     MIDNIGHT_NODE_DIR=/path/to/midnight-node \
     NODE_IMAGE=ghcr.io/midnight-ntwrk/midnight-node:1.0.x \
     INDEXER_TAG=dev just fork-up <network> --from-snapshot <ledger-8-snapshot-url>
     ```

  2. **Drive the in-place 8 → 9 upgrade** against that running fork (from
     `<midnight-node>/local-environment/`). `full-upgrade` = image rollout (`1.0.x` → `2.1.0`
     node) **then** the federated-authority runtime upgrade to the 2.1.0 WASM, which fires
     `pallet_midnight::migrations::v2::MigrateV1ToV2` at `apply + 1`:
     ```bash
     NODE_IMAGE=ghcr.io/midnight-ntwrk/midnight-node:1.0.x \
     NEW_NODE_IMAGE=<2.1.0 migration image> \
     npm run full-upgrade:<network> -- \
       --wasm upgrade/midnight_node_runtime.compact.wasm \
       --council-uris //Dave //Eve //Ferdie \
       --technical-uris //Alice //Bob //Charlie \
       --executor-uri //Alice
     ```
     The 2.1.0 WASM bumps `spec_version` to `2_001_000`, so do **not** pass `--allow-same-version`
     (that flag is only for same-spec local rehearsals).

  3. **Assert the crossing.** chain-indexer must reach `apply + 1` with its re-derived
     `ledger_state.root()` matching the node's RPC `ledger_state_root`, and the zswap root
     matching — the per-block checks in `application.rs`. Tear down with `just fork-down <network>`
     (overlay first) then `npm run stop:<network>`.

  > **Open integration point for the next owner:** both `just fork-up` and `full-upgrade` can
  > restore a snapshot, so decide the composition — e.g. let `fork-up` restore + attach the
  > indexer, then run `image-upgrade:<network>` + `governance-runtime-upgrade:<network>` (which
  > *reuse* the running fork) instead of a second `full-upgrade` restore. The overlay (#1364) and
  > the node's upgrade commands were built independently and have not yet been exercised together
  > across the 8→9 boundary; wiring that is this phase's real work.
- **Fresh-from-genesis resync.** Assert a from-genesis resync of the post-fork chain agrees
  block-for-block with the incrementally-indexed result — the direct QA acceptance criterion.
- **Resume / reorg across the boundary.** Kill the indexer at `apply + 1` and restart; confirm it
  re-seeds `ledger_state` at the correct version (V8 → re-translate deterministically; V9 → the
  no-op arm) and still matches. Exercise a reorg spanning the boundary. These become *tests you
  run on the oracle*, not properties argued on paper.
- Regenerate binary tx/state fixtures (`indexer-common/tests/*.raw`) as needed.

### Phase 4 — version-awareness cleanup
- Replace the `LedgerVersion::LATEST` hardcode in the dust queries
  (`indexer-api/src/infra/api/v4/query.rs`, `get_dust_generation_status` /
  `get_dust_generations`) with a per-chain/per-block-derived version — the follow-up already
  flagged in `protocol_version.rs`. Can be split into its own PR to keep the boundary-crossing
  change focused.

### Phase 5 — Dust-reset event  *(IDX-02, separate; gated on the node's Dust Fix)*
The fork is *planned* to bundle a Dust-balance reset security fix
([shielded-security-engineering#547/548](https://github.com/shieldedtech/shielded-security-engineering/issues/547)),
whose indexer part is [IDX-02 (#551)](https://github.com/shieldedtech/shielded-security-engineering/issues/551):
expose a "Dust Reset" event/subscription so wallets can zero their local dust state. **The
already-merged node migration (#1925) does not reset dust**, and the Dust Fix is currently
delayed behind higher-priority ledger security work, so this is *not* required to follow the
current node. Track it, build it only once the node's Dust Fix migration lands.

### Phase 6 — rollout
Ship indexer builds that recognize `2_001_000` **before** any environment forks, so the
indexer never hits an `Unsupported` spec_version at the boundary. Follow the node's rollout
order (`devnet → qanet → preview → preprod → mainnet`) with a mainnet-snapshot rehearsal — the
same overlay used in Phase 3, pointed at the shipping image.

## Risks & open questions

Anchoring validation on a live fork collapses most of what used to be open reasoning into
things we simply *observe against the node*. What remains:

- **The composition spike was the one genuine unknown (Decision 2) — now RESOLVED.** A live fork
  cannot answer a build-graph question. The spike has been run: the `state_translation` framework
  compiles over `v1_1::LedgerDb` and shares the arena with the ledger v8/v9 state types (see
  Decision 2's spike-result note). What remains of Phase 2 is the mechanical table port over a
  storage foundation that is now proven to hold.
- **Byte-identity is measured, not assumed — and now node-validated.** The per-block root check
  means a re-ported table that drifts by even one placeholder value breaks the boundary. The
  frozen golden-root test guards this on every commit, and it has been cross-checked against the
  node's own table over a populated state (Decision 3.2 status note). RC → final drift and the
  "placeholder parameters must match the node's chosen hardfork values" concern reduce to a single
  operation: **regenerate the golden fixture against the node build being shipped.** Both sides
  read the same `ledger_v9::structure::INITIAL_PARAMETERS`, so they agree iff the indexer pins the
  same final ledger tag the node ships.
- **No published `2.1.0` image is a blocker.** The migration node can be built from a local node
  checkout at the 2.1.0 migration runtime (`spec_version 002_001_000`); that checkout also carries
  `metadata/static/midnight_metadata_2.1.0.scale` for the indexer's `v2_1_0` subxt codegen. What
  *is* still required for a live crossing is the indexer-side recognition of `2_001_000` (Phase 1 /
  #1333) — see Status & handoff.
- **State size makes the drain path a real test.** `run(budget)` draining to completion only gets
  meaningfully exercised on a large state; a tiny synthetic dev network would pass even a
  short-drain bug. Use a **mainnet snapshot** for the boundary e2e so the drain loop and root
  timing are stressed realistically.
- **#1333 metadata provenance — confirmed by construction.** #1333's `.node/2.1.0/metadata.scale`
  must come from a node build that includes node#1925's migration. Rather than track this as a
  coordination item, a clean crossing on the overlay confirms it: stale metadata would fail to
  decode the boundary block.
- **`MIDNIGHT_LEDGER_EXPERIMENTAL` — confirmed by construction.** The node's `hardfork_e2e` runs
  with this flag; whether the shipped mainnet migration path is gated behind it is answered by
  whether the overlay's un-flagged crossing succeeds against the shipping image.

## References

- Node migration: [midnight-node#1925](https://github.com/midnightntwrk/midnight-node/pull/1925)
  (merged 2026-07-29 @ `74a91156`) — `ledger/helpers/src/state_translation_v8_to_v9.rs`,
  `ledger/src/host_api/migration_8_to_9.rs`, `pallets/midnight/src/migrations/v2.rs`,
  `pallets/version/src/lib.rs`.
- Master spec: [shieldedtech/product#119](https://github.com/shieldedtech/product/issues/119)
  (linchpin convention, indexer requirement).
- Indexer ticket: [IDX-01 / midnight-indexer#1282](https://github.com/midnightntwrk/midnight-indexer/issues/1282).
- Dust-reset chain: [shielded-security-engineering#547–552](https://github.com/shieldedtech/shielded-security-engineering/issues/547).
- QA test plan: `shielded-qa` → `releases/test plans/2026/ledger-8-to-9-hardfork-test-plan.md`.
- Prior art in this repo: [#1346](https://github.com/midnightntwrk/midnight-indexer/pull/1346)
  (runtime-version dispatch), [#1377](https://github.com/midnightntwrk/midnight-indexer/pull/1377)
  (BABE gating), [#1333](https://github.com/midnightntwrk/midnight-indexer/pull/1333) (Phase 1:
  `V2_1` + `v2_1_0` runtime), [#1364](https://github.com/midnightntwrk/midnight-indexer/pull/1364)
  (fork-overlay e2e, branch `feat/midnight-fork-overlay`), `docs/upgrading-ledger.md`,
  `docs/updating-node-version.md`.
