# Indexer Test Execution Report

## Run Details

| Field | Value |
| --- | --- |
| Component Under Test | Indexer / Node |
| Indexer Version | 4.3.800-rc.2 |
| Node Version | 1.0.400 (`1.0.400-c338b9ac`) |
| Test Suites | smoke, integration, e2e |
| Tests | Indexer GraphQL API, integration with Node and end-to-end transaction flows |
| Environment | qanet |
| Date | Friday 16 Oct 2026 |
| QA Contact | Oleksandr Romanov (oleksandr.romanov@shielded.io) |

## Summary

| Suite | Test Files | Passed | Failed | Skipped | Todo | Total | Duration | Run at |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- |
| smoke | 3 passed, 3 skipped (6) | 18 | 0 | 12 | 0 | 30 | 4.68s | 18:37 EEST |
| integration | 20 passed, 1 failed (retested on preview), 9 skipped (30) | 189 | 1 | 93 | 20 | 303 | 506.40s | 18:36 EEST |
| e2e | 8 passed, 3 skipped (11) | 53 | 0 | 11 | 1 | 65 | 594.57s | 18:50 EEST |
| **Total** | | **260** | **1** | **116** | **21** | **398** | | |

> Legend: ✓ passed · ✗ failed · ↓ skipped · □ todo.
> Duration is the total test time reported in each suite's `junit-test-results.xml`.

## Failures

### ✗ contract queries a contract query by address should return non-empty unshieldedBalances for contracts known to hold tokens

- Suite: integration
- File: `tests/integration/basic/queries/contract-queries.test.ts:163`

NOTE: 
- Root cause: indexer qanet was not synced from scratch, so the fix have not been applied.
- Fix have been retested against preview environment, indexer 4.3.800-rc.2 (synced from 0) - and it successfully works.

```
TARGET_ENV=preview bun run vitest run --project integration \
    tests/integration/basic/queries/contract-queries.test.ts \
    -t "should return non-empty unshieldedBalances for contracts known to hold tokens"

✓ contract queries > a contract query by address > should return non-empty unshieldedBalances for contracts known to hold tokens  730ms

Test Files  1 passed (1)
     Tests  1 passed | 24 skipped (25)
  Duration  1.28s
```

## Smoke results

### Bridge schema

File name: `tests/smoke/bridge-schema.test.ts`

#### Bridge GraphQL Schema

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | types, should expose BridgeEvent interface with flat blockHeight | — |
| ↓ | types, should expose the five concrete BridgeEvent variants | — |
| ↓ | types, should expose BridgeSubminimalFlushTransfer without a cardanoTxHash | — |
| ↓ | types, should expose BridgeClaimTransaction implementing Transaction | — |
| ↓ | types, should expose BridgeBalance with deposited, claimed, balance | — |
| ↓ | types, should expose BridgeEventVariant enum with five values | — |
| ↓ | queries, should expose the bridge query fields on Query | — |
| ↓ | subscriptions, should expose the bridge subscription fields on Subscription | — |

### Graphql healthchecks

File name: `tests/smoke/graphql-healthchecks.test.ts`

#### Graphql Health Checks

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | an introspection message sent to the websocket channel, should return the supported graphql schema | 732 ms |
| ✓ | an introspection message sent to the websocket channel, should return an error, given the depth of the query is > 15 | 719 ms |
| ✓ | an introspection request sent to the http channel, should return the supported graphql schema | 341 ms |
| ✓ | an introspection request sent to the http channel, should return an error, given the depth of the query is > 15 | 93 ms |

### Graphql schema change

File name: `tests/smoke/graphql-schema-change.test.ts`

#### GraphQL Schema Stability Check

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | should not change the schema unexpectedly | — |

### Graphql schema deprecations

File name: `tests/smoke/graphql-schema-deprecations.test.ts`

#### Graphql Schema Deprecations

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | RegularTransaction.merkleTreeRoot, should be marked deprecated with reason "Use zswapMerkleTreeRoot instead" | 446 ms |
| ✓ | RegularTransaction.startIndex, should be marked deprecated with reason "Use zswapStartIndex instead" | 83 ms |
| ✓ | RegularTransaction.endIndex, should be marked deprecated with reason "Use zswapEndIndex instead" | 85 ms |
| ✓ | RelevantTransaction.collapsedMerkleTree, should be marked deprecated with reason "Use zswapCollapsedUpdate instead" | 72 ms |
| ✓ | ShieldedTransactionsProgress.highestEndIndex, should be marked deprecated with reason "Use highestZswapEndIndex instead" | 84 ms |
| ✓ | ShieldedTransactionsProgress.highestCheckedEndIndex, should be marked deprecated with reason "Use highestCheckedZswapEndIndex instead" | 75 ms |
| ✓ | ShieldedTransactionsProgress.highestRelevantEndIndex, should be marked deprecated with reason "Use highestRelevantZswapEndIndex instead" | 79 ms |
| ✓ | RegularTransaction.fees, should be marked deprecated with reason "Use fee instead" | 89 ms |
| ✓ | TransactionFees.estimatedFees, should be marked deprecated with reason "Use paidFees instead" | 73 ms |

### Progress protocol version schema

File name: `tests/smoke/progress-protocol-version-schema.test.ts`

#### Progress Protocol Version Schema

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | UnshieldedTransactionsProgress.protocolVersion, should be served as a non-null Int | — |
| ↓ | ShieldedTransactionsProgress.protocolVersion, should be served as a non-null Int | — |
| ↓ | DustGenerationsProgress.protocolVersion, should be served as a non-null Int | — |

### Service healthchecks

File name: `tests/smoke/service-healthchecks.test.ts`

#### Service Health Checks

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a request to the /ready endpoint, should return a 200 status code OK | 430 ms |
| ✓ | a request to an unrecognised path /api/v3/__regression_unknown_path, should not 308 to a version-double-prefixed Location | 229 ms |
| ✓ | a request to an unrecognised path /api/v3/__regression_unknown_path, should terminate when redirects are followed | 75 ms |
| ✓ | a request to an unrecognised path /api/v4/__regression_unknown_path, should not 308 to a version-double-prefixed Location | 76 ms |
| ✓ | a request to an unrecognised path /api/v4/__regression_unknown_path, should terminate when redirects are followed | 77 ms |

## Integration results

### Block queries

File name: `tests/integration/basic/queries/block-queries.test.ts`

