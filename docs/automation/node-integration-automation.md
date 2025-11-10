# Node Integration Automation

**Jira**: [TPM-774](https://shielded.atlassian.net/browse/TPM-774)

## Overview

Automatically creates integration PR when midnight-node releases. Triggered by `repository_dispatch` from node repo or manual workflow.

**Workflow**: [`.github/workflows/auto-integrate-node-release.yaml`](../../.github/workflows/auto-integrate-node-release.yaml)

**Automated**: NODE_VERSION update, metadata.scale download, cargo check/test, PR creation (draft if errors)

**Manual**: Breaking changes fixes, event handlers, CHANGELOG.md

## Usage

When PR created, review logs. If draft: update event handlers (`chain-indexer/src/infra/subxt_node/runtimes.rs`), domain types (`indexer-common/src/domain.rs`), fix tests, update CHANGELOG.

**Manual trigger**:
```bash
gh workflow run auto-integrate-node-release.yaml \
  --repo midnightntwrk/midnight-indexer \
  -f node_version=0.18.0
```

## Troubleshooting

- **No PR?** Check [workflow runs](https://github.com/midnightntwrk/midnight-indexer/actions/workflows/auto-integrate-node-release.yaml)
- **Branch exists?** Delete `feat/integrate-node-<version>` to retry

Contact: @cosmir17, @hseeberger
