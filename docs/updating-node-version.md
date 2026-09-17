# Upgrading the Node Version

How to add or move to a Midnight Node version the indexer talks to.

## Model: many versions at once

`NODE_VERSIONS` (one version per line, oldest first, **append-only**) lists 
every node version this build supports at once; the last line is the default 
for local `just` recipes. The indexer picks a runtime per block from the chain's 
protocol version (`indexer-common/src/domain/protocol_version.rs`).

Per listed version `X`:

- `.node/X/metadata.scale` - subxt metadata, consumed at build time.
- `.node/X/chain/...` - paritydb snapshot for tests.
- `chain-indexer/src/infra/subxt_node/runtimes/vX_Y_Z.rs` - runtime-specific
  decode logic.

`build.rs` reads `NODE_VERSIONS` and emits a `#[subxt::subxt(...)]` module per
entry from its `metadata.scale`; a missing file fails the build.

The module is named for the runtime: `build.rs` reads `spec_version` out of the 
metadata from the node and emits `runtime_<major>_<minor>_<patch>`.
Node releases sharing a runtime therefore share one module. `node-1.0.2` ships
`runtime-1.0.0`, so the listed version `1.0.2` is decoded by `v1_0_0.rs`.

## Adding / bumping a version

### 1. Record the version

A new supported node version is appended to `NODE_VERSIONS`. Moving an existing supported
version forward (`2.0.0-rc.1` -> `rc.3`, `1.0.0` -> `1.0.2`) rewrites its line.

### 2. Generate data + metadata

```bash
just update-node                 # the latest NODE_VERSIONS line
just update-node 1.0.2           # a named version, e.g. a middle line
just update-node 1.0.2 1.0.0     # ... whose toolkit version differs
```

Produces `.node/X/chain/` (snapshot) and `.node/X/metadata.scale`. Needs `subxt`
at the version pinned in `Cargo.toml`: `cargo install subxt-cli --version
<pinned>`.

A node release names the toolkit release it ships with, and the two carry
independent version numbers: e.g. `node-1.0.2` ships `toolkit-1.0.0`. 
The second argument covers that; it defaults to the node version.

### 3. Wire the runtime

Build after regenerating. The outcome tells you whether the runtime moved:

- **`runtime_X_Y_Z` not found** - the module a decode module referenced is gone,
  because the runtime behind an existing line moved. Rename `runtimes/vX_Y_Z.rs`
  and its references to the new runtime, or add a module if both runtimes stay
  listed.
- **Compiles clean** - no listed line changed the runtime it had. A
  `metadata.scale` identical to the one it replaces confirms it.

A line added for a runtime the tree does not decode compiles clean too: the
generated module is simply unreferenced. Add `runtimes/vX_Y_Z.rs` (copy the
nearest existing one) and register it plus its match arms in `runtimes.rs`.

A protocol version outside every range in `protocol_version.rs` also needs a
`NodeVersion` variant, a `ProtocolVersion` range, and the mappings between them.
Nothing detects either omission.

### 4. Regenerate tx fixtures (if the wire format moved)

```bash
just generate-txs   # rewrites indexer-common/tests/*.raw from a running node
```

### 5. Drop superseded data (optional)

Delete `.node/<old-version>/` once it is off `NODE_VERSIONS`.

### 6. Verify

```bash
just all-all
just run-node                          # latest NODE_VERSIONS line
cargo test -p indexer-tests native_e2e
```

If a test hardcodes the old version (a version string, block hash, or count),
find it by searching rather than trusting a fixed location - such tests have
moved before: `rg '0\.22\.0|<old-version>'`.

### PR checklist

- [ ] `NODE_VERSIONS` updated
- [ ] `.node/<version>/{metadata.scale,chain/}` present
- [ ] `just all-all` green
- [ ] no references to a removed version (`rg <old-version>`)

A **runtime the tree does not already decode** also needs:

- [ ] `runtimes/vX_Y_Z.rs` plus dispatch arms in `runtimes.rs`

A **protocol version outside every `protocol_version.rs` range** also needs:

- [ ] `protocol_version.rs`: new `NodeVersion` variant, `ProtocolVersion` range,
      and `→ NodeVersion` / `→ LedgerVersion` mappings

## Breaking changes

A node bump can move the runtime API:

| Symptom | Cause | Fix lives in |
| ----------------------------------- | --------------------- | ------------------------------------------------------- |
| `E0560` struct has no field         | field removed/renamed | `indexer-common/src/domain/`, GraphQL schema if exposed |
| missing/extra fields on destructure | event struct changed  | `chain-indexer/.../runtimes/vX.rs`                      |
| hex/decode runtime error            | tx encoding changed   | `chain-indexer/src/infra/subxt_node/`                   |

Exposed field changed → regen the GraphQL schema
(`just generate-indexer-api-schema`). *Stored* field changed → add a migration
under `indexer-common/migrations/`. A protocol bump usually rides with a ledger
bump - see [Upgrading the ledger](./upgrading-ledger.md).

## Common mistakes

- **Metadata without code** - a new protocol version needs its runtime module
  and dispatch arms, not just `metadata.scale`.
- **Stale inline test data** - green locally if you skip tests, red in CI.
- **Partial search** - `rg <old-version>` catches every occurrence; eyeballing
  misses some.
- **No live node** - `just all-all` alone won't surface wire-format mismatches.

## CI considerations

CI fails if a listed version's `metadata.scale` is missing, versions disagree
across `NODE_VERSIONS` / code / `.node/`, or a test points at a deleted `.node/`
directory.

## Rollback

Revert the PR; the new `.node/<version>/` data can stay (harmless). Confirm
`NODE_VERSIONS` and code point back at the known-good version.

## See also

- [Upgrading the ledger](./upgrading-ledger.md)
- [Creating a release](./releasing.md)