#### Block Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a block query without parameters, should return the latest known block | 416 ms |
| ✓ | a block query without parameters, should respond with a block according to the requested schema | 211 ms |
| ✓ | a block query by hash, should return the block with that hash, given that block exists | 167 ms |
| ✓ | a block query by hash, should return blocks according to the requested schema | 166 ms |
| ✓ | a block query by hash, should return a null block, given a block with that hash doesn't exist | 91 ms |
| ✓ | a block query by hash, should return an error, when the hash is invalid (malformed) | 392 ms |
| ✓ | a block query by height, should return the block with that height, given a valid height | 167 ms |
| ✓ | a block query by height, should return a blocks according to the requested schema | 166 ms |
| ✓ | a block query by height, should return the genesis block, given height=0 is requested | 352 ms |
| ✓ | a block query by height, should return an empty body answer, given that block height requested is the maximum available height | 79 ms |
| ✓ | a block query by height, should return an error, given an invalid height | 237 ms |
| ✓ | a block query by height and hash, should return an error, as only one parameter at a time can be used | 349 ms |
| ✓ | block-level tree end indexes, should return non-negative integers for all three tree end index fields | 96 ms |
| ✓ | block-level tree end indexes, should return non-decreasing tree end indexes from parent to child block | 169 ms |
| ↓ | per-block dust merkle tree roots, should return different dust generation roots for blocks with different tree sizes | — |
| ↓ | per-block dust merkle tree roots, should return identical dust roots for repeated queries of the same block | — |
| ↓ | per-block dust merkle tree roots, should return equal dust generation roots for parent and child with equal tree sizes | — |

#### Genesis Block

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a block query to the genesis block, should contain transactions with pre-fund wallet utxos | 197 ms |
| ✓ | a block query to the genesis block, should contain utxos related to exactly 4 pre-fund wallets | 138 ms |
| ✓ | a block query to the genesis block, should contain utxos with exactly 1 token type | 160 ms |
| ✓ | a block query to the genesis block, should contain utxos sorted by outputIndex in ascending order | 136 ms |
| ✓ | a block query to the genesis block, should contain valid index ranges on regular transactions | 127 ms |
| ✓ | a block query to the genesis block, should return block-level end indexes >= max RegularTransaction end indexes | 151 ms |

### Bridge pool queries

File name: `tests/integration/basic/queries/bridge-pool-queries.test.ts`

#### Bridge Pool Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | bridgePoolSummary, should return zero pool totals when no reserve or treasury events are indexed | — |
| ↓ | bridgePoolSummary, should expose lastEventBlockHeight as a positive integer where bridge events exist | — |
| ↓ | bridgePoolSummary, should return an empty snapshot when atBlock predates all bridge events | — |
| □ | bridgePoolSummary, should set reserveTotal to the cumulative sum of ReserveTransfer amounts | — |
| □ | bridgePoolSummary, should aggregate treasury inflows separately by INVALID and UNAPPROVED reason | — |
| □ | bridgePoolSummary, should accumulate more pool inflow at a later atBlock than an earlier one | — |
| ↓ | bridgePoolSummary, should count SubminimalFlushTransfer.count in subminimumTxCount | — |
| ↓ | bridgePoolSummary, should aggregate UnapprovedTransfer amounts under UNAPPROVED treasury reason | — |
| ↓ | bridgeReserveInflows, should return an empty list when no ReserveTransfer events are indexed | — |
| □ | bridgeReserveInflows, should return ReserveTransfer events within the specified block range | — |
| □ | bridgeReserveInflows, should return events with correct BridgeReserveTransfer field shape | — |
| □ | bridgeReserveInflows, should paginate ReserveTransfer events with offset and limit | — |
| ↓ | bridgeTreasuryInflows, should return an empty list when no treasury-redirected events are indexed | — |
| □ | bridgeTreasuryInflows, should return all treasury event types when no reason filter is given | — |
| □ | bridgeTreasuryInflows, should return only BridgeInvalidTransfer when reason=INVALID | — |
| □ | bridgeTreasuryInflows, should respect blockHeightFrom and blockHeightTo filters for treasury inflows | — |
| ↓ | bridgeTreasuryInflows, should return only BridgeUnapprovedTransfer when reason=UNAPPROVED | — |
| ↓ | bridgeTreasuryInflows, should return only BridgeSubminimalFlushTransfer when reason=SUBMINIMAL_FLUSH | — |

### Bridge queries

File name: `tests/integration/basic/queries/bridge-queries.test.ts`

#### Bridge Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | bridgeEvents, should return an empty list for a recipient address with no bridge activity | — |
| ↓ | bridgeEvents, should return events with the correct shape for BridgeUserTransfer | — |
| ↓ | bridgeEvents, should return only events for the specified recipient address | — |
| ↓ | bridgeEvents, should return events of the known variant types present on chain | — |
| ↓ | bridgeEvents, should paginate results with offset and limit | — |
| ↓ | bridgeEvents, should return only UserTransfer events when filtered by variant USER_TRANSFER | — |
| ↓ | bridgeEvents, should reject a malformed recipient with a GraphQL error | — |
| □ | claims via unshieldedTransactions, should surface a BridgeClaimTransaction (recipient, amount) via unshieldedTransactions for a claim recipient | — |
| ↓ | bridgeBalance, should return zero balance for an address with no bridge activity | — |
| ↓ | bridgeBalance, should return zero balance once an address has fully claimed | — |
| ↓ | bridgeBalance, should set deposited to the sum of UserTransfer amounts for the address | — |
| □ | bridgeBalance, should reflect a non-zero remaining-claimable balance after a partial claim | — |
| ↓ | bridgeDeposits, should return an empty list for an address with no deposits | — |
| ↓ | bridgeDeposits, should return only UserTransfer events by default (excludes UnapprovedTransfer) | — |
| ↓ | bridgeDeposits, should include UnapprovedTransfer events when includeUnapproved=true | — |

### Contract event queries

File name: `tests/integration/basic/queries/contract-event-queries.test.ts`

