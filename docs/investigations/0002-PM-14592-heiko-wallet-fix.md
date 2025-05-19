# PM-14592: Explanation of the Rust Indexer Wallet Subscription Fix

**Ticket**: [PM-14592](https://shielded.atlassian.net/browse/PM-14592)  
**Merged PR**: [#407](https://github.com/midnightntwrk/midnight-indexer/pull/407)  
**Author**: Heiko Seeberger  
**Date**: 6 march 2025 (the PR merged on 19 Feb)

---

## Background

Prior to this fix, empty wallets (i.e. wallets without any relevant transactions) would never transition from "syncing" to "synced" when using the Rust indexer. In contrast, the older Scala indexer regularly sent redundant `MerkleTreeCollapsedUpdate` events—even for wallets with no relevant transactions—which accidentally triggered the wallet to show a "synced" state.

## Problem

- **Wallet Expects Merkle Updates**: The Scala indexer’s "spammy" updates made Lace reliant on receiving `MerkleTreeCollapsedUpdate` even when the user had no transaction history.
- **Rust Indexer Efficiency**: The Rust indexer only sent `MerkleTreeCollapsedUpdate` when an actual relevant transaction existed, relying on `ProgressUpdate` messages to handle the empty-wallet scenario. Lace wasn’t recognising progress updates as sufficient, and so remained stuck in "syncing".

## What Changed in PR #407

1. **Progress Updates**
    - The Rust indexer now sends accurate `ProgressUpdate` messages with the correct `synced` and `total` indexes.
    - This should let the wallet confirm it’s at the chain tip even if there are no relevant transactions.

2. **No Unnecessary Merkle-Tree Updates**
    - The Rust indexer does not spam collapsed Merkle-tree updates for empty wallets. This is intentional to avoid large, pointless data loads.

3. **Documentation and Wallet Adjustments**
    - The wallet code needs to handle the case where no transactions exist but `ProgressUpdate` indicates the user is fully synced. This fix is now in place, but older versions of Lace will remain stuck unless they’re updated.

## Credits

- The work and investigation for this fix were driven by **Heiko Seeberger**. This documentation is simply capturing his explanation and solution.

---

**If you have any questions, please refer to:**
- The original Jira ticket: [PM-14592](https://shielded.atlassian.net/browse/PM-14592)
- The PR (#407) in the [midnight-indexer](https://github.com/midnightntwrk/midnight-indexer/pull/407) repository.
