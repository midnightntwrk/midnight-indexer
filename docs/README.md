# Indexer Docs

Maintainer and contributor guides. For local setup see the top-level
[`README.md`](../README.md) and [`CLAUDE.md`](../CLAUDE.md).

## Architecture & testing

- [Architecture](./architecture.md) - the data flow, the pub-sub layer's role, and run modes.
- [Testing & node consistency](./testing.md) - the runtime root-match guard and the
  test layers.

## Releasing & version upgrades

- [Creating a release](./releasing.md) - versioning, changelog, tagging, image
  publish.
- [Upgrading the node version](./updating-node-version.md) - `NODE_VERSIONS`,
  metadata, per-version runtime modules.
- [Upgrading the ledger](./upgrading-ledger.md) - the `v8`/`v9` coexistence and
  the `[patch.crates-io]` git-tag pins.

## Other

- [Indexer API guide (v4)](./api/v4/api-documentation.md) - the indexer's
  GraphQL queries, mutations, and subscriptions.
- [actionlint guide](./actionlint-guide.md)