#### Contract Event Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | the contract event GraphQL schema, should expose ContractEvent as an interface with the common fields | — |
| ↓ | the contract event GraphQL schema, should expose all eleven concrete contract event types | — |
| ↓ | the contract event GraphQL schema, should expose the ContractEventType enum with eleven variants | — |
| ↓ | the contract event GraphQL schema, should expose the ContractEventFilter input fields | — |
| ↓ | the contract event GraphQL schema, should expose the FieldPrefixFilter input fields | — |
| ↓ | a contract events query for an address with no emitted events, should return an empty list | — |
| ↓ | a contract events query for an address with no emitted events, should return an empty list when filtered by event types | — |
| ↓ | a contract events query for an address with no emitted events, should return an empty list when filtered by a block range | — |
| ↓ | a contract events query for an address with no emitted events, should return an empty list when filtered by valid field prefixes | — |
| ↓ | a contract events query with an invalid filter, should return an error when the contract address is empty | — |
| ↓ | a contract events query with an invalid filter, should return an error for malformed contract addresses | — |
| ↓ | a contract events query with an invalid filter, should return an error for an unknown field prefix field name | — |
| ↓ | a contract events query with an invalid filter, should return an error when a field prefix is not hex | — |
| ↓ | a contract events query for a contract with emitted events, should return events conforming to the contract event schema | — |
| ↓ | a contract events query for a contract with emitted events, should return only events of the requested type when filtered | — |
| ↓ | a contract events query for a contract with emitted events, should filter events by a prefix of an indexed field value | — |
| ↓ | a contract events query for a contract with emitted events, should combine multiple field prefixes with AND semantics | — |
| ↓ | a contract events query for a contract with emitted events, should return only events carrying the field for an empty prefix | — |
| ↓ | a contract events query for a contract with emitted events, should reach every event through each of its indexed fields | — |

### Contract queries

File name: `tests/integration/basic/queries/contract-queries.test.ts`

#### Contract Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a contract query by address, should return success as they are valid contract addresses | 598 ms |
| ✓ | a contract query by address, should return the most recent action for a contract with multiple actions | 588 ms |
| ✓ | a contract query by address, should return null when contract with that address does not exist | 75 ms |
| ✓ | a contract query by address, should return an error for malformed addresses | 1224 ms |
| ✗ | a contract query by address, should return non-empty unshieldedBalances for contracts known to hold tokens | 192 ms |
| ✓ | a contract query by address and offset, should return the correct action using exact block hash where it was included | 96 ms |
| ✓ | a contract query by address and offset, should return the latest state using a future block hash | 88 ms |
| ✓ | a contract query by address and offset, should respond with a contract action according to the expected schema | 78 ms |
| ✓ | a contract query by address and offset, should return the correct action using exact block height where it was included | 89 ms |
| ✓ | a contract query by address and offset, should return the latest state using a future block height | 106 ms |
| ✓ | a contract query by address and offset, should return the most recent contract action for that address before the specified block | 198 ms |
| ✓ | a contract query by address and offset, should return null when contract with valid address and valid offset does not exist | 72 ms |
| ✓ | a contract query by address and offset, should return null when contract with valid address and non-existing hash does not exist | 68 ms |
| ✓ | a contract query by address and offset, should return error when contract with invalid address and valid hash | 81 ms |
| ✓ | a contract query by address and offset, should return error when contract with invalid address and non-existing hash | 89 ms |
| ✓ | a contract query by address and offset, should return error when contract with valid address and invalid hash | 78 ms |
| ✓ | a contract query by address and offset, should return error when contract with invalid address and invalid hash | 413 ms |
| ✓ | a contract query by address and offset, should return null when contract with valid address and valid height does not exist | 79 ms |
| ✓ | a contract query by address and offset, should return null when contract with valid address and non-existing height does not exist | 77 ms |
| ✓ | a contract query by address and offset, should return error when contract with invalid address and valid height | 93 ms |
| ✓ | a contract query by address and offset, should return error when contract with invalid address and invalid height | 237 ms |
| ✓ | a contract query by address and offset, should return error for negative height | 72 ms |
| ✓ | a contract query by address and offset, should return error for non-integer height | 76 ms |
| ✓ | a contract query by address and offset, should return error for extremely large height | 79 ms |
| ✓ | a contract query by address and offset, should return null when using a block hash from before the action existed | 725 ms |
| ↓ | the contractEvents field on contract actions, should expose contractEvents on ContractCall | — |
| ✓ | the contractEvents field on contract actions, should not expose contractEvents on ContractDeploy or ContractUpdate | 329 ms |

### Contract type queries

File name: `tests/integration/basic/queries/contract-type-queries.test.ts`

#### Contract Type Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | a contract query by address, should return the contract with state, maintenance authority and actions | — |
| ↓ | a contract query by address, should return null for a non-existent contract | — |
| ↓ | a contract query by address, should error for malformed contract addresses | — |
| ↓ | the maintenance authority, should expose a committee of valid verifying keys with threshold and counter | — |
| □ | the maintenance authority, should map an ECDSA committee verifying key | — |
| ↓ | the actions sub-query, should bound the actions list by limit | — |
| ↓ | the actions sub-query, should filter actions by type DEPLOY | — |
| ↓ | the actions sub-query, should filter actions by type CALL | — |
| □ | the actions sub-query, should filter actions by type UPDATE | — |
| ↓ | point-in-time state via offset, should return state as of a pinned block hash | — |
| ↓ | point-in-time state via offset, should return state as of a pinned block height | — |
| ↓ | point-in-time state via offset, should return null when queried at a block before the contract existed | — |

### Contract zswap state queries

File name: `tests/integration/basic/queries/contract-zswap-state-queries.test.ts`

#### Contract Zswap State Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | block.contractZswapState, should return the contract-filtered zswap state for a known contract | — |
| ↓ | block.contractZswapState, should return null for a non-existent contract | — |
| ↓ | block.contractZswapState, should return identical state for repeated queries pinned to the same block | — |
| ↓ | block.contractZswapState, should error for malformed contract addresses | — |
| ↓ | block.contractZswapState, should error for a malformed block-hash offset | — |
| ↓ | block.contractZswapState, should return the zswap state when the block is pinned by height | — |
| ↓ | the composed execution-inputs query, should resolve zswap state, ledger parameters and contract state anchored to one block | — |
| ↓ | the composed execution-inputs query, should return null zswap state and null contract for a non-existent contract | — |

### Dust commitment update queries

File name: `tests/integration/basic/queries/dust-commitment-update-queries.test.ts`

