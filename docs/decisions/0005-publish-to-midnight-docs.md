## Decision: Replace Scala Indexer Docs with Rust Indexer Docs

**Date:** 29 Jan 2025

Discussions were made in Slack channels, [Team](https://shielded.slack.com/archives/C080ARCQ8LS/p1737654283028199) and [topic-docs-dev](https://shielded.slack.com/archives/C0827CHRHE0/p1737639788827869?thread_ts=1737452557.825489&cid=C0827CHRHE0).

### 1. Context
- **Scala Indexer Documentation**: Historically lived under `docs/develop/reference/midnight-api/pubsub-indexer` in the [midnight-docs] repository.
- **Rust Indexer**: Now supersedes the Scala Indexer. We want to publish Rust docs in that same location (but renamed to `midnight-indexer`).

### 2. Summary of Change
We have an **open PR** in [midnight-docs #283](https://github.com/input-output-hk/midnight-docs/pull/283) that:

1. Updates `.github/workflows/apis.yml` to reference `midnight-indexer` instead of `pubsub-indexer`.
2. Prepares the docs site to pull from this repository (rather than the old Scala one).

Once the Rust Indexer is testnet-ready, we’ll **merge** that PR, so the doc workflow can fetch these Rust docs and make them live on <https://docs.midnight.network>.

### 3. Process & Steps

1. **Merge the `pubsub-indexer` → `midnight-indexer` PR in midnight-docs**
    - This effectively instructs the “Copy API docs” workflow to pull from **`midnight-indexer`** instead of **`midnight-pubsub-indexer`**.

2. **Run the “Copy API docs” Workflow** (manually in midnight-docs)
    - You open <https://github.com/input-output-hk/midnight-docs/actions/workflows/apis.yml>, select the correct branch for `component docs to update: midnight-indexer`, and run.
    - It creates a new PR in midnight-docs, auto-fetching `API-documentation.md` and `schema-v1.graphql` from **this** repository’s `docs/api/v1/` folder.

3. **Review & Merge the Generated PR**
    - This automated future PR in midnight-docs updates the live docs site, replacing the old Scala Indexer references with Rust Indexer docs.

4. **Remove Scala References**
    - If any leftover files remain in midnight-docs (like older Scala `insomnia_api.json` or `application.conf` references), we’ll open a small housekeeping PR to delete them once we confirm Rust docs are live and stable.

### 4. Rationale
- **Retiring Scala Indexer**: The Scala-based indexer is no longer actively developed.
- **Minimize Confusion**: We want one canonical set of docs, i.e. Rust Indexer docs, matching the new production path.
- **Maintain a Clear Pipeline**: After doc changes are merged in this repository, the manual “Copy API docs” workflow in midnight-docs ensures we only publish stable docs to the live site.

### 5. Impact
- **Midnight-docs**: Gains an updated “midnight-indexer” page.
- **Users**: Will see only the Rust-based Indexer docs for new integrations, avoiding confusion with old “pubsub-indexer” naming.

### 6. Status & Next Steps
- The team will merge [midnight-docs #283](https://github.com/input-output-hk/midnight-docs/pull/283) once Rust Indexer is testnet-ready.
- After merging, a new “Copy API docs” run will publish these docs to the site.
- Scala references can be dropped in a minor clean-up step afterward.
