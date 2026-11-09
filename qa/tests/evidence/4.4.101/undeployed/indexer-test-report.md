# Indexer Test Execution Report — undeployed

## Run Details

| Field | Value |
| --- | --- |
| Component Under Test | Indexer |
| Indexer Version | 4.4.101: image tag `4.4.101-5b60b43a` (arm64), equivalent to `4.4.101` (both built from commit `5b60b43a65a898933f25b9bbd60bcd5d855c8560`, tag `v4.4.101`) |
| Indexer images | `ghcr.io/midnight-ntwrk/{chain-indexer,wallet-indexer,spo-indexer,indexer-api}:4.4.101-5b60b43a` (`INDEXER_TAG`); |
| Node Version | 2.1.101: `ghcr.io/midnight-ntwrk/midnight-node:2.1.101` (`NODE_TAG`) |
| Node toolkit | `ghcr.io/midnight-ntwrk/midnight-node-toolkit:2.1.101` |
| Proof server | `midnightntwrk/proof-server:9.0.0-rc.10` |
| Test Suites | smoke, integration, end-to-end |
| Transaction backends | node toolkit (smoke, integration, end-to-end) and moth (end-to-end) |
| Environment | undeployed (`TARGET_ENV=undeployed`). Local Docker stack from the repository's `docker-compose.yaml`, provisioned by the suite, indexer API on port 8088 |
| Test commit | `24b350f06776261a3dd54b7188a09891de5b2029` on `release/4.4.101` |
| Date | Thursday 5 November 2026 |
| Tested by | Oleksandr Romanov (oleksandr.romanov@shielded.io) |