#### Dust Commitment Merkle Tree Update Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a collapsed update query with valid index range, should return a collapsed update for a valid index range | 367 ms |
| ✓ | a collapsed update query with valid index range, should respond with a collapsed update according to the expected schema | 81 ms |
| ✓ | a collapsed update query with valid index range, should return a collapsed update for the full genesis dust range | 1059 ms |
| ✓ | a collapsed update query with equal start and end indices, should return a valid update when startIndex equals endIndex | 102 ms |
| ✓ | a collapsed update query idempotency, should return identical results for the same query parameters | 135 ms |
| ✓ | a collapsed update query with invalid index range, should return an error when startIndex is greater than endIndex | 74 ms |
| ✓ | a collapsed update query with invalid index range, should return an error when indices are negative | 69 ms |
| ✓ | a collapsed update query with invalid index range, should return an error when endIndex is beyond the indexed range | 85 ms |

### Dust generation update queries

File name: `tests/integration/basic/queries/dust-generation-update-queries.test.ts`

#### Dust Generation Merkle Tree Update Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a collapsed update query with valid index range, should return a collapsed update for a valid index range | 361 ms |
| ✓ | a collapsed update query with valid index range, should respond with a collapsed update according to the expected schema | 76 ms |
| ✓ | a collapsed update query with valid index range, should return a collapsed update for the full genesis dust range | 1184 ms |
| ✓ | a collapsed update query with equal start and end indices, should return a valid update when startIndex equals endIndex | 66 ms |
| ✓ | a collapsed update query idempotency, should return identical results for the same query parameters | 130 ms |
| ✓ | a collapsed update query with invalid index range, should return an error when startIndex is greater than endIndex | 71 ms |
| ✓ | a collapsed update query with invalid index range, should return an error when indices are negative | 82 ms |
| ✓ | a collapsed update query with invalid index range, should return an error when endIndex is beyond the indexed range | 2969 ms |

### Dust generations queries

File name: `tests/integration/basic/queries/dust-generations-queries.test.ts`

#### Dust Generations Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a dust generations query with a valid Cardano reward address, should return dust generations for a registered address | 321 ms |
| ✓ | a dust generations query with a valid Cardano reward address, should respond with dust generations according to the expected schema | 75 ms |
| ✓ | a dust generations query with a valid Cardano reward address, should return registrations with valid fields for a registered address | 81 ms |
| ✓ | a dust generations query with multiple addresses, should return dust generations for multiple addresses | 83 ms |
| ✓ | a dust generations query with non-registered address, should return empty registrations for a non-registered address | 81 ms |
| ✓ | a dust generations query with invalid input, should return an empty result for an empty addresses array | 62 ms |
| ✓ | a dust generations query with invalid input, should return an error for a malformed address | 317 ms |
| ✓ | a dustGenerations query with a multi-UTXO stake key (#926), should aggregate nightBalance across all backing cNIGHT UTXOs | 152 ms |
| ✓ | backwards compatibility with dustGenerationStatus, should return consistent data from both endpoints for a registered address | 82 ms |

### Dust queries

File name: `tests/integration/basic/queries/dust-queries.test.ts`

#### Dust Generation Status Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a dust generation status query with a valid Cardano reward address, should respond with a dust generation status response according to the requested schema | 309 ms |
| ✓ | a dust generation status query with a valid Cardano reward address, should report registered status for a registered Cardano reward address | 79 ms |
| ✓ | a dust generation status query with a valid Cardano reward address, should give the DUST destination address for the expected network when Cardano reward address is registered | 139 ms |
| ✓ | a dust generation status query with a valid Cardano reward address, should correctly indicate registration status for a non-registered key | 83 ms |
| ✓ | a dust generation status query with a valid Cardano reward address, should indicate zero generation for registered address without cNIGHT balance | 78 ms |
| ✓ | a dust generation status query with a valid Cardano reward address, should report the correct value of max capacity for registered address with cNIGHT | 83 ms |
| ✓ | a dust generation status query with a valid Cardano reward address, should correctly indicate deregistered status for a previously registered Cardano reward address | 76 ms |
| ✓ | a dust generation status query with multiple valid Cardano reward addresses, should return statuses for multiple Cardano reward addresses in order | 127 ms |
| ✓ | a dust generation status query with multiple valid Cardano reward addresses, should respond with dust generation statuses according to the requested schema for multiple addresses | 90 ms |
| ✓ | a dust generation status query with multiple valid Cardano reward addresses, should return an error given the number of addresses is greater than 10 | 75 ms |
| ✓ | a dust generation status query with malformed reward addresses, should return an error when the address is in plain hex string format | 60 ms |
| ✓ | a dust generation status query with empty list of reward addresses, should return an empty list of dust generation statuses | 61 ms |
| ✓ | a dust generation status query with a Cardano payment address, should return an error as only Cardano reward addresses are supported | 76 ms |
| ✓ | a dust generation status query with a Cardano reward address not meant for this network, should return an error reporting the target network mismatch | 75 ms |
| ✓ | a dust generation status query with duplicate reward addresses, should handle duplicate Cardano reward addresses appropriately | 81 ms |

### Graphql http compression

File name: `tests/integration/basic/queries/graphql-http-compression.test.ts`

#### Graphql Http Response Compression

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a GraphQL HTTP request with Accept-Encoding: gzip, should return a gzip-compressed response | 325 ms |
| ✓ | a GraphQL HTTP request with Accept-Encoding: br, should return a brotli-compressed response | 79 ms |
| ✓ | a GraphQL HTTP request with Accept-Encoding: zstd, should return a zstd-compressed response | 81 ms |
| ✓ | a GraphQL HTTP request with Accept-Encoding: identity, should return an uncompressed response | 257 ms |

### Spo epoch series queries

File name: `tests/integration/basic/queries/spo-epoch-series-queries.test.ts`

#### SPO Epoch Series Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a registered totals series query with an epoch span within the maximum, should accept a range covering a single epoch | 309 ms |
| ✓ | a registered totals series query with an epoch span within the maximum, should accept a range whose span equals the maximum | 80 ms |
| ✓ | a registered totals series query with an epoch span within the maximum, should accept a permitted span that does not start at epoch zero | 79 ms |
| ✓ | a registered totals series query with an epoch span exceeding the maximum, should reject a range one epoch beyond the maximum span | 66 ms |
| ✓ | a registered totals series query with an epoch span exceeding the maximum, should reject a reversed range whose span exceeds the maximum | 71 ms |
| ✓ | a registered totals series query with an epoch span exceeding the maximum, should reject a range far beyond the maximum span without returning data | 65 ms |
| ✓ | a registered SPO series query with an epoch span exceeding the maximum, should reject a range beyond the maximum span | 71 ms |
| ✓ | a registered SPO series query with an epoch span exceeding the maximum, should accept a range whose span equals the maximum | 242 ms |
| ✓ | a registered presence query with an epoch span exceeding the maximum, should reject a range beyond the maximum span | 63 ms |
| ✓ | a registered presence query with an epoch span exceeding the maximum, should accept a range whose span equals the maximum | 82 ms |

### Transaction queries

File name: `tests/integration/basic/queries/transaction-queries.test.ts`

#### Transaction Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a transaction query by hash, should return the transaction with that hash, given that transaction exists | 3488 ms |
| ✓ | a transaction query by hash, should return an empty transaction list, given a transaction with that hash doesn't exist | 74 ms |
| ✓ | a transaction query by hash, should return an error, given a hash is invalid (malformed) | 591 ms |
| ✓ | a transaction query by hash, should return an error when called with an empty offset object | 64 ms |
| ✓ | a transaction query by identifier, should return the transaction with that identifier, given that transaction exists | 3220 ms |
| ✓ | a transaction query by identifier, should return an empty list of transactions, given a transaction with that identifier doesn't exist | 94 ms |
| ✓ | a transaction query by identifier, should return an error, given an invalid identifier | 228 ms |
| ✓ | a transaction query by hash and identifier, should return an error, as only one parameter at a time can be used | 80 ms |

#### Genesis Transactions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | transaction queries to the genesis block transactions, should return utxos related to 4 pre-fund wallets | 2512 ms |
| ✓ | transaction queries to the genesis block transactions, should return utxos with 1 token type | 2370 ms |
| ✓ | transaction queries to the genesis block transactions, should return utxos sorted by outputIndex in ascending order | 2406 ms |
| ✓ | transaction queries to the genesis block transactions, should return system transactions when queried by hash | 2765 ms |
| ✓ | transaction queries to the genesis block transactions, should not contain RegularTransaction-specific fields on system transactions | 2339 ms |
| ✓ | schema validation, should respond with full transaction data according to the expected schema | 7 ms |
| ✓ | schema validation, should respond with system transactions according to the expected schema | 0 ms |
| ✓ | schema validation, should contain both regular and system transactions | 0 ms |
| ✓ | schema validation, should respond with regular transactions according to the expected schema | 2 ms |
| ✓ | schema validation, should respond with nested ledger events and unshielded outputs according to the expected schema | 1 ms |

### Zswap collapsed update queries

File name: `tests/integration/basic/queries/zswap-collapsed-update-queries.test.ts`

#### Zswap Merkle Tree Collapsed Update Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a collapsed update query with valid index range, should return a collapsed update for a valid index range | 319 ms |
| ✓ | a collapsed update query with valid index range, should respond with a collapsed update according to the expected schema | 77 ms |
| ✓ | a collapsed update query with valid index range, should return a collapsed update for the full genesis zswap range | 871 ms |
| ✓ | a collapsed update query with equal start and end indices, should return a valid update when startIndex equals endIndex | 83 ms |
| ✓ | a collapsed update query idempotency, should return identical results for the same query parameters | 168 ms |
| ✓ | a collapsed update query with invalid index range, should return an error when startIndex is greater than endIndex | 80 ms |
| ✓ | a collapsed update query with invalid index range, should return an error when indices are negative | 84 ms |
| ✓ | a collapsed update query with invalid index range, should return an error when endIndex is beyond the indexed range | 76 ms |

### Block subscriptions

File name: `tests/integration/basic/subscriptions/block-subscriptions.test.ts`

#### Block Subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a subscription to block updates without parameters, should stream blocks starting from the latest block | 6705 ms |
| ✓ | a subscription to block updates without parameters, should stream blocks adhering to the expected schema | 968 ms |
| ✓ | a subscription to block updates by hash, should stream blocks starting from the block with that hash, given that hash exists | 1626 ms |
| ✓ | a subscription to block updates by hash, should return an error message, given that hash doesn't exist | 707 ms |
| ✓ | a subscription to block updates by hash, should return an error message, given that hash is invalid | 704 ms |
| ✓ | a subscription to block updates by height, should stream blocks from the block with that height, given that height exists | 1052 ms |
| ✓ | a subscription to block updates by height, should return an error message, given that height is higher than the latest block height | 931 ms |
| ✓ | a subscription to block updates by height, should return an error message, given that height is invalid | 691 ms |
| ✓ | a subscription to block updates by height and hash, should return an error message, as only one parameter at a time can be used | 684 ms |
| ↓ | regular transaction fees, should report ledger paidFees and estimatedFees on regular transactions | — |

### Bridge pool subscriptions

File name: `tests/integration/basic/subscriptions/bridge-pool-subscriptions.test.ts`

#### Bridge Pool Subscription

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | bridgePoolUpdates, should emit current pool summary immediately on subscribe | — |
| □ | bridgePoolUpdates, should push an update when a new ReserveTransfer is indexed | — |
| □ | bridgePoolUpdates, should push an update when a new InvalidTransfer is indexed | — |
| □ | bridgePoolUpdates, should not push a pool update for UserTransfer events (user address, not pool) | — |
| □ | bridgePoolUpdates, should carry a running pool total in each frame (cumulative consistency) | — |
| ↓ | bridgePoolUpdates, should push an update when an UnapprovedTransfer is indexed | — |
| ↓ | bridgePoolUpdates, should push an update and increment subminimumTxCount when a flush is indexed | — |

### Bridge subscriptions

File name: `tests/integration/basic/subscriptions/bridge-subscriptions.test.ts`

#### Bridge Subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | bridgeEvents, should replay historical events from the given cursor id | — |
| ↓ | bridgeEvents, should only deliver events matching the recipient filter | — |
| ↓ | bridgeEvents, should only deliver events matching the variant filter | — |
| ↓ | bridgeEvents, should resume from cursor without gap or duplication on reconnection | — |
| ↓ | bridgeEvents, should deliver BridgeUnapprovedTransfer events via subscription | — |
| □ | claims via unshieldedTransactions, should observe claims as BridgeClaimTransaction via the unshieldedTransactions query | — |
| ↓ | bridgeBalance, should emit zero balance immediately for an address with no bridge activity | — |
| □ | bridgeBalance, should push an updated BridgeBalance when a relevant UserTransfer is indexed | — |
| □ | bridgeBalance, should push an updated BridgeBalance when a claim reduces the balance | — |

### Contract event subscriptions

File name: `tests/integration/basic/subscriptions/contract-event-subscriptions.test.ts`

#### Contract Event Subscription

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | a contract events subscription for an address with no emitted events, should complete via the toBlock terminator without delivering events | — |
| ↓ | a contract events subscription with an invalid filter, should return an error when the contract address is empty | — |
| ↓ | a contract events subscription with an invalid filter, should return an error for an unknown field prefix field name | — |
| ↓ | a contract events subscription for a contract with emitted events, should stream events conforming to the contract event schema | — |
| ↓ | a contract events subscription for a contract with emitted events, should stream only events carrying the filtered field | — |

### Contract subscriptions

File name: `tests/integration/basic/subscriptions/contract-subscriptions.test.ts`

#### Contract Action Subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a subscription to contract action updates without parameters, should stream contract actions from the latest available block | 824 ms |
| ✓ | a subscription to contract action updates without parameters, should stream contract actions adhering to the expected schema | 776 ms |
| ✓ | a subscription to contract action updates with block hash offset, should stream historical and new contract actions from a specific block hash | 871 ms |

### Dust generations subscriptions

File name: `tests/integration/basic/subscriptions/dust-generations-subscriptions.test.ts`

#### Dust Generations Subscription

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | a subscription at the latest block hash, should stream a complete generation snapshot for a registered dust address | — |
| ↓ | a subscription pinned to a block hash, should deliver identical events for repeated subscriptions at the same block | — |
| ↓ | a subscription pinned to a block hash, should snapshot the generation tree at the queried block rather than the tip | — |
| ↓ | dtime update delivery relative to the cutoff height, should replay owned dtime updates before generation items when the cutoff is zero | — |
| ↓ | dtime update delivery relative to the cutoff height, should deliver no dtime updates when the cutoff equals the snapshot block height | — |
| ↓ | subscription error handling, should return an error for an invalid dust address | — |
| ↓ | subscription error handling, should return an error for a valid address that is meant for another networkid | — |
| ↓ | subscription error handling, should return an error for a valid dust address passed in hex format | — |
| ↓ | subscription error handling, should return an error for an unknown block hash | — |
| ↓ | subscription error handling, should return an error for a malformed block hash | — |
| ↓ | subscription error handling, should reject a block hash older than the snapshot freshness window | — |
| ↓ | transactionHash on dust generation events, first item transactionHash resolves via transactions(offset) | — |

#### Dust Generations Progress Protocol Version

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | a subscription pinned to a block hash, should report the pinned block protocol version in the progress event | — |

### Dust ledger subscriptions

File name: `tests/integration/basic/subscriptions/dust-ledger-subscriptions.test.ts`

#### Dust Ledger Event Subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a subscription to dust ledger events without offset (default replay), streams events in ledger order | 1005 ms |
| ✓ | subscription with explicit offset, streams events in ledger order starting from the specified ID | 1050 ms |
| ✓ | subscription with explicit offset, validates historical dust events against schema | 980 ms |
| ✓ | subscription error handling, should return an error for unknown field | 690 ms |
| ✓ | subscription error handling, rejects negative offset ID with an error | 732 ms |

### Dust nullifier subscriptions

File name: `tests/integration/basic/subscriptions/dust-nullifier-subscriptions.test.ts`

#### Dust Nullifier Transactions Subscription

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | streaming dust nullifier transactions with block range, should stream transactions within a block range and complete | 980 ms |
| ✓ | subscription error handling, should return an error for empty nullifier prefixes | 682 ms |
| ✓ | subscription error handling, should return an error when fromBlock is greater than toBlock | 693 ms |
| ✓ | transactionHash on dust nullifier events (#1114), first event transactionHash resolves via transactions(offset) | 1118 ms |
| ✓ | transaction reference on dust nullifier events (#1115), first event transaction.hash matches transactionHash | 1006 ms |

### Shielded nullifier subscriptions

File name: `tests/integration/basic/subscriptions/shielded-nullifier-subscriptions.test.ts`

#### Shielded Nullifier Transactions Subscription

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | streaming shielded nullifier transactions with block range, should stream transactions within a block range and complete | 996 ms |
| ✓ | subscription error handling, should return an error for empty nullifier prefixes | 692 ms |
| ✓ | subscription error handling, should return an error for an empty-string nullifier prefix element | 706 ms |
| ✓ | subscription error handling, should return an error when fromBlock is greater than toBlock | 711 ms |
| ✓ | transactionHash on shielded nullifier events (#1114), first event transactionHash resolves via transactions(offset) | 1073 ms |
| ✓ | transaction reference on shielded nullifier events (#1115), first event transaction.hash matches transactionHash | 935 ms |

### Shielded transaction subscriptions

File name: `tests/integration/basic/subscriptions/shielded-transaction-subscriptions.test.ts`

#### Shielded Transaction Subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | opening a session with viewing key, should return a session ID, given a valid viewing key | 794 ms |
| ✓ | opening a session with viewing key, should return an error, given an unsupported hex format viewing key | 699 ms |
| ✓ | opening a session with viewing key, should return an error, given an invalid viewing key | 675 ms |
| ✓ | opening a session with viewing key, should return an error, given a valid viewing key meant for a different network | 1361 ms |
| ✓ | closing a session with session ID, should terminate the session successfully, given a valid session ID | 820 ms |
| ✓ | closing a session with session ID, should return an error, given an invalid session ID | 675 ms |
| ✓ | a subscription to wallet updates providing viewing key only, should stream wallet events starting from the beginning, given there are relevant transactions | 2784 ms |
| ✓ | a subscription to wallet updates providing viewing key only, should stream shielded transaction events adhering to the expected schema | 3746 ms |
| ✓ | a subscription to wallet updates providing viewing key only, should be able to use highestZswapEndIndex from progress event in collapsed update query | 1229 ms |
| ↓ | a subscription to wallet updates providing viewing key only, should report the chain tip protocol version in the progress event | — |
| ✓ | a subscription to wallet updates providing viewing key only, should reject shieldedTransactions subscription when using expired session ID | 991 ms |

### Subscription quotas

File name: `tests/integration/basic/subscriptions/subscription-quotas.test.ts`

#### Subscription Quotas

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | per-connection concurrent subscription cap, should reject a subscription beyond the per-connection cap | 1725 ms |
| ✓ | per-connection concurrent subscription cap, should free a slot when an active subscription is closed | 2712 ms |
| ✓ | per-session subscription creation rate limit, should reject shielded subscription creations beyond the per-session rate limit | 1852 ms |
| ✓ | per-session subscription creation rate limit, should allow a new shielded subscription after the rate limit refills | 10009 ms |
| ✓ | per-session subscription creation rate limit, should keep a live shielded subscription streaming while creations are rate-limited | 34917 ms |

### Unshielded transaction subscriptions

File name: `tests/integration/basic/subscriptions/unshielded-transaction-subscriptions.test.ts`

#### Unshielded Transaction Subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a subscription to unshielded transaction events by address, should stream unshielded transaction events related to that address, given that address has transactions | 209 ms |
| ✓ | a subscription to unshielded transaction events by address, should stream unshielded transaction events up to highest transaction id | 7350 ms |
| ✓ | a subscription to unshielded transaction events by address, should stream unshielded transaction events that adhere to the expected schema | 151 ms |
| ✓ | a subscription to unshielded transaction events by address, should only return a transaction progress message with highest transaction = 0, given that address does not have transactions | 5055 ms |
| ↓ | a subscription to unshielded transaction events by address, should report the chain tip protocol version in the progress update, given that address does not have transactions | — |
| ✓ | a subscription to unshielded transaction events by address, should return an error message, given the address provided is in hex format | 108 ms |
| ✓ | a subscription to unshielded transaction events by address, should return an error message, given the address provided is for another network | 743 ms |
| ✓ | a subscription to unshielded transaction events by address and transaction id, should return a stream of transactions containing that address, starting from transaction id = 0 | 5540 ms |
| ✓ | a subscription to unshielded transaction events by address and transaction id, should return an error message, given the transaction id provided is negative | 125 ms |
| ✓ | a subscription to unshielded transaction events by address and transaction id, should only return a transaction progress message without streaming transactions, given that the transaction id provided is bigger number | 5060 ms |
| ✓ | a subscription to unshielded transaction events by address and transaction id, should start a transaction stream from the given transaction id | 5292 ms |
| ✓ | a subscription to unshielded transaction events by address and transaction id, should return an error message, given the address is provided in hex format | 82 ms |

### Wallet connect options

File name: `tests/integration/basic/subscriptions/wallet-connect-options.test.ts`

#### Wallet Connect Options (startIndex)

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | opening a session with startIndex, should accept startIndex = 0 (no-op equivalent of unset options) | 784 ms |
| ✓ | opening a session with startIndex, should skip historical scan when startIndex equals the current tip | 1665 ms |
| ✓ | opening a session with startIndex, should accept startIndex past the current tip (fast-forward) | 1627 ms |

### Websocket keepalive

File name: `tests/integration/basic/subscriptions/websocket-keepalive.test.ts`

#### Websocket Keepalive

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | an idle socket without the client keepalive, should be closed while idle past the idle limit | 152611 ms |
| ✓ | an idle socket with the client keepalive, should stay open while idle past the idle limit | 150644 ms |

### Zswap events subscriptions

File name: `tests/integration/basic/subscriptions/zswap-events-subscriptions.test.ts`

#### Zswap Ledger Event Subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a subscription to zswap ledger events without offset (default replay), streams events in ledger order | 914 ms |
| ✓ | subscription with explicit offset, streams events starting from the specified ID | 2363 ms |
| ✓ | subscription with explicit offset, validates historical zswap events against schema | 1124 ms |
| ✓ | subscription error handling, should return an error for unknown field | 765 ms |
| ✓ | subscription error handling, rejects negative offset ID with an error | 701 ms |

## E2E results

### Contract actions partial success

File name: `tests/e2e/contract-actions-partial-success.test.ts`

#### Contract Actions In A Guaranteed/fallible Segment Split

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | a Call whose guaranteed transcript applied but whose fallible segment failed, the fixture produces a call with a guaranteed transcript | — |
| ↓ | a Call whose guaranteed transcript applied but whose fallible segment failed, the fixture produces a partially successful transaction | — |
| ↓ | a Call whose guaranteed transcript applied but whose fallible segment failed, is still reported by the indexer | — |
| ↓ | a Call where neither execution phase applied, the fixture produces a call without a guaranteed transcript | — |
| ↓ | a Call where neither execution phase applied, the fixture still produces a partially successful transaction | — |
| ↓ | a Call where neither execution phase applied, is not reported by the indexer | — |

### Contract actions sequential

File name: `tests/e2e/contract-actions-sequential.test.ts`

#### Contract Actions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a transaction to deploy a counter smart contract, should be reported by the indexer through a transaction query by hash | 108 ms |
| ✓ | a transaction to deploy a counter smart contract, should be reported by the indexer through a block query by hash | 164 ms |
| ✓ | a transaction to deploy a counter smart contract, should be reported by the indexer through a contract action query by address | 79 ms |
| ✓ | a transaction to call the store circuit on the deployed counter smart contract, should be reported by the indexer through a transaction query by hash | 86 ms |
| ✓ | a transaction to call the store circuit on the deployed counter smart contract, should be reported by the indexer through a block query by hash | 120 ms |
| ✓ | a transaction to call the store circuit on the deployed counter smart contract, should be reported by the indexer through a contract action query by address | 100 ms |
| ✓ | a transaction to update the counter smart contract, should be reported by the indexer through a transaction query by hash | 88 ms |
| ✓ | a transaction to update the counter smart contract, should be reported by the indexer through a block query by hash | 216 ms |
| ✓ | a transaction to update the counter smart contract, should be reported by the indexer through a contract action query by address | 196 ms |

### Custom unshielded token transactions

File name: `tests/e2e/custom-unshielded-token-transactions.test.ts`

#### Unshielded Custom Token Transactions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a successful unshielded transaction transferring 1 token between two addresses, should be reported by the indexer through a block query by hash | 121 ms |
| ✓ | a successful unshielded transaction transferring 1 token between two addresses, should be reported by the indexer through a transaction query by hash | 103 ms |
| ✓ | a successful unshielded transaction transferring 1 token between two addresses, should be reported by the indexer through an unshielded transaction event for the source address | 1 ms |
| ✓ | a successful unshielded transaction transferring 1 token between two addresses, should be reported by the indexer through an unshielded transaction event for the destination address | 2 ms |
| ✓ | a successful unshielded transaction transferring 1 token between two addresses, should have transferred 1 token from the source to the destination address | 109 ms |
| ✓ | a successful unshielded transaction transferring 1 token between two addresses, should report the token type under test on every created and spent output | 158 ms |
| ✓ | a successful unshielded transaction transferring 1 token between two addresses, should be reported by the indexer through a progress update event for the source address | 45031 ms |
| ✓ | a successful unshielded transaction transferring 1 token between two addresses, should be reported by the indexer through a progress update event for the destination address | 2 ms |

### Night transactions

File name: `tests/e2e/night-transactions.test.ts`

#### Unshielded NIGHT Transactions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses, should be reported by the indexer through a block query by hash | 113 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses, should be reported by the indexer through a transaction query by hash | 88 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses, should be reported by the indexer through an unshielded transaction event for the source address | 2 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses, should be reported by the indexer through an unshielded transaction event for the destination address | 2 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses, should have transferred 1 STAR from the source to the destination address | 100 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses, should report the token type under test on every created and spent output | 96 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses, should be reported by the indexer through a progress update event for the source address | 30017 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses, should be reported by the indexer through a progress update event for the destination address | 1 ms |
| ✓ | the genesis block of a chain minted with NIGHT alone, should report the all-zeros NIGHT token type on its outputs | 1 ms |
| ✓ | the dust activity of a confirmed NIGHT transaction, should increase the dust commitment Merkle tree end index | 335 ms |
| ✓ | a confirmed unshielded transfer streamed to address subscriptions, should emit UnshieldedTransaction only for the target wallet (A > B1) | 36793 ms |
| ✓ | a confirmed unshielded transfer streamed to address subscriptions, should emit UnshieldedTransaction only for the target wallet (A > B2) | 16875 ms |
| ✓ | an address with no transaction history, should emit only ProgressUpdate for empty wallet | 2689 ms |

### Segment probe contract actions

File name: `tests/e2e/segment-probe-contract-actions.test.ts`

#### Segment-split Contract Actions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a transaction to deploy a segment-split smart contract, should be reported by the indexer through a transaction query by hash | 100 ms |
| ✓ | a transaction to deploy a segment-split smart contract, should be reported by the indexer through a block query by hash | 106 ms |
| ✓ | a transaction to deploy a segment-split smart contract, should be reported by the indexer through a contract action query by address | 115 ms |
| ✓ | a transaction to call the burnWithGuaranteed circuit on the deployed segment-split smart contract, should be reported by the indexer through a transaction query by hash | 103 ms |
| ✓ | a transaction to call the burnWithGuaranteed circuit on the deployed segment-split smart contract, should be reported by the indexer through a block query by hash | 512 ms |
| ✓ | a transaction to call the burnWithGuaranteed circuit on the deployed segment-split smart contract, should be reported by the indexer through a contract action query by address | 92 ms |

### Shielded transactions

File name: `tests/e2e/shielded-transactions.test.ts`

#### Shielded Transactions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a successful shielded transaction transferring 1 Shielded Token between two wallets, should be reported by the indexer through a block query by hash | 217 ms |
| ✓ | a successful shielded transaction transferring 1 Shielded Token between two wallets, should be reported by the indexer through a transaction query by hash | 148 ms |
| ✓ | a successful shielded transaction transferring 1 Shielded Token between two wallets, should stream Zswap events followed by DustSpendProcessed after a shielded transaction | 1736 ms |
| ✓ | a successful shielded transaction transferring 1 Shielded Token between two wallets, should increase the zswap Merkle tree end index | 415 ms |
| ✓ | a successful shielded transaction transferring 1 Shielded Token between two wallets, should increase the dust commitment Merkle tree end index | 122 ms |
| ✓ | a confirmed shielded transfer streamed to wallet sessions by viewing key, should stream the transaction to the source viewing key with a matching hash | 1141 ms |
| ✓ | a confirmed shielded transfer streamed to wallet sessions by viewing key, should stream the transaction to the destination viewing key with a matching hash | 1142 ms |
| ✓ | a confirmed shielded transfer streamed to wallet sessions by viewing key, should not stream the transaction to an unrelated viewing key | 20879 ms |

### Subscription polling

File name: `tests/e2e/subscription-polling.test.ts`

#### Subscription Polling

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | an unshielded transactions subscription for a never-used address, should emit progress immediately and back off while the address is idle | — |
| ↓ | an unshielded transactions subscription for a never-used address, should reset the polling interval when the address receives a transaction | — |
| ↓ | the keep-alive heartbeat of a shielded transactions subscription, should refresh wallet activity once at subscription start and then throttle | — |
| □ | the keep-alive heartbeat of a shielded transactions subscription, should keep a subscribed wallet active beyond the wallet-indexer inactivity TTL | — |
| ↓ | a shielded transactions subscription on an idle wallet, should emit progress immediately and keep emitting while idle | — |

### Unshielded balance e2e

File name: `tests/e2e/unshielded-balance-e2e.test.ts`

#### Unshielded Balance Indexing (deploy + Mint + Verify) [#1253 Scaffold]

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | reports the contract remainder after mint-to-self then partial send | — |

### Toolkit: Dust balance

File name: `tests/e2e/toolkit/dust-balance.test.ts`

#### Dust Balance Query Using Moth

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a dust balance query with a valid wallet seed, should respond with a dust balance according to the requested schema | 34619 ms |

### Toolkit: Key material

File name: `tests/e2e/toolkit/key-material.test.ts`

#### Key Material Derivation Validation

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a midnight shielded address, should show with the expected prefix for the current network ID | 53 ms |
| ✓ | a midnight shielded address, should show with the expected prefix for all network IDs | 338 ms |
| ✓ | a midnight unshielded address, should show with the expected prefix for the current network ID | 55 ms |
| ✓ | a midnight unshielded address, should show with the expected prefix for all network IDs | 348 ms |
| ✓ | a midnight viewing key, should show with the expected prefix for the current network ID | 42 ms |
| ✓ | a midnight viewing key, should show with the expected prefix for all network IDs | 309 ms |

### Toolkit: Show wallet queries

File name: `tests/e2e/toolkit/show-wallet-queries.test.ts`

#### Show Wallet Queries Using Moth

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | private wallet state query using moth, should respond with a private wallet state according to the requested schema | 37100 ms |
| ✓ | public wallet state query using moth, should respond with a public wallet state according to the requested schema | 79 ms |