Shared tooling versions (moth-wallet, wallet SDK, Midnight.js, ledger client, test runner) are listed in the [summary report](../README.md#component-versions).

## Summary

> Legend: ✓ passed · ✗ failed · ↓ skipped · ☐ todo.

| Suite | Backend | Test Files | Passed | Failed | Skipped | Todo | Total | Duration | Run at (UTC) | Evidence |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- | --- |
| Smoke | toolkit | 6 | 29 | 0 | 1 | 0 | 30 | 23 s | 16:53:17 – 16:53:40 | [`smoke/smoke.json`](smoke/smoke.json) |
| Integration | toolkit | 30 | 175 | 0 | 120 | 20 | 315 | 2 min 40 s | 16:53:41 – 16:56:20 | [`integration/integration.json`](integration/integration.json) |
| End-to-end | toolkit | 13 | 61 | 0 | 15 | 2 | 78 | 15 min 15 s | 16:56:21 – 17:11:34 | [`e2e/e2e-toolkit.json`](e2e/e2e-toolkit.json) |
| End-to-end | moth | 13 | 49 | 0 | 27 | 2 | 78 | 13 min 23 s | 17:11:36 – 17:24:56 | [`moth/e2e.json`](moth/e2e.json) |
| **Total** | | **62** | **314** | **0** | **163** | **24** | **501** | | | |

Result: **PASS.** One moth end-to-end test passed only on its automatic retry (see [Known issues](#known-issues)).

## Known issues

| Test | Run | Result | What happens |
| --- | --- | --- | --- |
| unshielded NIGHT transactions › a confirmed unshielded transfer streamed to address subscriptions › should emit UnshieldedTransaction only for the target wallet (A > B1) (`tests/e2e/night-transactions.test.ts`) | end-to-end, moth | PASS on retry | The first attempt failed and the automatic retry (`retry: 1`) passed, taking 44 s. The vitest JSON keeps the first attempt's error: `AssertionError: expected 3n to be -49999999999995n` at `night-transactions.test.ts:127`, inside `validateCrossWalletTransaction`. The test passed on its first attempt in the toolkit run. See [Failures and known issues](../README.md#failures-and-known-issues). |

## Cases

Each case is one test file. "Expected" is what every test in the file asserts. Many cases skip themselves on a fresh local chain because they need chain data that only qanet has: DUST generations, bridge data, contract fixtures and a custom unshielded token.

### Smoke

| Case | Covers | Expected | Result |
| --- | --- | --- | --- |
| `smoke/service-healthchecks.test.ts` | `/ready`, and unknown paths under `/api/v3` and `/api/v4` | `/ready` returns 200; unknown paths do not redirect to a doubled version prefix, and following redirects terminates | PASS 5/5 |
| `smoke/graphql-healthchecks.test.ts` | GraphQL introspection over HTTP and WebSocket, query depth limit | schema returned; queries deeper than 15 rejected | PASS 4/4 |
| `smoke/graphql-schema-deprecations.test.ts` | Nine deprecated fields (`merkleTreeRoot`, `startIndex`, `endIndex`, `fees`, `estimatedFees` and the progress end indexes) | each field still present and marked deprecated | PASS 9/9 |
| `smoke/progress-protocol-version-schema.test.ts` | `protocolVersion` on the unshielded, shielded and DUST progress types | field present with the expected type | PASS 3/3 |
| `smoke/bridge-schema.test.ts` | Bridge types, queries and subscriptions in the schema | five `BridgeEvent` variants, the claim and balance types and the bridge fields exist | PASS 8/8 |
| `smoke/graphql-schema-change.test.ts` | Full schema diff against a reference | no unexpected change | SKIPPED 1 |

### Integration

All integration paths are under `qa/tests/tests/integration/basic/`. The run used the toolkit backend.

| Case | Covers | Expected | Result |
| --- | --- | --- | --- |
| `queries/block-queries.test.ts` | Block by hash, by height, both, latest and genesis; tree end indexes; per-block DUST tree roots | correct block returned with a valid shape; invalid input rejected | PASS 22, SKIPPED 1 |
| `queries/transaction-queries.test.ts` | Transactions by hash, by identifier and both; genesis transactions; schema validation | correct transactions with a valid shape | PASS 18/18 |
| `queries/contract-queries.test.ts` | Contract by address and offset; `contractEvents` on actions; state anchor block; contracts across the hard fork | correct action and state; state anchored at the right block | PASS 29, SKIPPED 8 |
| `queries/contract-type-queries.test.ts` | Contract types: maintenance authority, actions sub-query, point-in-time state | typed fields resolve correctly | SKIPPED 10, TODO 2 |
| `queries/contract-event-queries.test.ts` | Contract-event schema; empty address; invalid filters; contracts that emit events | events returned and filtered correctly; bad filters rejected | PASS 13, SKIPPED 6 |
| `queries/contract-zswap-state-queries.test.ts` | `block.contractZswapState` and the composed execution-inputs query | zswap state for the contract at the block | SKIPPED 8 |
| `queries/zswap-collapsed-update-queries.test.ts` | Collapsed zswap Merkle updates over valid, equal and invalid index ranges; idempotency | valid update for good ranges; error for bad ranges | PASS 8/8 |
| `queries/dust-commitment-update-queries.test.ts` | Collapsed DUST commitment Merkle updates (same matrix) | as above | PASS 8/8 |
| `queries/dust-generation-update-queries.test.ts` | Collapsed DUST generation Merkle updates (same matrix) | as above | SKIPPED 8 |
| `queries/dust-queries.test.ts` | `dustGenerationStatus` for valid, multiple, malformed, empty, duplicate, payment and wrong-network addresses | status for valid addresses; errors for invalid ones | SKIPPED 15 |
| `queries/dust-generations-queries.test.ts` | `dustGenerations` with valid, multiple, unregistered and invalid addresses; multi-UTXO stake key (#926); compatibility with `dustGenerationStatus` | generations returned, consistent with the status query | SKIPPED 9 |
| `queries/spo-epoch-series-queries.test.ts` | SPO epoch series: span within and above the maximum | data within the limit; error above it | PASS 10/10 |
| `queries/graphql-http-compression.test.ts` | HTTP response compression: gzip, br, zstd and identity | response encoded as requested | PASS 4/4 |
| `queries/bridge-queries.test.ts` | `bridgeEvents`, `bridgeBalance`, `bridgeDeposits` | events, balances and deposits for bridge addresses | SKIPPED 13, TODO 2 |
| `queries/bridge-pool-queries.test.ts` | `bridgePoolSummary`, reserve and treasury inflows | pool totals and inflows | SKIPPED 9, TODO 9 |
| `subscriptions/block-subscriptions.test.ts` | Block stream: no offset, by hash, by height, both; transaction fees | ordered blocks from the requested offset | PASS 9, SKIPPED 1 |
| `subscriptions/contract-subscriptions.test.ts` | Contract-action stream: no offset, from genesis, from a block hash, anchor block; across the hard fork | ordered actions from the offset | PASS 4, SKIPPED 1 |
| `subscriptions/contract-event-subscriptions.test.ts` | Contract-event stream: empty address, invalid filter, contract that emits events | events streamed and filtered; bad filters rejected | PASS 3, SKIPPED 2 |
| `subscriptions/dust-generations-subscriptions.test.ts` | `dustGenerations` stream at the tip and pinned to a block; dtime updates relative to the cutoff height; errors; `transactionHash`; progress protocol version | snapshot at the pinned block; dtime updates delivered before generation items | PASS 1, SKIPPED 12 |
| `subscriptions/dust-ledger-subscriptions.test.ts` | DUST ledger events: default replay, explicit offset, errors | events from the offset | PASS 5/5 |
| `subscriptions/dust-nullifier-subscriptions.test.ts` | DUST nullifier stream over a block range; errors; `transactionHash` (#1114) and transaction reference (#1115) | nullifiers with their transaction | PASS 3, SKIPPED 2 |
| `subscriptions/shielded-nullifier-subscriptions.test.ts` | Shielded nullifier stream; errors; #1114, #1115 | as above | PASS 4, SKIPPED 2 |
| `subscriptions/zswap-events-subscriptions.test.ts` | Zswap ledger events: default replay, offset, errors | events from the offset | PASS 5/5 |
| `subscriptions/shielded-transaction-subscriptions.test.ts` | Wallet sessions: connect by viewing key, disconnect, wallet updates | session lifecycle; relevant transactions and progress | PASS 11/11 |
| `subscriptions/wallet-connect-options.test.ts` | Connect with `startIndex` | stream starts at the requested index | SKIPPED 3 |
| `subscriptions/unshielded-transaction-subscriptions.test.ts` | Unshielded stream by address, and by address plus transaction id | transactions for the address from the offset | PASS 12/12 |
| `subscriptions/subscription-quotas.test.ts` | Concurrent subscriptions per connection; creation rate per session | subscriptions over the cap rejected and a slot freed on close; creations over the rate rejected, then allowed after refill, while a live subscription keeps streaming | PASS 5/5 |
| `subscriptions/websocket-keepalive.test.ts` | Idle WebSocket with and without the client keepalive | idle socket past the idle limit closed without the keepalive, kept open with it | PASS 1, SKIPPED 1 |
| `subscriptions/bridge-subscriptions.test.ts` | `bridgeEvents` and `bridgeBalance` streams | bridge events and balance updates | SKIPPED 6, TODO 3 |
| `subscriptions/bridge-pool-subscriptions.test.ts` | `bridgePoolUpdates` stream | a pool update per bridge event | SKIPPED 3, TODO 4 |

### End-to-end

Each test submits real transactions and checks what the indexer reports. Paths are under `qa/tests/tests/`.

| Case | Covers | Expected | toolkit | moth |
| --- | --- | --- | --- | --- |
| `e2e/contract-actions-sequential.test.ts` | Deploy a counter contract, call its `store` circuit, update it | each action reported through transaction, block and contract-action queries, and through subscriptions | PASS 10/10 | PASS 10/10 |
| `e2e/contract-shielded-balance.test.ts` | Shielded balance held by a contract | the contract's shielded balance reported correctly | PASS 5/5 | PASS 5/5 |
| `e2e/segment-probe-contract-actions.test.ts` | Deploy a segment-split contract; call `burnWithGuaranteed` | deploy and call reported through transaction, block and contract-action queries | SKIPPED 6 | PASS 6/6 |
| `e2e/night-transactions.test.ts` | NIGHT transfer between two addresses; genesis NIGHT; DUST activity of the transfer; address subscriptions; address with no history | transfer reported with correct inputs, outputs and value; only the target wallet notified | PASS 13/13 | PASS 13/13 (1 on retry) |
| `e2e/custom-unshielded-token-transactions.test.ts` | Unshielded custom-token transfer | transfer reported with correct token type and value | SKIPPED 8 | SKIPPED 8 |
| `e2e/shielded-transactions.test.ts` | Shielded transfer between two wallets; wallet-session stream by viewing key | transfer visible to both wallets | PASS 8/8 | PASS 8/8 |
| `e2e/toolkit/dust-balance.test.ts` | DUST balance of the funding wallet | balance returned in the expected schema | PASS 1/1 | PASS 1/1 |
| `e2e/toolkit/show-wallet-queries.test.ts` | Private and public wallet state | state returned in the expected schema | PASS 2/2 | PASS 2/2 |
| `e2e/contract-actions-partial-success.test.ts` | Calls that partially succeed, with and without a guaranteed transcript | reported when the guaranteed transcript applied; not reported when neither phase applied | PASS 6/6 | SKIPPED 6 |
| `e2e/contract-deploy-failed-segment.test.ts` | Deploy in a rolled-back fallible segment | no deploy reported for the rolled-back address; the indexer keeps advancing | PASS 6, TODO 1 | SKIPPED 6, TODO 1 |
| `e2e/subscription-polling.test.ts` | Subscription polling, backoff and heartbeat for idle addresses and wallets | progress emitted at once, backs off while idle, resets on a new transaction | PASS 4, TODO 1 | PASS 4, TODO 1 |
| `e2e/toolkit/key-material.test.ts` | Address and viewing-key prefixes through the toolkit | network-specific prefixes | PASS 6/6 | SKIPPED 6 |
| `e2e/unshielded-balance-e2e.test.ts` | Unshielded balance after deploy, mint and send (#1253) | contract remainder reported | SKIPPED 1 | SKIPPED 1 |

## Smoke results (toolkit)

### Bridge schema

File name: `tests/smoke/bridge-schema.test.ts`

#### bridge GraphQL schema

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | types › should expose BridgeEvent interface with flat blockHeight | 5 ms |
| ✓ | types › should expose the five concrete BridgeEvent variants | 16 ms |
| ✓ | types › should expose BridgeSubminimalFlushTransfer without a cardanoTxHash | 2 ms |
| ✓ | types › should expose BridgeClaimTransaction implementing Transaction | 7 ms |
| ✓ | types › should expose BridgeBalance with deposited, claimed, balance | 4 ms |
| ✓ | types › should expose BridgeEventVariant enum with five values | 3 ms |
| ✓ | queries › should expose the bridge query fields on Query | 2 ms |
| ✓ | subscriptions › should expose the bridge subscription fields on Subscription | 2 ms |

### Graphql healthchecks

File name: `tests/smoke/graphql-healthchecks.test.ts`

#### graphql health checks

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | an introspection message sent to the websocket channel › should return the supported graphql schema | 528 ms |
| ✓ | an introspection message sent to the websocket channel › should return an error, given the depth of the query is > 15 | 521 ms |
| ✓ | an introspection request sent to the http channel › should return the supported graphql schema | 28 ms |
| ✓ | an introspection request sent to the http channel › should return an error, given the depth of the query is > 15 | 3 ms |

### Graphql schema change

File name: `tests/smoke/graphql-schema-change.test.ts`

#### GraphQL Schema Stability Check

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | should not change the schema unexpectedly | — |

### Graphql schema deprecations

File name: `tests/smoke/graphql-schema-deprecations.test.ts`

#### graphql schema deprecations

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | RegularTransaction.merkleTreeRoot › should be marked deprecated with reason "Use zswapMerkleTreeRoot instead" | 29 ms |
| ✓ | RegularTransaction.startIndex › should be marked deprecated with reason "Use zswapStartIndex instead" | 4 ms |
| ✓ | RegularTransaction.endIndex › should be marked deprecated with reason "Use zswapEndIndex instead" | 3 ms |
| ✓ | RelevantTransaction.collapsedMerkleTree › should be marked deprecated with reason "Use zswapCollapsedUpdate instead" | 2 ms |
| ✓ | ShieldedTransactionsProgress.highestEndIndex › should be marked deprecated with reason "Use highestZswapEndIndex instead" | 3 ms |
| ✓ | ShieldedTransactionsProgress.highestCheckedEndIndex › should be marked deprecated with reason "Use highestCheckedZswapEndIndex instead" | 2 ms |
| ✓ | ShieldedTransactionsProgress.highestRelevantEndIndex › should be marked deprecated with reason "Use highestRelevantZswapEndIndex instead" | 3 ms |
| ✓ | RegularTransaction.fees › should be marked deprecated with reason "Use fee instead" | 4 ms |
| ✓ | TransactionFees.estimatedFees › should be marked deprecated with reason "Use paidFees instead" | 4 ms |

### Progress protocol version schema

File name: `tests/smoke/progress-protocol-version-schema.test.ts`

#### progress protocol version schema

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | UnshieldedTransactionsProgress.protocolVersion › should be served as a non-null Int | 5 ms |
| ✓ | ShieldedTransactionsProgress.protocolVersion › should be served as a non-null Int | 4 ms |
| ✓ | DustGenerationsProgress.protocolVersion › should be served as a non-null Int | 2 ms |

### Service healthchecks

File name: `tests/smoke/service-healthchecks.test.ts`

#### service health checks

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a request to the /ready endpoint › should return a 200 status code OK | 26 ms |
| ✓ | a request to an unrecognised path /api/v3/__regression_unknown_path › should not 308 to a version-double-prefixed Location | 5 ms |
| ✓ | a request to an unrecognised path /api/v3/__regression_unknown_path › should terminate when redirects are followed | 2 ms |
| ✓ | a request to an unrecognised path /api/v4/__regression_unknown_path › should not 308 to a version-double-prefixed Location | 1 ms |
| ✓ | a request to an unrecognised path /api/v4/__regression_unknown_path › should terminate when redirects are followed | 1 ms |
## Integration results (toolkit)

### Block queries

File name: `tests/integration/basic/queries/block-queries.test.ts`

#### block queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a block query without parameters › should return the latest known block | 44 ms |
| ✓ | a block query without parameters › should respond with a block according to the requested schema | 11 ms |
| ✓ | a block query by hash › should return the block with that hash, given that block exists | 25 ms |
| ✓ | a block query by hash › should return blocks according to the requested schema | 17 ms |
| ✓ | a block query by hash › should return a null block, given a block with that hash doesn't exist | 5 ms |
| ✓ | a block query by hash › should return an error, when the hash is invalid (malformed) | 13 ms |
| ✓ | a block query by height › should return the block with that height, given a valid height | 12 ms |
| ✓ | a block query by height › should return a blocks according to the requested schema | 13 ms |
| ✓ | a block query by height › should return the genesis block, given height=0 is requested | 31 ms |
| ✓ | a block query by height › should return an empty body answer, given that block height requested is the maximum available height | 3 ms |
| ✓ | a block query by height › should return an error, given an invalid height | 6 ms |
| ✓ | a block query by height and hash › should return an error, as only one parameter at a time can be used | 10 ms |
| ✓ | block-level tree end indexes › should return non-negative integers for all three tree end index fields | 6 ms |
| ✓ | block-level tree end indexes › should return non-decreasing tree end indexes from parent to child block | 11 ms |
| ↓ | per-block dust merkle tree roots › should return different dust generation roots for blocks with different tree sizes | — |
| ✓ | per-block dust merkle tree roots › should return identical dust roots for repeated queries of the same block | 18 ms |
| ✓ | per-block dust merkle tree roots › should return equal dust generation roots for parent and child with equal tree sizes | 13 ms |

#### genesis block

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a block query to the genesis block › should contain transactions with pre-fund wallet utxos | 29 ms |
| ✓ | a block query to the genesis block › should contain utxos related to exactly 4 pre-fund wallets | 27 ms |
| ✓ | a block query to the genesis block › should contain utxos with exactly 1 token type | 40 ms |
| ✓ | a block query to the genesis block › should contain utxos sorted by outputIndex in ascending order | 40 ms |
| ✓ | a block query to the genesis block › should contain valid index ranges on regular transactions | 28 ms |
| ✓ | a block query to the genesis block › should return block-level end indexes >= max RegularTransaction end indexes | 35 ms |

### Bridge pool queries

File name: `tests/integration/basic/queries/bridge-pool-queries.test.ts`

#### bridge pool queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | bridgePoolSummary › should return zero pool totals when no reserve or treasury events are indexed | — |
| ↓ | bridgePoolSummary › should expose lastEventBlockHeight as a positive integer where bridge events exist | — |
| ↓ | bridgePoolSummary › should return an empty snapshot when atBlock predates all bridge events | — |
| ☐ | bridgePoolSummary › should set reserveTotal to the cumulative sum of ReserveTransfer amounts | — |
| ☐ | bridgePoolSummary › should aggregate treasury inflows separately by INVALID and UNAPPROVED reason | — |
| ☐ | bridgePoolSummary › should accumulate more pool inflow at a later atBlock than an earlier one | — |
| ↓ | bridgePoolSummary › should count SubminimalFlushTransfer.count in subminimumTxCount | — |
| ↓ | bridgePoolSummary › should aggregate UnapprovedTransfer amounts under UNAPPROVED treasury reason | — |
| ↓ | bridgeReserveInflows › should return an empty list when no ReserveTransfer events are indexed | — |
| ☐ | bridgeReserveInflows › should return ReserveTransfer events within the specified block range | — |
| ☐ | bridgeReserveInflows › should return events with correct BridgeReserveTransfer field shape | — |
| ☐ | bridgeReserveInflows › should paginate ReserveTransfer events with offset and limit | — |
| ↓ | bridgeTreasuryInflows › should return an empty list when no treasury-redirected events are indexed | — |
| ☐ | bridgeTreasuryInflows › should return all treasury event types when no reason filter is given | — |
| ☐ | bridgeTreasuryInflows › should return only BridgeInvalidTransfer when reason=INVALID | — |
| ☐ | bridgeTreasuryInflows › should respect blockHeightFrom and blockHeightTo filters for treasury inflows | — |
| ↓ | bridgeTreasuryInflows › should return only BridgeUnapprovedTransfer when reason=UNAPPROVED | — |
| ↓ | bridgeTreasuryInflows › should return only BridgeSubminimalFlushTransfer when reason=SUBMINIMAL_FLUSH | — |

### Bridge queries

File name: `tests/integration/basic/queries/bridge-queries.test.ts`

#### bridge queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | bridgeEvents › should return an empty list for a recipient address with no bridge activity | — |
| ↓ | bridgeEvents › should return events with the correct shape for BridgeUserTransfer | — |
| ↓ | bridgeEvents › should return only events for the specified recipient address | — |
| ↓ | bridgeEvents › should return events of the known variant types present on chain | — |
| ↓ | bridgeEvents › should paginate results with offset and limit | — |
| ↓ | bridgeEvents › should return only UserTransfer events when filtered by variant USER_TRANSFER | — |
| ↓ | bridgeEvents › should reject a malformed recipient with a GraphQL error | — |
| ☐ | claims via unshieldedTransactions › should surface a BridgeClaimTransaction (recipient, amount) via unshieldedTransactions for a claim recipient | — |
| ↓ | bridgeBalance › should return zero balance for an address with no bridge activity | — |
| ↓ | bridgeBalance › should return zero balance once an address has fully claimed | — |
| ↓ | bridgeBalance › should set deposited to the sum of UserTransfer amounts for the address | — |
| ☐ | bridgeBalance › should reflect a non-zero remaining-claimable balance after a partial claim | — |
| ↓ | bridgeDeposits › should return an empty list for an address with no deposits | — |
| ↓ | bridgeDeposits › should return only UserTransfer events by default (excludes UnapprovedTransfer) | — |
| ↓ | bridgeDeposits › should include UnapprovedTransfer events when includeUnapproved=true | — |

### Contract event queries

File name: `tests/integration/basic/queries/contract-event-queries.test.ts`

#### contract event queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | the contract event GraphQL schema › should expose ContractEvent as an interface with the common fields | 5 ms |
| ✓ | the contract event GraphQL schema › should expose all eleven concrete contract event types | 3 ms |
| ✓ | the contract event GraphQL schema › should expose the ContractEventType enum with eleven variants | 2 ms |
| ✓ | the contract event GraphQL schema › should expose the ContractEventFilter input fields | 4 ms |
| ✓ | the contract event GraphQL schema › should expose the FieldPrefixFilter input fields | 2 ms |
| ✓ | a contract events query for an address with no emitted events › should return an empty list | 5 ms |
| ✓ | a contract events query for an address with no emitted events › should return an empty list when filtered by event types | 4 ms |
| ✓ | a contract events query for an address with no emitted events › should return an empty list when filtered by a block range | 4 ms |
| ✓ | a contract events query for an address with no emitted events › should return an empty list when filtered by valid field prefixes | 12 ms |
| ✓ | a contract events query with an invalid filter › should return an error when the contract address is empty | 2 ms |
| ✓ | a contract events query with an invalid filter › should return an error for malformed contract addresses | 25 ms |
| ✓ | a contract events query with an invalid filter › should return an error for an unknown field prefix field name | 3 ms |
| ✓ | a contract events query with an invalid filter › should return an error when a field prefix is not hex | 1 ms |
| ↓ | a contract events query for a contract with emitted events › should return events conforming to the contract event schema | — |
| ↓ | a contract events query for a contract with emitted events › should return only events of the requested type when filtered | — |
| ↓ | a contract events query for a contract with emitted events › should filter events by a prefix of an indexed field value | — |
| ↓ | a contract events query for a contract with emitted events › should combine multiple field prefixes with AND semantics | — |
| ↓ | a contract events query for a contract with emitted events › should return only events carrying the field for an empty prefix | — |
| ↓ | a contract events query for a contract with emitted events › should reach every event through each of its indexed fields | — |

### Contract queries

File name: `tests/integration/basic/queries/contract-queries.test.ts`

#### contract queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a contract query by address › should return success as they are valid contract addresses | 52 ms |
| ✓ | a contract query by address › should return the most recent action for a contract with multiple actions | 19 ms |
| ✓ | a contract query by address › should return null when contract with that address does not exist | 3 ms |
| ✓ | a contract query by address › should return an error for malformed addresses | 30 ms |
| ↓ | a contract query by address › should return non-empty unshieldedBalances for contracts known to hold tokens | — |
| ✓ | a contract query by address and offset › should return the correct action using exact block hash where it was included | 7 ms |
| ✓ | a contract query by address and offset › should return the latest state using a future block hash | 11 ms |
| ✓ | a contract query by address and offset › should respond with a contract action according to the expected schema | 9 ms |
| ✓ | a contract query by address and offset › should return the correct action using exact block height where it was included | 7 ms |
| ✓ | a contract query by address and offset › should return the latest state using a future block height | 8 ms |
| ✓ | a contract query by address and offset › should return the most recent contract action for that address before the specified block | 14 ms |
| ✓ | a contract query by address and offset › should return null when contract with valid address and valid offset does not exist | 3 ms |
| ✓ | a contract query by address and offset › should return null when contract with valid address and non-existing hash does not exist | 2 ms |
| ✓ | a contract query by address and offset › should return error when contract with invalid address and valid hash | 5 ms |
| ✓ | a contract query by address and offset › should return error when contract with invalid address and non-existing hash | 2 ms |
| ✓ | a contract query by address and offset › should return error when contract with valid address and invalid hash | 2 ms |
| ✓ | a contract query by address and offset › should return error when contract with invalid address and invalid hash | 9 ms |
| ✓ | a contract query by address and offset › should return null when contract with valid address and valid height does not exist | 3 ms |
| ✓ | a contract query by address and offset › should return null when contract with valid address and non-existing height does not exist | 2 ms |
| ✓ | a contract query by address and offset › should return error when contract with invalid address and valid height | 2 ms |
| ✓ | a contract query by address and offset › should return error when contract with invalid address and invalid height | 5 ms |
| ✓ | a contract query by address and offset › should return error for negative height | 1 ms |
| ✓ | a contract query by address and offset › should return error for non-integer height | 2 ms |
| ✓ | a contract query by address and offset › should return error for extremely large height | 2 ms |
| ✓ | a contract query by address and offset › should return null when using a block hash from before the action existed | 54 ms |
| ✓ | the contractEvents field on contract actions › should expose contractEvents on ContractCall | 1 ms |
| ✓ | the contractEvents field on contract actions › should not expose contractEvents on ContractDeploy or ContractUpdate | 3 ms |
| ✓ | the block a contract state is anchored at › should anchor the latest contract action state at the block it was last written | 24 ms |
| ✓ | the block a contract state is anchored at › should anchor the contract state at the block of its latest action | 23 ms |
| ✓ | the block a contract state is anchored at › should anchor every recorded contract action at its own block | 27 ms |

#### contract queries across a ledger hard fork

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | a contract untouched since the fork queried without an offset › should serve the contract action state in the post-fork encoding of the node | — |
| ↓ | a contract untouched since the fork queried without an offset › should serve the contract state in the post-fork encoding of the node | — |
| ↓ | a contract untouched since the fork queried without an offset › should anchor the translated state at the fork block | — |
| ↓ | a contract untouched since the fork queried as of the blocks around it › should serve the state in the encoding of the queried block | — |
| ↓ | a contract untouched since the fork queried as of the blocks around it › should anchor the state at the latest action before the fork and at the fork block after | — |
| ↓ | the recorded contract actions of a contract untouched since the fork › should keep every recorded contract action in the pre-fork encoding | — |
| ↓ | the recorded contract actions of a contract untouched since the fork › should list the recorded actions anchored at their own blocks | — |

### Contract type queries

File name: `tests/integration/basic/queries/contract-type-queries.test.ts`

#### contract type queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | a contract query by address › should return the contract with state, maintenance authority and actions | — |
| ↓ | a contract query by address › should return null for a non-existent contract | — |
| ↓ | a contract query by address › should error for malformed contract addresses | — |
| ↓ | the maintenance authority › should expose a committee of valid verifying keys with threshold and counter | — |
| ☐ | the maintenance authority › should map an ECDSA committee verifying key | — |
| ↓ | the actions sub-query › should bound the actions list by limit | — |
| ↓ | the actions sub-query › should filter actions by type DEPLOY | — |
| ↓ | the actions sub-query › should filter actions by type CALL | — |
| ☐ | the actions sub-query › should filter actions by type UPDATE | — |
| ↓ | point-in-time state via offset › should return state as of a pinned block hash | — |
| ↓ | point-in-time state via offset › should return state as of a pinned block height | — |
| ↓ | point-in-time state via offset › should return null when queried at a block before the contract existed | — |

### Contract zswap state queries

File name: `tests/integration/basic/queries/contract-zswap-state-queries.test.ts`

#### contract zswap state queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | block.contractZswapState › should return the contract-filtered zswap state for a known contract | — |
| ↓ | block.contractZswapState › should return null for a non-existent contract | — |
| ↓ | block.contractZswapState › should return identical state for repeated queries pinned to the same block | — |
| ↓ | block.contractZswapState › should error for malformed contract addresses | — |
| ↓ | block.contractZswapState › should error for a malformed block-hash offset | — |
| ↓ | block.contractZswapState › should return the zswap state when the block is pinned by height | — |
| ↓ | the composed execution-inputs query › should resolve zswap state, ledger parameters and contract state anchored to one block | — |
| ↓ | the composed execution-inputs query › should return null zswap state and null contract for a non-existent contract | — |

### Dust commitment update queries

File name: `tests/integration/basic/queries/dust-commitment-update-queries.test.ts`

#### dust commitment merkle tree update queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a collapsed update query with valid index range › should return a collapsed update for a valid index range | 31 ms |
| ✓ | a collapsed update query with valid index range › should respond with a collapsed update according to the expected schema | 5 ms |
| ✓ | a collapsed update query with valid index range › should return a collapsed update for the full genesis dust range | 34 ms |
| ✓ | a collapsed update query with equal start and end indices › should return a valid update when startIndex equals endIndex | 3 ms |
| ✓ | a collapsed update query idempotency › should return identical results for the same query parameters | 5 ms |
| ✓ | a collapsed update query with invalid index range › should return an error when startIndex is greater than endIndex | 2 ms |
| ✓ | a collapsed update query with invalid index range › should return an error when indices are negative | 5 ms |
| ✓ | a collapsed update query with invalid index range › should return an error when endIndex is beyond the indexed range | 3 ms |

### Dust generation update queries

File name: `tests/integration/basic/queries/dust-generation-update-queries.test.ts`

#### dust generation merkle tree update queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | a collapsed update query with valid index range › should return a collapsed update for a valid index range | — |
| ↓ | a collapsed update query with valid index range › should respond with a collapsed update according to the expected schema | — |
| ↓ | a collapsed update query with valid index range › should return a collapsed update for the full genesis dust range | — |
| ↓ | a collapsed update query with equal start and end indices › should return a valid update when startIndex equals endIndex | — |
| ↓ | a collapsed update query idempotency › should return identical results for the same query parameters | — |
| ↓ | a collapsed update query with invalid index range › should return an error when startIndex is greater than endIndex | — |
| ↓ | a collapsed update query with invalid index range › should return an error when indices are negative | — |
| ↓ | a collapsed update query with invalid index range › should return an error when endIndex is beyond the indexed range | — |

### Dust generations queries

File name: `tests/integration/basic/queries/dust-generations-queries.test.ts`

#### dust generations queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | a dust generations query with a valid Cardano reward address › should return dust generations for a registered address | — |
| ↓ | a dust generations query with a valid Cardano reward address › should respond with dust generations according to the expected schema | — |
| ↓ | a dust generations query with a valid Cardano reward address › should return registrations with valid fields for a registered address | — |
| ↓ | a dust generations query with multiple addresses › should return dust generations for multiple addresses | — |
| ↓ | a dust generations query with non-registered address › should return empty registrations for a non-registered address | — |
| ↓ | a dust generations query with invalid input › should return an empty result for an empty addresses array | — |
| ↓ | a dust generations query with invalid input › should return an error for a malformed address | — |
| ↓ | a dustGenerations query with a multi-UTXO stake key (#926) › should aggregate nightBalance across all backing cNIGHT UTXOs | — |
| ↓ | backwards compatibility with dustGenerationStatus › should return consistent data from both endpoints for a registered address | — |

### Dust queries

File name: `tests/integration/basic/queries/dust-queries.test.ts`

#### dust generation status queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | a dust generation status query with a valid Cardano reward address › should respond with a dust generation status response according to the requested schema | — |
| ↓ | a dust generation status query with a valid Cardano reward address › should report registered status for a registered Cardano reward address | — |
| ↓ | a dust generation status query with a valid Cardano reward address › should give the DUST destination address for the expected network when Cardano reward address is registered | — |
| ↓ | a dust generation status query with a valid Cardano reward address › should correctly indicate registration status for a non-registered key | — |
| ↓ | a dust generation status query with a valid Cardano reward address › should indicate zero generation for registered address without cNIGHT balance | — |
| ↓ | a dust generation status query with a valid Cardano reward address › should report the correct value of max capacity for registered address with cNIGHT | — |
| ↓ | a dust generation status query with a valid Cardano reward address › should correctly indicate deregistered status for a previously registered Cardano reward address | — |
| ↓ | a dust generation status query with multiple valid Cardano reward addresses › should return statuses for multiple Cardano reward addresses in order | — |
| ↓ | a dust generation status query with multiple valid Cardano reward addresses › should respond with dust generation statuses according to the requested schema for multiple addresses | — |
| ↓ | a dust generation status query with multiple valid Cardano reward addresses › should return an error given the number of addresses is greater than 10 | — |
| ↓ | a dust generation status query with malformed reward addresses › should return an error when the address is in plain hex string format | — |
| ↓ | a dust generation status query with empty list of reward addresses › should return an empty list of dust generation statuses | — |
| ↓ | a dust generation status query with a Cardano payment address › should return an error as only Cardano reward addresses are supported | — |
| ↓ | a dust generation status query with a Cardano reward address not meant for this network › should return an error reporting the target network mismatch | — |
| ↓ | a dust generation status query with duplicate reward addresses › should handle duplicate Cardano reward addresses appropriately | — |

### Graphql http compression

File name: `tests/integration/basic/queries/graphql-http-compression.test.ts`

#### graphql http response compression

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a GraphQL HTTP request with Accept-Encoding: gzip › should return a gzip-compressed response | 31 ms |
| ✓ | a GraphQL HTTP request with Accept-Encoding: br › should return a brotli-compressed response | 8 ms |
| ✓ | a GraphQL HTTP request with Accept-Encoding: zstd › should return a zstd-compressed response | 6 ms |
| ✓ | a GraphQL HTTP request with Accept-Encoding: identity › should return an uncompressed response | 6 ms |

### Spo epoch series queries

File name: `tests/integration/basic/queries/spo-epoch-series-queries.test.ts`

#### SPO epoch series queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a registered totals series query with an epoch span within the maximum › should accept a range covering a single epoch | 39 ms |
| ✓ | a registered totals series query with an epoch span within the maximum › should accept a range whose span equals the maximum | 8 ms |
| ✓ | a registered totals series query with an epoch span within the maximum › should accept a permitted span that does not start at epoch zero | 8 ms |
| ✓ | a registered totals series query with an epoch span exceeding the maximum › should reject a range one epoch beyond the maximum span | 2 ms |
| ✓ | a registered totals series query with an epoch span exceeding the maximum › should reject a reversed range whose span exceeds the maximum | 3 ms |
| ✓ | a registered totals series query with an epoch span exceeding the maximum › should reject a range far beyond the maximum span without returning data | 2 ms |
| ✓ | a registered SPO series query with an epoch span exceeding the maximum › should reject a range beyond the maximum span | 2 ms |
| ✓ | a registered SPO series query with an epoch span exceeding the maximum › should accept a range whose span equals the maximum | 101 ms |
| ✓ | a registered presence query with an epoch span exceeding the maximum › should reject a range beyond the maximum span | 4 ms |
| ✓ | a registered presence query with an epoch span exceeding the maximum › should accept a range whose span equals the maximum | 5 ms |

### Transaction queries

File name: `tests/integration/basic/queries/transaction-queries.test.ts`

#### transaction queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a transaction query by hash › should return the transaction with that hash, given that transaction exists | 321 ms |
| ✓ | a transaction query by hash › should return an empty transaction list, given a transaction with that hash doesn't exist | 3 ms |
| ✓ | a transaction query by hash › should return an error, given a hash is invalid (malformed) | 11 ms |
| ✓ | a transaction query by hash › should return an error when called with an empty offset object | 2 ms |
| ✓ | a transaction query by identifier › should return the transaction with that identifier, given that transaction exists | 219 ms |
| ✓ | a transaction query by identifier › should return an empty list of transactions, given a transaction with that identifier doesn't exist | 3 ms |
| ✓ | a transaction query by identifier › should return an error, given an invalid identifier | 7 ms |
| ✓ | a transaction query by hash and identifier › should return an error, as only one parameter at a time can be used | 2 ms |

#### genesis transactions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | transaction queries to the genesis block transactions › should return utxos related to 4 pre-fund wallets | 279 ms |
| ✓ | transaction queries to the genesis block transactions › should return utxos with 1 token type | 256 ms |
| ✓ | transaction queries to the genesis block transactions › should return utxos sorted by outputIndex in ascending order | 213 ms |
| ✓ | transaction queries to the genesis block transactions › should return system transactions when queried by hash | 243 ms |
| ✓ | transaction queries to the genesis block transactions › should not contain RegularTransaction-specific fields on system transactions | 191 ms |
| ✓ | schema validation › should respond with full transaction data according to the expected schema | 8 ms |
| ✓ | schema validation › should respond with system transactions according to the expected schema | 0 ms |
| ✓ | schema validation › should contain both regular and system transactions | 0 ms |
| ✓ | schema validation › should respond with regular transactions according to the expected schema | 2 ms |
| ✓ | schema validation › should respond with nested ledger events and unshielded outputs according to the expected schema | 1 ms |

### Zswap collapsed update queries

File name: `tests/integration/basic/queries/zswap-collapsed-update-queries.test.ts`

#### zswap merkle tree collapsed update queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a collapsed update query with valid index range › should return a collapsed update for a valid index range | 32 ms |
| ✓ | a collapsed update query with valid index range › should respond with a collapsed update according to the expected schema | 5 ms |
| ✓ | a collapsed update query with valid index range › should return a collapsed update for the full genesis zswap range | 33 ms |
| ✓ | a collapsed update query with equal start and end indices › should return a valid update when startIndex equals endIndex | 3 ms |
| ✓ | a collapsed update query idempotency › should return identical results for the same query parameters | 5 ms |
| ✓ | a collapsed update query with invalid index range › should return an error when startIndex is greater than endIndex | 2 ms |
| ✓ | a collapsed update query with invalid index range › should return an error when indices are negative | 5 ms |
| ✓ | a collapsed update query with invalid index range › should return an error when endIndex is beyond the indexed range | 2 ms |

### Block subscriptions

File name: `tests/integration/basic/subscriptions/block-subscriptions.test.ts`

#### block subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a subscription to block updates without parameters › should stream blocks starting from the latest block | 5,189 ms |
| ✓ | a subscription to block updates without parameters › should stream blocks adhering to the expected schema | 541 ms |
| ✓ | a subscription to block updates by hash › should stream blocks starting from the block with that hash, given that hash exists | 603 ms |
| ✓ | a subscription to block updates by hash › should return an error message, given that hash doesn't exist | 507 ms |
| ✓ | a subscription to block updates by hash › should return an error message, given that hash is invalid | 504 ms |
| ✓ | a subscription to block updates by height › should stream blocks from the block with that height, given that height exists | 626 ms |
| ✓ | a subscription to block updates by height › should return an error message, given that height is higher than the latest block height | 512 ms |
| ✓ | a subscription to block updates by height › should return an error message, given that height is invalid | 504 ms |
| ✓ | a subscription to block updates by height and hash › should return an error message, as only one parameter at a time can be used | 506 ms |
| ↓ | regular transaction fees › should report ledger paidFees and estimatedFees on regular transactions | — |

### Bridge pool subscriptions

File name: `tests/integration/basic/subscriptions/bridge-pool-subscriptions.test.ts`

#### bridge pool subscription

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | bridgePoolUpdates › should emit current pool summary immediately on subscribe | — |
| ☐ | bridgePoolUpdates › should push an update when a new ReserveTransfer is indexed | — |
| ☐ | bridgePoolUpdates › should push an update when a new InvalidTransfer is indexed | — |
| ☐ | bridgePoolUpdates › should not push a pool update for UserTransfer events (user address, not pool) | — |
| ☐ | bridgePoolUpdates › should carry a running pool total in each frame (cumulative consistency) | — |
| ↓ | bridgePoolUpdates › should push an update when an UnapprovedTransfer is indexed | — |
| ↓ | bridgePoolUpdates › should push an update and increment subminimumTxCount when a flush is indexed | — |

### Bridge subscriptions

File name: `tests/integration/basic/subscriptions/bridge-subscriptions.test.ts`

#### bridge subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | bridgeEvents › should replay historical events from the given cursor id | — |
| ↓ | bridgeEvents › should only deliver events matching the recipient filter | — |
| ↓ | bridgeEvents › should only deliver events matching the variant filter | — |
| ↓ | bridgeEvents › should resume from cursor without gap or duplication on reconnection | — |
| ↓ | bridgeEvents › should deliver BridgeUnapprovedTransfer events via subscription | — |
| ☐ | claims via unshieldedTransactions › should observe claims as BridgeClaimTransaction via the unshieldedTransactions query | — |
| ↓ | bridgeBalance › should emit zero balance immediately for an address with no bridge activity | — |
| ☐ | bridgeBalance › should push an updated BridgeBalance when a relevant UserTransfer is indexed | — |
| ☐ | bridgeBalance › should push an updated BridgeBalance when a claim reduces the balance | — |

### Contract event subscriptions

File name: `tests/integration/basic/subscriptions/contract-event-subscriptions.test.ts`

#### contract event subscription

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a contract events subscription for an address with no emitted events › should complete via the toBlock terminator without delivering events | 520 ms |
| ✓ | a contract events subscription with an invalid filter › should return an error when the contract address is empty | 504 ms |
| ✓ | a contract events subscription with an invalid filter › should return an error for an unknown field prefix field name | 509 ms |
| ↓ | a contract events subscription for a contract with emitted events › should stream events conforming to the contract event schema | — |
| ↓ | a contract events subscription for a contract with emitted events › should stream only events carrying the filtered field | — |

### Contract subscriptions

File name: `tests/integration/basic/subscriptions/contract-subscriptions.test.ts`

#### contract action subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a subscription to contract action updates without parameters › should not replay existing contract actions | 8,578 ms |
| ✓ | a subscription to contract action updates from genesis › should stream contract actions adhering to the expected schema | 521 ms |
| ✓ | a subscription to contract action updates with block hash offset › should stream historical and new contract actions from a specific block hash | 512 ms |
| ✓ | a subscription to contract action updates with the block their state is anchored at › should anchor each streamed contract action at the block its state was written | 9,026 ms |
| ↓ | a subscription from before a ledger hard fork to a contract untouched since › should re-emit the latest pre-fork action once at the fork block | — |

### Dust generations subscriptions

File name: `tests/integration/basic/subscriptions/dust-generations-subscriptions.test.ts`

#### dust generations subscription

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | a subscription at the latest block hash › should stream a complete generation snapshot for a registered dust address | — |
| ↓ | a subscription pinned to a block hash › should deliver identical events for repeated subscriptions at the same block | — |
| ↓ | a subscription pinned to a block hash › should snapshot the generation tree at the queried block rather than the tip | — |
| ↓ | dtime update delivery relative to the cutoff height › should replay owned dtime updates before generation items when the cutoff is zero | — |
| ↓ | dtime update delivery relative to the cutoff height › should deliver no dtime updates when the cutoff equals the snapshot block height | — |
| ↓ | subscription error handling › should return an error for an invalid dust address | — |
| ↓ | subscription error handling › should return an error for a valid address that is meant for another networkid | — |
| ↓ | subscription error handling › should return an error for a valid dust address passed in hex format | — |
| ↓ | subscription error handling › should return an error for an unknown block hash | — |
| ↓ | subscription error handling › should return an error for a malformed block hash | — |
| ↓ | subscription error handling › should reject a block hash older than the snapshot freshness window | — |
| ↓ | transactionHash on dust generation events › first item transactionHash resolves via transactions(offset) | — |

#### dust generations progress protocol version

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a subscription pinned to a block hash › should report the pinned block protocol version in the progress event | 558 ms |

### Dust ledger subscriptions

File name: `tests/integration/basic/subscriptions/dust-ledger-subscriptions.test.ts`

#### dust ledger event subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a subscription to dust ledger events without offset (default replay) › streams events in ledger order | 525 ms |
| ✓ | subscription with explicit offset › streams events in ledger order starting from the specified ID | 511 ms |
| ✓ | subscription with explicit offset › validates historical dust events against schema | 508 ms |
| ✓ | subscription error handling › should return an error for unknown field | 503 ms |
| ✓ | subscription error handling › rejects negative offset ID with an error | 503 ms |

### Dust nullifier subscriptions

File name: `tests/integration/basic/subscriptions/dust-nullifier-subscriptions.test.ts`

#### dust nullifier transactions subscription

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | streaming dust nullifier transactions with block range › should stream transactions within a block range and complete | 543 ms |
| ✓ | subscription error handling › should return an error for empty nullifier prefixes | 505 ms |
| ✓ | subscription error handling › should return an error when fromBlock is greater than toBlock | 504 ms |
| ↓ | transactionHash on dust nullifier events (#1114) › first event transactionHash resolves via transactions(offset) | — |
| ↓ | transaction reference on dust nullifier events (#1115) › first event transaction.hash matches transactionHash | — |

### Shielded nullifier subscriptions

File name: `tests/integration/basic/subscriptions/shielded-nullifier-subscriptions.test.ts`

#### shielded nullifier transactions subscription

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | streaming shielded nullifier transactions with block range › should stream transactions within a block range and complete | 561 ms |
| ✓ | subscription error handling › should return an error for empty nullifier prefixes | 506 ms |
| ✓ | subscription error handling › should return an error for an empty-string nullifier prefix element | 506 ms |
| ✓ | subscription error handling › should return an error when fromBlock is greater than toBlock | 504 ms |
| ↓ | transactionHash on shielded nullifier events (#1114) › first event transactionHash resolves via transactions(offset) | — |
| ↓ | transaction reference on shielded nullifier events (#1115) › first event transaction.hash matches transactionHash | — |

### Shielded transaction subscriptions

File name: `tests/integration/basic/subscriptions/shielded-transaction-subscriptions.test.ts`

#### shielded transaction subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | opening a session with viewing key › should return a session ID, given a valid viewing key | 582 ms |
| ✓ | opening a session with viewing key › should return an error, given an unsupported hex format viewing key | 509 ms |
| ✓ | opening a session with viewing key › should return an error, given an invalid viewing key | 504 ms |
| ✓ | opening a session with viewing key › should return an error, given a valid viewing key meant for a different network | 827 ms |
| ✓ | closing a session with session ID › should terminate the session successfully, given a valid session ID | 558 ms |
| ✓ | closing a session with session ID › should return an error, given an invalid session ID | 503 ms |
| ✓ | a subscription to wallet updates providing viewing key only › should stream wallet events starting from the beginning, given there are relevant transactions | 2,551 ms |
| ✓ | a subscription to wallet updates providing viewing key only › should stream shielded transaction events adhering to the expected schema | 3,554 ms |
| ✓ | a subscription to wallet updates providing viewing key only › should be able to use highestZswapEndIndex from progress event in collapsed update query | 569 ms |
| ✓ | a subscription to wallet updates providing viewing key only › should report the chain tip protocol version in the progress event | 557 ms |
| ✓ | a subscription to wallet updates providing viewing key only › should reject shieldedTransactions subscription when using expired session ID | 564 ms |

### Subscription quotas

File name: `tests/integration/basic/subscriptions/subscription-quotas.test.ts`

#### subscription quotas

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | per-connection concurrent subscription cap › should reject a subscription beyond the per-connection cap | 1,516 ms |
| ✓ | per-connection concurrent subscription cap › should free a slot when an active subscription is closed | 2,515 ms |
| ✓ | per-session subscription creation rate limit › should reject shielded subscription creations beyond the per-session rate limit | 1,560 ms |
| ✓ | per-session subscription creation rate limit › should allow a new shielded subscription after the rate limit refills | 9,569 ms |
| ✓ | per-session subscription creation rate limit › should keep a live shielded subscription streaming while creations are rate-limited | 35,658 ms |

### Unshielded transaction subscriptions

File name: `tests/integration/basic/subscriptions/unshielded-transaction-subscriptions.test.ts`

#### unshielded transaction subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a subscription to unshielded transaction events by address › should stream unshielded transaction events related to that address, given that address has transactions | 5,069 ms |
| ✓ | a subscription to unshielded transaction events by address › should stream unshielded transaction events up to highest transaction id | 77 ms |
| ✓ | a subscription to unshielded transaction events by address › should stream unshielded transaction events that adhere to the expected schema | 72 ms |
| ✓ | a subscription to unshielded transaction events by address › should only return a transaction progress message with highest transaction = 0, given that address does not have transactions | 5,038 ms |
| ✓ | a subscription to unshielded transaction events by address › should report the chain tip protocol version in the progress update, given that address does not have transactions | 5,071 ms |
| ✓ | a subscription to unshielded transaction events by address › should return an error message, given the address provided is in hex format | 4 ms |
| ✓ | a subscription to unshielded transaction events by address › should return an error message, given the address provided is for another network | 283 ms |
| ✓ | a subscription to unshielded transaction events by address and transaction id › should return a stream of transactions containing that address, starting from transaction id = 0 | 72 ms |
| ✓ | a subscription to unshielded transaction events by address and transaction id › should return an error message, given the transaction id provided is negative | 44 ms |
| ✓ | a subscription to unshielded transaction events by address and transaction id › should only return a transaction progress message without streaming transactions, given that the transaction id provided is bigger number | 5,043 ms |
| ✓ | a subscription to unshielded transaction events by address and transaction id › should start a transaction stream from the given transaction id | 10,051 ms |
| ✓ | a subscription to unshielded transaction events by address and transaction id › should return an error message, given the address is provided in hex format | 2 ms |

### Wallet connect options

File name: `tests/integration/basic/subscriptions/wallet-connect-options.test.ts`

#### wallet connect options (startIndex)

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | opening a session with startIndex › should accept startIndex = 0 (no-op equivalent of unset options) | — |
| ↓ | opening a session with startIndex › should skip historical scan when startIndex equals the current tip | — |
| ↓ | opening a session with startIndex › should accept startIndex past the current tip (fast-forward) | — |

### Websocket keepalive

File name: `tests/integration/basic/subscriptions/websocket-keepalive.test.ts`

#### websocket keepalive

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | an idle socket without the client keepalive › should be closed while idle past the idle limit | — |
| ✓ | an idle socket with the client keepalive › should stay open while idle past the idle limit | 150,537 ms |

### Zswap events subscriptions

File name: `tests/integration/basic/subscriptions/zswap-events-subscriptions.test.ts`

#### zswap ledger event subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a subscription to zswap ledger events without offset (default replay) › streams events in ledger order | 527 ms |
| ✓ | subscription with explicit offset › streams events starting from the specified ID | 519 ms |
| ✓ | subscription with explicit offset › validates historical zswap events against schema | 518 ms |
| ✓ | subscription error handling › should return an error for unknown field | 504 ms |
| ✓ | subscription error handling › rejects negative offset ID with an error | 506 ms |
## End-to-end results (toolkit)

### Contract actions partial success

File name: `tests/e2e/contract-actions-partial-success.test.ts`

#### contract actions in a guaranteed/fallible segment split

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a Call whose guaranteed transcript applied but whose fallible segment failed › the fixture produces a call with a guaranteed transcript | 1 ms |
| ✓ | a Call whose guaranteed transcript applied but whose fallible segment failed › the fixture produces a partially successful transaction | 0 ms |
| ✓ | a Call whose guaranteed transcript applied but whose fallible segment failed › is still reported by the indexer | 1 ms |
| ✓ | a Call where neither execution phase applied › the fixture produces a call without a guaranteed transcript | 0 ms |
| ✓ | a Call where neither execution phase applied › the fixture still produces a partially successful transaction | 0 ms |
| ✓ | a Call where neither execution phase applied › is not reported by the indexer | 0 ms |

### Contract actions sequential

File name: `tests/e2e/contract-actions-sequential.test.ts`

#### contract actions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a transaction to deploy a counter smart contract › should be reported by the indexer through a transaction query by hash | 11 ms |
| ✓ | a transaction to deploy a counter smart contract › should be reported by the indexer through a block query by hash | 16 ms |
| ✓ | a transaction to deploy a counter smart contract › should be reported by the indexer through a contract action query by address | 11 ms |
| ✓ | a transaction to call the store circuit on the deployed counter smart contract › should stream only the call to a subscription opened without an offset before it | 5,007 ms |
| ✓ | a transaction to call the store circuit on the deployed counter smart contract › should be reported by the indexer through a transaction query by hash | 33 ms |
| ✓ | a transaction to call the store circuit on the deployed counter smart contract › should be reported by the indexer through a block query by hash | 24 ms |
| ✓ | a transaction to call the store circuit on the deployed counter smart contract › should be reported by the indexer through a contract action query by address | 10 ms |
| ✓ | a transaction to update the counter smart contract › should be reported by the indexer through a transaction query by hash | 12 ms |
| ✓ | a transaction to update the counter smart contract › should be reported by the indexer through a block query by hash | 28 ms |
| ✓ | a transaction to update the counter smart contract › should be reported by the indexer through a contract action query by address | 18 ms |

### Contract deploy failed segment

File name: `tests/e2e/contract-deploy-failed-segment.test.ts`

#### a contract deploy in a rolled-back fallible segment

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | the crafted transaction › should carry two deploys of the same contract | 1 ms |
| ✓ | the crafted transaction › should be included in a block the node serves | 0 ms |
| ✓ | what the node holds afterwards › should report an ordinary deploy as present | 0 ms |
| ✓ | what the node holds afterwards › should hold no contract at the rolled-back address | 0 ms |
| ✓ | what the indexer reports › should keep advancing past that block | 18,079 ms |
| ✓ | what the indexer reports › should report no deploy for the rolled-back address | 26 ms |
| ☐ | what the indexer reports › should not increment indexer_uncaptured_contract_state_count | — |

### Contract shielded balance

File name: `tests/e2e/contract-shielded-balance.test.ts`

#### a contract whose balance map holds a shielded token

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | the holdShielded call should declare a shielded token in its unshielded inputs | 3 ms |
| ✓ | the indexer should index the block that holds the holdShielded call | 20,436 ms |
| ✓ | the indexer should keep indexing past that block | 22,756 ms |
| ✓ | the holdShielded contract action should report no unshielded balance | 13 ms |
| ✓ | the later ping contract action should also report no unshielded balance | 8 ms |

### Custom unshielded token transactions

File name: `tests/e2e/custom-unshielded-token-transactions.test.ts`

#### unshielded custom token transactions

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | a successful unshielded transaction transferring 1 token between two addresses › should be reported by the indexer through a block query by hash | — |
| ↓ | a successful unshielded transaction transferring 1 token between two addresses › should be reported by the indexer through a transaction query by hash | — |
| ↓ | a successful unshielded transaction transferring 1 token between two addresses › should be reported by the indexer through an unshielded transaction event for the source address | — |
| ↓ | a successful unshielded transaction transferring 1 token between two addresses › should be reported by the indexer through an unshielded transaction event for the destination address | — |
| ↓ | a successful unshielded transaction transferring 1 token between two addresses › should have transferred 1 token from the source to the destination address | — |
| ↓ | a successful unshielded transaction transferring 1 token between two addresses › should report the token type under test on every created and spent output | — |
| ↓ | a successful unshielded transaction transferring 1 token between two addresses › should be reported by the indexer through a progress update event for the source address | — |
| ↓ | a successful unshielded transaction transferring 1 token between two addresses › should be reported by the indexer through a progress update event for the destination address | — |

### Night transactions

File name: `tests/e2e/night-transactions.test.ts`

#### unshielded NIGHT transactions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses › should be reported by the indexer through a block query by hash | 36 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses › should be reported by the indexer through a transaction query by hash | 17 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses › should be reported by the indexer through an unshielded transaction event for the source address | 1 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses › should be reported by the indexer through an unshielded transaction event for the destination address | 1 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses › should have transferred 1 STAR from the source to the destination address | 19 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses › should report the token type under test on every created and spent output | 21 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses › should be reported by the indexer through a progress update event for the source address | 5,006 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses › should be reported by the indexer through a progress update event for the destination address | 1 ms |
| ✓ | the genesis block of a chain minted with NIGHT alone › should report the all-zeros NIGHT token type on its outputs | 2 ms |
| ✓ | the dust activity of a confirmed NIGHT transaction › should increase the dust commitment Merkle tree end index | 28 ms |
| ✓ | a confirmed unshielded transfer streamed to address subscriptions › should emit UnshieldedTransaction only for the target wallet (A > B1) | 19,936 ms |
| ✓ | a confirmed unshielded transfer streamed to address subscriptions › should emit UnshieldedTransaction only for the target wallet (A > B2) | 18,796 ms |
| ✓ | an address with no transaction history › should emit only ProgressUpdate for empty wallet | 2,602 ms |

### Segment probe contract actions

File name: `tests/e2e/segment-probe-contract-actions.test.ts`

#### segment-split contract actions

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | a transaction to deploy a segment-split smart contract › should be reported by the indexer through a transaction query by hash | — |
| ↓ | a transaction to deploy a segment-split smart contract › should be reported by the indexer through a block query by hash | — |
| ↓ | a transaction to deploy a segment-split smart contract › should be reported by the indexer through a contract action query by address | — |
| ↓ | a transaction to call the burnWithGuaranteed circuit on the deployed segment-split smart contract › should be reported by the indexer through a transaction query by hash | — |
| ↓ | a transaction to call the burnWithGuaranteed circuit on the deployed segment-split smart contract › should be reported by the indexer through a block query by hash | — |
| ↓ | a transaction to call the burnWithGuaranteed circuit on the deployed segment-split smart contract › should be reported by the indexer through a contract action query by address | — |

### Shielded transactions

File name: `tests/e2e/shielded-transactions.test.ts`

#### shielded transactions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a successful shielded transaction transferring 1 Shielded Token between two wallets › should be reported by the indexer through a block query by hash | 41 ms |
| ✓ | a successful shielded transaction transferring 1 Shielded Token between two wallets › should be reported by the indexer through a transaction query by hash | 27 ms |
| ✓ | a successful shielded transaction transferring 1 Shielded Token between two wallets › should stream Zswap events followed by DustSpendProcessed after a shielded transaction | 538 ms |
| ✓ | a successful shielded transaction transferring 1 Shielded Token between two wallets › should increase the zswap Merkle tree end index | 26 ms |
| ✓ | a successful shielded transaction transferring 1 Shielded Token between two wallets › should increase the dust commitment Merkle tree end index | 19 ms |
| ✓ | a confirmed shielded transfer streamed to wallet sessions by viewing key › should stream the transaction to the source viewing key with a matching hash | 3,874 ms |
| ✓ | a confirmed shielded transfer streamed to wallet sessions by viewing key › should stream the transaction to the destination viewing key with a matching hash | 560 ms |
| ✓ | a confirmed shielded transfer streamed to wallet sessions by viewing key › should not stream the transaction to an unrelated viewing key | 20,552 ms |

### Subscription polling

File name: `tests/e2e/subscription-polling.test.ts`

#### subscription polling

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | an unshielded transactions subscription for a never-used address › should emit progress immediately and back off while the address is idle | 96,394 ms |
| ✓ | an unshielded transactions subscription for a never-used address › should reset the polling interval when the address receives a transaction | 129,144 ms |
| ✓ | the keep-alive heartbeat of a shielded transactions subscription › should refresh wallet activity once at subscription start and then throttle | 92,777 ms |
| ☐ | the keep-alive heartbeat of a shielded transactions subscription › should keep a subscribed wallet active beyond the wallet-indexer inactivity TTL | — |
| ✓ | a shielded transactions subscription on an idle wallet › should emit progress immediately and keep emitting while idle | 34,176 ms |

### Unshielded balance e2e

File name: `tests/e2e/unshielded-balance-e2e.test.ts`

#### unshielded balance indexing (deploy + mint + verify) [#1253 scaffold]

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | reports the contract remainder after mint-to-self then partial send | — |

### Dust balance

File name: `tests/e2e/toolkit/dust-balance.test.ts`

#### dust balance query using toolkit

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a dust balance query with a valid wallet seed › should respond with a dust balance according to the requested schema | 608 ms |

### Key material

File name: `tests/e2e/toolkit/key-material.test.ts`

#### key material derivation validation

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a midnight shielded address › should show with the expected prefix for the current network ID | 56 ms |
| ✓ | a midnight shielded address › should show with the expected prefix for all network IDs | 303 ms |
| ✓ | a midnight unshielded address › should show with the expected prefix for the current network ID | 47 ms |
| ✓ | a midnight unshielded address › should show with the expected prefix for all network IDs | 340 ms |
| ✓ | a midnight viewing key › should show with the expected prefix for the current network ID | 51 ms |
| ✓ | a midnight viewing key › should show with the expected prefix for all network IDs | 346 ms |

### Show wallet queries

File name: `tests/e2e/toolkit/show-wallet-queries.test.ts`

#### show wallet queries using toolkit

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | private wallet state query using toolkit › should respond with a private wallet state according to the requested schema | 630 ms |
| ✓ | public wallet state query using toolkit › should respond with a public wallet state according to the requested schema | 857 ms |
## End-to-end results (moth)

### Contract actions partial success

File name: `tests/e2e/contract-actions-partial-success.test.ts`

#### contract actions in a guaranteed/fallible segment split

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | a Call whose guaranteed transcript applied but whose fallible segment failed › the fixture produces a call with a guaranteed transcript | — |
| ↓ | a Call whose guaranteed transcript applied but whose fallible segment failed › the fixture produces a partially successful transaction | — |
| ↓ | a Call whose guaranteed transcript applied but whose fallible segment failed › is still reported by the indexer | — |
| ↓ | a Call where neither execution phase applied › the fixture produces a call without a guaranteed transcript | — |
| ↓ | a Call where neither execution phase applied › the fixture still produces a partially successful transaction | — |
| ↓ | a Call where neither execution phase applied › is not reported by the indexer | — |

### Contract actions sequential

File name: `tests/e2e/contract-actions-sequential.test.ts`

#### contract actions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a transaction to deploy a counter smart contract › should be reported by the indexer through a transaction query by hash | 24 ms |
| ✓ | a transaction to deploy a counter smart contract › should be reported by the indexer through a block query by hash | 13 ms |
| ✓ | a transaction to deploy a counter smart contract › should be reported by the indexer through a contract action query by address | 11 ms |
| ✓ | a transaction to call the store circuit on the deployed counter smart contract › should stream only the call to a subscription opened without an offset before it | 5,005 ms |
| ✓ | a transaction to call the store circuit on the deployed counter smart contract › should be reported by the indexer through a transaction query by hash | 23 ms |
| ✓ | a transaction to call the store circuit on the deployed counter smart contract › should be reported by the indexer through a block query by hash | 21 ms |
| ✓ | a transaction to call the store circuit on the deployed counter smart contract › should be reported by the indexer through a contract action query by address | 9 ms |
| ✓ | a transaction to update the counter smart contract › should be reported by the indexer through a transaction query by hash | 8 ms |
| ✓ | a transaction to update the counter smart contract › should be reported by the indexer through a block query by hash | 22 ms |
| ✓ | a transaction to update the counter smart contract › should be reported by the indexer through a contract action query by address | 19 ms |

### Contract deploy failed segment

File name: `tests/e2e/contract-deploy-failed-segment.test.ts`

#### a contract deploy in a rolled-back fallible segment

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | the crafted transaction › should carry two deploys of the same contract | — |
| ↓ | the crafted transaction › should be included in a block the node serves | — |
| ↓ | what the node holds afterwards › should report an ordinary deploy as present | — |
| ↓ | what the node holds afterwards › should hold no contract at the rolled-back address | — |
| ↓ | what the indexer reports › should keep advancing past that block | — |
| ↓ | what the indexer reports › should report no deploy for the rolled-back address | — |
| ☐ | what the indexer reports › should not increment indexer_uncaptured_contract_state_count | — |

### Contract shielded balance

File name: `tests/e2e/contract-shielded-balance.test.ts`

#### a contract whose balance map holds a shielded token

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | the holdShielded call should declare a shielded token in its unshielded inputs | 1 ms |
| ✓ | the indexer should index the block that holds the holdShielded call | 16,705 ms |
| ✓ | the indexer should keep indexing past that block | 24,094 ms |
| ✓ | the holdShielded contract action should report no unshielded balance | 7 ms |
| ✓ | the later ping contract action should also report no unshielded balance | 6 ms |

### Custom unshielded token transactions

File name: `tests/e2e/custom-unshielded-token-transactions.test.ts`

#### unshielded custom token transactions

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | a successful unshielded transaction transferring 1 token between two addresses › should be reported by the indexer through a block query by hash | — |
| ↓ | a successful unshielded transaction transferring 1 token between two addresses › should be reported by the indexer through a transaction query by hash | — |
| ↓ | a successful unshielded transaction transferring 1 token between two addresses › should be reported by the indexer through an unshielded transaction event for the source address | — |
| ↓ | a successful unshielded transaction transferring 1 token between two addresses › should be reported by the indexer through an unshielded transaction event for the destination address | — |
| ↓ | a successful unshielded transaction transferring 1 token between two addresses › should have transferred 1 token from the source to the destination address | — |
| ↓ | a successful unshielded transaction transferring 1 token between two addresses › should report the token type under test on every created and spent output | — |
| ↓ | a successful unshielded transaction transferring 1 token between two addresses › should be reported by the indexer through a progress update event for the source address | — |
| ↓ | a successful unshielded transaction transferring 1 token between two addresses › should be reported by the indexer through a progress update event for the destination address | — |

### Night transactions

File name: `tests/e2e/night-transactions.test.ts`

#### unshielded NIGHT transactions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses › should be reported by the indexer through a block query by hash | 34 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses › should be reported by the indexer through a transaction query by hash | 15 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses › should be reported by the indexer through an unshielded transaction event for the source address | 1 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses › should be reported by the indexer through an unshielded transaction event for the destination address | 1 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses › should have transferred 1 STAR from the source to the destination address | 18 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses › should report the token type under test on every created and spent output | 25 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses › should be reported by the indexer through a progress update event for the source address | 5,003 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses › should be reported by the indexer through a progress update event for the destination address | 0 ms |
| ✓ | the genesis block of a chain minted with NIGHT alone › should report the all-zeros NIGHT token type on its outputs | 1 ms |
| ✓ | the dust activity of a confirmed NIGHT transaction › should increase the dust commitment Merkle tree end index | 17 ms |
| ✓ | a confirmed unshielded transfer streamed to address subscriptions › should emit UnshieldedTransaction only for the target wallet (A > B1) (passed on retry; first attempt: `AssertionError: expected 3n to be -49999999999995n // Object.is equality`) | 44,117 ms |
| ✓ | a confirmed unshielded transfer streamed to address subscriptions › should emit UnshieldedTransaction only for the target wallet (A > B2) | 15,603 ms |
| ✓ | an address with no transaction history › should emit only ProgressUpdate for empty wallet | 2,577 ms |

### Segment probe contract actions

File name: `tests/e2e/segment-probe-contract-actions.test.ts`

#### segment-split contract actions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a transaction to deploy a segment-split smart contract › should be reported by the indexer through a transaction query by hash | 8 ms |
| ✓ | a transaction to deploy a segment-split smart contract › should be reported by the indexer through a block query by hash | 10 ms |
| ✓ | a transaction to deploy a segment-split smart contract › should be reported by the indexer through a contract action query by address | 8 ms |
| ✓ | a transaction to call the burnWithGuaranteed circuit on the deployed segment-split smart contract › should be reported by the indexer through a transaction query by hash | 7 ms |
| ✓ | a transaction to call the burnWithGuaranteed circuit on the deployed segment-split smart contract › should be reported by the indexer through a block query by hash | 12 ms |
| ✓ | a transaction to call the burnWithGuaranteed circuit on the deployed segment-split smart contract › should be reported by the indexer through a contract action query by address | 6 ms |

### Shielded transactions

File name: `tests/e2e/shielded-transactions.test.ts`

#### shielded transactions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a successful shielded transaction transferring 1 Shielded Token between two wallets › should be reported by the indexer through a block query by hash | 53 ms |
| ✓ | a successful shielded transaction transferring 1 Shielded Token between two wallets › should be reported by the indexer through a transaction query by hash | 13 ms |
| ✓ | a successful shielded transaction transferring 1 Shielded Token between two wallets › should stream Zswap events followed by DustSpendProcessed after a shielded transaction | 533 ms |
| ✓ | a successful shielded transaction transferring 1 Shielded Token between two wallets › should increase the zswap Merkle tree end index | 24 ms |
| ✓ | a successful shielded transaction transferring 1 Shielded Token between two wallets › should increase the dust commitment Merkle tree end index | 13 ms |
| ✓ | a confirmed shielded transfer streamed to wallet sessions by viewing key › should stream the transaction to the source viewing key with a matching hash | 1,444 ms |
| ✓ | a confirmed shielded transfer streamed to wallet sessions by viewing key › should stream the transaction to the destination viewing key with a matching hash | 524 ms |
| ✓ | a confirmed shielded transfer streamed to wallet sessions by viewing key › should not stream the transaction to an unrelated viewing key | 20,519 ms |

### Subscription polling

File name: `tests/e2e/subscription-polling.test.ts`

#### subscription polling

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | an unshielded transactions subscription for a never-used address › should emit progress immediately and back off while the address is idle | 83,406 ms |
| ✓ | an unshielded transactions subscription for a never-used address › should reset the polling interval when the address receives a transaction | 143,887 ms |
| ✓ | the keep-alive heartbeat of a shielded transactions subscription › should refresh wallet activity once at subscription start and then throttle | 92,814 ms |
| ☐ | the keep-alive heartbeat of a shielded transactions subscription › should keep a subscribed wallet active beyond the wallet-indexer inactivity TTL | — |
| ✓ | a shielded transactions subscription on an idle wallet › should emit progress immediately and keep emitting while idle | 35,663 ms |

### Unshielded balance e2e

File name: `tests/e2e/unshielded-balance-e2e.test.ts`

#### unshielded balance indexing (deploy + mint + verify) [#1253 scaffold]

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | reports the contract remainder after mint-to-self then partial send | — |

### Dust balance

File name: `tests/e2e/toolkit/dust-balance.test.ts`

#### dust balance query using moth

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a dust balance query with a valid wallet seed › should respond with a dust balance according to the requested schema | 5,396 ms |

### Key material

File name: `tests/e2e/toolkit/key-material.test.ts`

#### key material derivation validation

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | a midnight shielded address › should show with the expected prefix for the current network ID | — |
| ↓ | a midnight shielded address › should show with the expected prefix for all network IDs | — |
| ↓ | a midnight unshielded address › should show with the expected prefix for the current network ID | — |
| ↓ | a midnight unshielded address › should show with the expected prefix for all network IDs | — |
| ↓ | a midnight viewing key › should show with the expected prefix for the current network ID | — |
| ↓ | a midnight viewing key › should show with the expected prefix for all network IDs | — |

### Show wallet queries

File name: `tests/e2e/toolkit/show-wallet-queries.test.ts`

#### show wallet queries using moth

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | private wallet state query using moth › should respond with a private wallet state according to the requested schema | 5,362 ms |
| ✓ | public wallet state query using moth › should respond with a public wallet state according to the requested schema | 30 ms |
