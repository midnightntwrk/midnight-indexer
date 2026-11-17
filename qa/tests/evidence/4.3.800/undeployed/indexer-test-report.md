# Indexer Test Execution Report

## Run Details

| Field | Value |
| --- | --- |
| Component Under Test | Indexer / Node |
| Indexer Version | 4.3.800 |
| Node Version | 1.0.400 (toolkit 1.0.400) |
| Test Suites | smoke, integration, e2e |
| Tests | Indexer GraphQL API and integration with Node |
| Environment | undeployed (INDEXER_TAG=4.3.800) |
| Date | Tuesday 27 Oct 2026 |
| QA Contact | Oleksandr Romanov (oleksandr.romanov@shielded.io) |

## Summary

| Suite | Test Files | Passed | Failed | Skipped | Todo | Total | Duration | Run at |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- |
| smoke | 3 passed, 1 skipped (4) | 18 | 0 | 1 | 0 | 19 | 1.53s | 15:01 EET |
| integration | 17 passed, 5 skipped (22) | 148 | 0 | 49 | 0 | 197 | 265.59s | 15:09 EET |
| e2e | 8 passed, 3 skipped (11) | 49 | 0 | 15 | 1 | 65 | 714.92s | 15:23 EET |
| **Total** | | **215** | **0** | **65** | **1** | **281** | | |

> Legend: ✓ passed · ✗ failed · ↓ skipped · □ todo.
> Duration is the total test time reported in each suite's `junit-test-results.xml`.

## Smoke results

### Graphql healthchecks

File name: `tests/smoke/graphql-healthchecks.test.ts`

#### Graphql Health Checks

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | an introspection message sent to the websocket channel, should return the supported graphql schema | 573 ms |
| ✓ | an introspection message sent to the websocket channel, should return an error, given the depth of the query is > 15 | 513 ms |
| ✓ | an introspection request sent to the http channel, should return the supported graphql schema | 32 ms |
| ✓ | an introspection request sent to the http channel, should return an error, given the depth of the query is > 15 | 6 ms |

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
| ✓ | RegularTransaction.merkleTreeRoot, should be marked deprecated with reason "Use zswapMerkleTreeRoot instead" | 134 ms |
| ✓ | RegularTransaction.startIndex, should be marked deprecated with reason "Use zswapStartIndex instead" | 14 ms |
| ✓ | RegularTransaction.endIndex, should be marked deprecated with reason "Use zswapEndIndex instead" | 25 ms |
| ✓ | RelevantTransaction.collapsedMerkleTree, should be marked deprecated with reason "Use zswapCollapsedUpdate instead" | 13 ms |
| ✓ | ShieldedTransactionsProgress.highestEndIndex, should be marked deprecated with reason "Use highestZswapEndIndex instead" | 11 ms |
| ✓ | ShieldedTransactionsProgress.highestCheckedEndIndex, should be marked deprecated with reason "Use highestCheckedZswapEndIndex instead" | 8 ms |
| ✓ | ShieldedTransactionsProgress.highestRelevantEndIndex, should be marked deprecated with reason "Use highestRelevantZswapEndIndex instead" | 12 ms |
| ✓ | RegularTransaction.fees, should be marked deprecated with reason "Use fee instead" | 9 ms |
| ✓ | TransactionFees.estimatedFees, should be marked deprecated with reason "Use paidFees instead" | 13 ms |

### Service healthchecks

File name: `tests/smoke/service-healthchecks.test.ts`

#### Service Health Checks

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a request to the /ready endpoint, should return a 200 status code OK | 72 ms |
| ✓ | a request to an unrecognised path /api/v3/__regression_unknown_path, should not 308 to a version-double-prefixed Location | 67 ms |
| ✓ | a request to an unrecognised path /api/v3/__regression_unknown_path, should terminate when redirects are followed | 5 ms |
| ✓ | a request to an unrecognised path /api/v4/__regression_unknown_path, should not 308 to a version-double-prefixed Location | 4 ms |
| ✓ | a request to an unrecognised path /api/v4/__regression_unknown_path, should terminate when redirects are followed | 4 ms |

## Integration results

### Block queries

File name: `tests/integration/basic/queries/block-queries.test.ts`

#### Block Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a block query without parameters, should return the latest known block | 45 ms |
| ✓ | a block query without parameters, should respond with a block according to the requested schema | 17 ms |
| ✓ | a block query by hash, should return the block with that hash, given that block exists | 14 ms |
| ✓ | a block query by hash, should return blocks according to the requested schema | 12 ms |
| ✓ | a block query by hash, should return a null block, given a block with that hash doesn't exist | 4 ms |
| ✓ | a block query by hash, should return an error, when the hash is invalid (malformed) | 30 ms |
| ✓ | a block query by height, should return the block with that height, given a valid height | 11 ms |
| ✓ | a block query by height, should return a blocks according to the requested schema | 10 ms |
| ✓ | a block query by height, should return the genesis block, given height=0 is requested | 39 ms |
| ✓ | a block query by height, should return an empty body answer, given that block height requested is the maximum available height | 2 ms |
| ✓ | a block query by height, should return an error, given an invalid height | 5 ms |
| ✓ | a block query by height and hash, should return an error, as only one parameter at a time can be used | 6 ms |
| ✓ | block-level tree end indexes, should return non-negative integers for all three tree end index fields | 4 ms |
| ✓ | block-level tree end indexes, should return non-decreasing tree end indexes from parent to child block | 9 ms |

#### Genesis Block

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a block query to the genesis block, should contain transactions with pre-fund wallet utxos | 30 ms |
| ✓ | a block query to the genesis block, should contain utxos related to exactly 4 pre-fund wallets | 28 ms |
| ✓ | a block query to the genesis block, should contain utxos with exactly 1 token type | 28 ms |
| ✓ | a block query to the genesis block, should contain utxos sorted by outputIndex in ascending order | 35 ms |
| ✓ | a block query to the genesis block, should contain valid index ranges on regular transactions | 32 ms |
| ✓ | a block query to the genesis block, should return block-level end indexes >= max RegularTransaction end indexes | 30 ms |

### Contract queries

File name: `tests/integration/basic/queries/contract-queries.test.ts`

#### Contract Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a contract query by address, should return success as they are valid contract addresses | 45 ms |
| ✓ | a contract query by address, should return the most recent action for a contract with multiple actions | 12 ms |
| ✓ | a contract query by address, should return null when contract with that address does not exist | 3 ms |
| ✓ | a contract query by address, should return an error for malformed addresses | 26 ms |
| ↓ | a contract query by address, should return non-empty unshieldedBalances for contracts known to hold tokens | — |
| ✓ | a contract query by address and offset, should return the correct action using exact block hash where it was included | 7 ms |
| ✓ | a contract query by address and offset, should return the latest state using a future block hash | 10 ms |
| ✓ | a contract query by address and offset, should respond with a contract action according to the expected schema | 10 ms |
| ✓ | a contract query by address and offset, should return the correct action using exact block height where it was included | 13 ms |
| ✓ | a contract query by address and offset, should return the latest state using a future block height | 8 ms |
| ✓ | a contract query by address and offset, should return the most recent contract action for that address before the specified block | 12 ms |
| ✓ | a contract query by address and offset, should return null when contract with valid address and valid offset does not exist | 2 ms |
| ✓ | a contract query by address and offset, should return null when contract with valid address and non-existing hash does not exist | 2 ms |
| ✓ | a contract query by address and offset, should return error when contract with invalid address and valid hash | 1 ms |
| ✓ | a contract query by address and offset, should return error when contract with invalid address and non-existing hash | 1 ms |
| ✓ | a contract query by address and offset, should return error when contract with valid address and invalid hash | 1 ms |
| ✓ | a contract query by address and offset, should return error when contract with invalid address and invalid hash | 6 ms |
| ✓ | a contract query by address and offset, should return null when contract with valid address and valid height does not exist | 3 ms |
| ✓ | a contract query by address and offset, should return null when contract with valid address and non-existing height does not exist | 2 ms |
| ✓ | a contract query by address and offset, should return error when contract with invalid address and valid height | 1 ms |
| ✓ | a contract query by address and offset, should return error when contract with invalid address and invalid height | 4 ms |
| ✓ | a contract query by address and offset, should return error for negative height | 1 ms |
| ✓ | a contract query by address and offset, should return error for non-integer height | 1 ms |
| ✓ | a contract query by address and offset, should return error for extremely large height | 1 ms |
| ✓ | a contract query by address and offset, should return null when using a block hash from before the action existed | 31 ms |

### Dust commitment update queries

File name: `tests/integration/basic/queries/dust-commitment-update-queries.test.ts`

#### Dust Commitment Merkle Tree Update Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a collapsed update query with valid index range, should return a collapsed update for a valid index range | 34 ms |
| ✓ | a collapsed update query with valid index range, should respond with a collapsed update according to the expected schema | 5 ms |
| ✓ | a collapsed update query with valid index range, should return a collapsed update for the full genesis dust range | 37 ms |
| ✓ | a collapsed update query with equal start and end indices, should return a valid update when startIndex equals endIndex | 4 ms |
| ✓ | a collapsed update query idempotency, should return identical results for the same query parameters | 5 ms |
| ✓ | a collapsed update query with invalid index range, should return an error when startIndex is greater than endIndex | 2 ms |
| ✓ | a collapsed update query with invalid index range, should return an error when indices are negative | 4 ms |
| ✓ | a collapsed update query with invalid index range, should return an error when endIndex is beyond the indexed range | 3 ms |

### Dust generation update queries

File name: `tests/integration/basic/queries/dust-generation-update-queries.test.ts`

#### Dust Generation Merkle Tree Update Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | a collapsed update query with valid index range, should return a collapsed update for a valid index range | — |
| ↓ | a collapsed update query with valid index range, should respond with a collapsed update according to the expected schema | — |
| ↓ | a collapsed update query with valid index range, should return a collapsed update for the full genesis dust range | — |
| ↓ | a collapsed update query with equal start and end indices, should return a valid update when startIndex equals endIndex | — |
| ↓ | a collapsed update query idempotency, should return identical results for the same query parameters | — |
| ↓ | a collapsed update query with invalid index range, should return an error when startIndex is greater than endIndex | — |
| ↓ | a collapsed update query with invalid index range, should return an error when indices are negative | — |
| ↓ | a collapsed update query with invalid index range, should return an error when endIndex is beyond the indexed range | — |

### Dust generations queries

File name: `tests/integration/basic/queries/dust-generations-queries.test.ts`

#### Dust Generations Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | a dust generations query with a valid Cardano reward address, should return dust generations for a registered address | — |
| ↓ | a dust generations query with a valid Cardano reward address, should respond with dust generations according to the expected schema | — |
| ↓ | a dust generations query with a valid Cardano reward address, should return registrations with valid fields for a registered address | — |
| ↓ | a dust generations query with multiple addresses, should return dust generations for multiple addresses | — |
| ↓ | a dust generations query with non-registered address, should return empty registrations for a non-registered address | — |
| ↓ | a dust generations query with invalid input, should return an empty result for an empty addresses array | — |
| ↓ | a dust generations query with invalid input, should return an error for a malformed address | — |
| ↓ | a dustGenerations query with a multi-UTXO stake key (#926), should aggregate nightBalance across all backing cNIGHT UTXOs | — |
| ↓ | backwards compatibility with dustGenerationStatus, should return consistent data from both endpoints for a registered address | — |

### Dust queries

File name: `tests/integration/basic/queries/dust-queries.test.ts`

#### Dust Generation Status Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | a dust generation status query with a valid Cardano reward address, should respond with a dust generation status response according to the requested schema | — |
| ↓ | a dust generation status query with a valid Cardano reward address, should report registered status for a registered Cardano reward address | — |
| ↓ | a dust generation status query with a valid Cardano reward address, should give the DUST destination address for the expected network when Cardano reward address is registered | — |
| ↓ | a dust generation status query with a valid Cardano reward address, should correctly indicate registration status for a non-registered key | — |
| ↓ | a dust generation status query with a valid Cardano reward address, should indicate zero generation for registered address without cNIGHT balance | — |
| ↓ | a dust generation status query with a valid Cardano reward address, should report the correct value of max capacity for registered address with cNIGHT | — |
| ↓ | a dust generation status query with a valid Cardano reward address, should correctly indicate deregistered status for a previously registered Cardano reward address | — |
| ↓ | a dust generation status query with multiple valid Cardano reward addresses, should return statuses for multiple Cardano reward addresses in order | — |
| ↓ | a dust generation status query with multiple valid Cardano reward addresses, should respond with dust generation statuses according to the requested schema for multiple addresses | — |
| ↓ | a dust generation status query with multiple valid Cardano reward addresses, should return an error given the number of addresses is greater than 10 | — |
| ↓ | a dust generation status query with malformed reward addresses, should return an error when the address is in plain hex string format | — |
| ↓ | a dust generation status query with empty list of reward addresses, should return an empty list of dust generation statuses | — |
| ↓ | a dust generation status query with a Cardano payment address, should return an error as only Cardano reward addresses are supported | — |
| ↓ | a dust generation status query with a Cardano reward address not meant for this network, should return an error reporting the target network mismatch | — |
| ↓ | a dust generation status query with duplicate reward addresses, should handle duplicate Cardano reward addresses appropriately | — |

### Graphql http compression

File name: `tests/integration/basic/queries/graphql-http-compression.test.ts`

#### Graphql Http Response Compression

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a GraphQL HTTP request with Accept-Encoding: gzip, should return a gzip-compressed response | 31 ms |
| ✓ | a GraphQL HTTP request with Accept-Encoding: br, should return a brotli-compressed response | 6 ms |
| ✓ | a GraphQL HTTP request with Accept-Encoding: zstd, should return a zstd-compressed response | 5 ms |
| ✓ | a GraphQL HTTP request with Accept-Encoding: identity, should return an uncompressed response | 5 ms |

### Spo epoch series queries

File name: `tests/integration/basic/queries/spo-epoch-series-queries.test.ts`

#### SPO Epoch Series Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a registered totals series query with an epoch span within the maximum, should accept a range covering a single epoch | 47 ms |
| ✓ | a registered totals series query with an epoch span within the maximum, should accept a range whose span equals the maximum | 8 ms |
| ✓ | a registered totals series query with an epoch span within the maximum, should accept a permitted span that does not start at epoch zero | 11 ms |
| ✓ | a registered totals series query with an epoch span exceeding the maximum, should reject a range one epoch beyond the maximum span | 2 ms |
| ✓ | a registered totals series query with an epoch span exceeding the maximum, should reject a reversed range whose span exceeds the maximum | 3 ms |
| ✓ | a registered totals series query with an epoch span exceeding the maximum, should reject a range far beyond the maximum span without returning data | 2 ms |
| ✓ | a registered SPO series query with an epoch span exceeding the maximum, should reject a range beyond the maximum span | 2 ms |
| ✓ | a registered SPO series query with an epoch span exceeding the maximum, should accept a range whose span equals the maximum | 123 ms |
| ✓ | a registered presence query with an epoch span exceeding the maximum, should reject a range beyond the maximum span | 4 ms |
| ✓ | a registered presence query with an epoch span exceeding the maximum, should accept a range whose span equals the maximum | 8 ms |

### Transaction queries

File name: `tests/integration/basic/queries/transaction-queries.test.ts`

#### Transaction Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a transaction query by hash, should return the transaction with that hash, given that transaction exists | 282 ms |
| ✓ | a transaction query by hash, should return an empty transaction list, given a transaction with that hash doesn't exist | 3 ms |
| ✓ | a transaction query by hash, should return an error, given a hash is invalid (malformed) | 10 ms |
| ✓ | a transaction query by hash, should return an error when called with an empty offset object | 2 ms |
| ✓ | a transaction query by identifier, should return the transaction with that identifier, given that transaction exists | 176 ms |
| ✓ | a transaction query by identifier, should return an empty list of transactions, given a transaction with that identifier doesn't exist | 2 ms |
| ✓ | a transaction query by identifier, should return an error, given an invalid identifier | 5 ms |
| ✓ | a transaction query by hash and identifier, should return an error, as only one parameter at a time can be used | 2 ms |

#### Genesis Transactions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | transaction queries to the genesis block transactions, should return utxos related to 4 pre-fund wallets | 190 ms |
| ✓ | transaction queries to the genesis block transactions, should return utxos with 1 token type | 183 ms |
| ✓ | transaction queries to the genesis block transactions, should return utxos sorted by outputIndex in ascending order | 209 ms |
| ✓ | transaction queries to the genesis block transactions, should return system transactions when queried by hash | 205 ms |
| ✓ | transaction queries to the genesis block transactions, should not contain RegularTransaction-specific fields on system transactions | 170 ms |
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
| ✓ | a collapsed update query with valid index range, should return a collapsed update for a valid index range | 33 ms |
| ✓ | a collapsed update query with valid index range, should respond with a collapsed update according to the expected schema | 5 ms |
| ✓ | a collapsed update query with valid index range, should return a collapsed update for the full genesis zswap range | 57 ms |
| ✓ | a collapsed update query with equal start and end indices, should return a valid update when startIndex equals endIndex | 3 ms |
| ✓ | a collapsed update query idempotency, should return identical results for the same query parameters | 5 ms |
| ✓ | a collapsed update query with invalid index range, should return an error when startIndex is greater than endIndex | 3 ms |
| ✓ | a collapsed update query with invalid index range, should return an error when indices are negative | 4 ms |
| ✓ | a collapsed update query with invalid index range, should return an error when endIndex is beyond the indexed range | 3 ms |

### Block subscriptions

File name: `tests/integration/basic/subscriptions/block-subscriptions.test.ts`

#### Block Subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a subscription to block updates without parameters, should stream blocks starting from the latest block | 5106 ms |
| ✓ | a subscription to block updates without parameters, should stream blocks adhering to the expected schema | 536 ms |
| ✓ | a subscription to block updates by hash, should stream blocks starting from the block with that hash, given that hash exists | 584 ms |
| ✓ | a subscription to block updates by hash, should return an error message, given that hash doesn't exist | 509 ms |
| ✓ | a subscription to block updates by hash, should return an error message, given that hash is invalid | 506 ms |
| ✓ | a subscription to block updates by height, should stream blocks from the block with that height, given that height exists | 570 ms |
| ✓ | a subscription to block updates by height, should return an error message, given that height is higher than the latest block height | 519 ms |
| ✓ | a subscription to block updates by height, should return an error message, given that height is invalid | 506 ms |
| ✓ | a subscription to block updates by height and hash, should return an error message, as only one parameter at a time can be used | 516 ms |
| ↓ | regular transaction fees, should report ledger paidFees and estimatedFees on regular transactions | — |

### Contract subscriptions

File name: `tests/integration/basic/subscriptions/contract-subscriptions.test.ts`

#### Contract Action Subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a subscription to contract action updates without parameters, should stream contract actions from the latest available block | 537 ms |
| ✓ | a subscription to contract action updates without parameters, should stream contract actions adhering to the expected schema | 519 ms |
| ✓ | a subscription to contract action updates with block hash offset, should stream historical and new contract actions from a specific block hash | 512 ms |

### Dust generations subscriptions

File name: `tests/integration/basic/subscriptions/dust-generations-subscriptions.test.ts`

#### Dust Generations Subscription

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | streaming dust generation entries, should stream dust generation events for a valid dust address in bech32m format | — |
| ↓ | fresh subscription dtime delivery (#1167), fresh startIndex=0 subscription delivers DustGenerationDtimeUpdateItem for a wallet with spent NIGHT | — |
| ↓ | fresh subscription dtime delivery (#1167), resumption startIndex>0 delivers only in-range items and completes correctly | — |
| ↓ | subscription error handling, should return an error for an invalid dust address | — |
| ↓ | subscription error handling, should return an error for a valid address that is meant for another networkid | — |
| ↓ | subscription error handling, should return an error for a valid dust address passed in hex format | — |
| ↓ | transactionHash on dust generation events (#1114), first item transactionHash resolves via transactions(offset) | — |

### Dust ledger subscriptions

File name: `tests/integration/basic/subscriptions/dust-ledger-subscriptions.test.ts`

#### Dust Ledger Event Subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a subscription to dust ledger events without offset (default replay), streams events in ledger order | 539 ms |
| ✓ | subscription with explicit offset, streams events in ledger order starting from the specified ID | 520 ms |
| ✓ | subscription with explicit offset, validates historical dust events against schema | 512 ms |
| ✓ | subscription error handling, should return an error for unknown field | 504 ms |
| ✓ | subscription error handling, rejects negative offset ID with an error | 503 ms |

### Dust nullifier subscriptions

File name: `tests/integration/basic/subscriptions/dust-nullifier-subscriptions.test.ts`

#### Dust Nullifier Transactions Subscription

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | streaming dust nullifier transactions with block range, should stream transactions within a block range and complete | 543 ms |
| ✓ | subscription error handling, should return an error for empty nullifier prefixes | 505 ms |
| ✓ | subscription error handling, should return an error when fromBlock is greater than toBlock | 504 ms |
| ↓ | transactionHash on dust nullifier events (#1114), first event transactionHash resolves via transactions(offset) | — |
| ↓ | transaction reference on dust nullifier events (#1115), first event transaction.hash matches transactionHash | — |

### Shielded nullifier subscriptions

File name: `tests/integration/basic/subscriptions/shielded-nullifier-subscriptions.test.ts`

#### Shielded Nullifier Transactions Subscription

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | streaming shielded nullifier transactions with block range, should stream transactions within a block range and complete | 544 ms |
| ✓ | subscription error handling, should return an error for empty nullifier prefixes | 505 ms |
| ✓ | subscription error handling, should return an error for an empty-string nullifier prefix element | 504 ms |
| ✓ | subscription error handling, should return an error when fromBlock is greater than toBlock | 503 ms |
| ↓ | transactionHash on shielded nullifier events (#1114), first event transactionHash resolves via transactions(offset) | — |
| ↓ | transaction reference on shielded nullifier events (#1115), first event transaction.hash matches transactionHash | — |

### Shielded transaction subscriptions

File name: `tests/integration/basic/subscriptions/shielded-transaction-subscriptions.test.ts`

#### Shielded Transaction Subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | opening a session with viewing key, should return a session ID, given a valid viewing key | 565 ms |
| ✓ | opening a session with viewing key, should return an error, given an unsupported hex format viewing key | 503 ms |
| ✓ | opening a session with viewing key, should return an error, given an invalid viewing key | 503 ms |
| ✓ | opening a session with viewing key, should return an error, given a valid viewing key meant for a different network | 782 ms |
| ✓ | closing a session with session ID, should terminate the session successfully, given a valid session ID | 561 ms |
| ✓ | closing a session with session ID, should return an error, given an invalid session ID | 504 ms |
| ✓ | a subscription to wallet updates providing viewing key only, should stream wallet events starting from the beginning, given there are relevant transactions | 2557 ms |
| ✓ | a subscription to wallet updates providing viewing key only, should stream shielded transaction events adhering to the expected schema | 3565 ms |
| ✓ | a subscription to wallet updates providing viewing key only, should be able to use highestZswapEndIndex from progress event in collapsed update query | 560 ms |
| ✓ | a subscription to wallet updates providing viewing key only, should reject shieldedTransactions subscription when using expired session ID | 545 ms |

### Subscription quotas

File name: `tests/integration/basic/subscriptions/subscription-quotas.test.ts`

#### Subscription Quotas

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | per-connection concurrent subscription cap, should reject a subscription beyond the per-connection cap | 1517 ms |
| ✓ | per-connection concurrent subscription cap, should free a slot when an active subscription is closed | 2714 ms |
| ✓ | per-session subscription creation rate limit, should reject shielded subscription creations beyond the per-session rate limit | 1572 ms |
| ✓ | per-session subscription creation rate limit, should allow a new shielded subscription after the rate limit refills | 9574 ms |
| ✓ | per-session subscription creation rate limit, should keep a live shielded subscription streaming while creations are rate-limited | 36121 ms |

### Unshielded transaction subscriptions

File name: `tests/integration/basic/subscriptions/unshielded-transaction-subscriptions.test.ts`

#### Unshielded Transaction Subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a subscription to unshielded transaction events by address, should stream unshielded transaction events related to that address, given that address has transactions | 5064 ms |
| ✓ | a subscription to unshielded transaction events by address, should stream unshielded transaction events up to highest transaction id | 76 ms |
| ✓ | a subscription to unshielded transaction events by address, should stream unshielded transaction events that adhere to the expected schema | 70 ms |
| ✓ | a subscription to unshielded transaction events by address, should only return a transaction progress message with highest transaction = 0, given that address does not have transactions | 5043 ms |
| ✓ | a subscription to unshielded transaction events by address, should return an error message, given the address provided is in hex format | 2 ms |
| ✓ | a subscription to unshielded transaction events by address, should return an error message, given the address provided is for another network | 206 ms |
| ✓ | a subscription to unshielded transaction events by address and transaction id, should return a stream of transactions containing that address, starting from transaction id = 0 | 80 ms |
| ✓ | a subscription to unshielded transaction events by address and transaction id, should return an error message, given the transaction id provided is negative | 45 ms |
| ✓ | a subscription to unshielded transaction events by address and transaction id, should only return a transaction progress message without streaming transactions, given that the transaction id provided is bigger number | 5041 ms |
| ✓ | a subscription to unshielded transaction events by address and transaction id, should start a transaction stream from the given transaction id | 10059 ms |
| ✓ | a subscription to unshielded transaction events by address and transaction id, should return an error message, given the address is provided in hex format | 2 ms |

### Wallet connect options

File name: `tests/integration/basic/subscriptions/wallet-connect-options.test.ts`

#### Wallet Connect Options (StartIndex)

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | opening a session with startIndex, should accept startIndex = 0 (no-op equivalent of unset options) | — |
| ↓ | opening a session with startIndex, should skip historical scan when startIndex equals the current tip | — |
| ↓ | opening a session with startIndex, should accept startIndex past the current tip (fast-forward) | — |

### Websocket keepalive

File name: `tests/integration/basic/subscriptions/websocket-keepalive.test.ts`

#### Websocket Keepalive

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | an idle socket without the client keepalive, should be closed while idle past the idle limit | — |
| ✓ | an idle socket with the client keepalive, should stay open while idle past the idle limit | 150531 ms |

### Zswap events subscriptions

File name: `tests/integration/basic/subscriptions/zswap-events-subscriptions.test.ts`

#### Zswap Ledger Event Subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a subscription to zswap ledger events without offset (default replay), streams events in ledger order | 532 ms |
| ✓ | subscription with explicit offset, streams events starting from the specified ID | 515 ms |
| ✓ | subscription with explicit offset, validates historical zswap events against schema | 509 ms |
| ✓ | subscription error handling, should return an error for unknown field | 504 ms |
| ✓ | subscription error handling, rejects negative offset ID with an error | 506 ms |

## E2E results

### Contract actions partial success

File name: `tests/e2e/contract-actions-partial-success.test.ts`

#### Contract Actions In A Guaranteed/fallible Segment Split

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a Call whose guaranteed transcript applied but whose fallible segment failed, the fixture produces a call with a guaranteed transcript | 10 ms |
| ✓ | a Call whose guaranteed transcript applied but whose fallible segment failed, the fixture produces a partially successful transaction | 1 ms |
| ✓ | a Call whose guaranteed transcript applied but whose fallible segment failed, is still reported by the indexer | 1 ms |
| ✓ | a Call where neither execution phase applied, the fixture produces a call without a guaranteed transcript | 1 ms |
| ✓ | a Call where neither execution phase applied, the fixture still produces a partially successful transaction | 0 ms |
| ✓ | a Call where neither execution phase applied, is not reported by the indexer | 0 ms |

### Contract actions sequential

File name: `tests/e2e/contract-actions-sequential.test.ts`

#### Contract Actions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a transaction to deploy a counter smart contract, should be reported by the indexer through a transaction query by hash | 18 ms |
| ✓ | a transaction to deploy a counter smart contract, should be reported by the indexer through a block query by hash | 12 ms |
| ✓ | a transaction to deploy a counter smart contract, should be reported by the indexer through a contract action query by address | 8 ms |
| ✓ | a transaction to call the store circuit on the deployed counter smart contract, should be reported by the indexer through a transaction query by hash | 18 ms |
| ✓ | a transaction to call the store circuit on the deployed counter smart contract, should be reported by the indexer through a block query by hash | 23 ms |
| ✓ | a transaction to call the store circuit on the deployed counter smart contract, should be reported by the indexer through a contract action query by address | 11 ms |
| ✓ | a transaction to update the counter smart contract, should be reported by the indexer through a transaction query by hash | 12 ms |
| ✓ | a transaction to update the counter smart contract, should be reported by the indexer through a block query by hash | 23 ms |
| ✓ | a transaction to update the counter smart contract, should be reported by the indexer through a contract action query by address | 18 ms |

### Custom unshielded token transactions

File name: `tests/e2e/custom-unshielded-token-transactions.test.ts`

#### Unshielded Custom Token Transactions

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | a successful unshielded transaction transferring 1 token between two addresses, should be reported by the indexer through a block query by hash | — |
| ↓ | a successful unshielded transaction transferring 1 token between two addresses, should be reported by the indexer through a transaction query by hash | — |
| ↓ | a successful unshielded transaction transferring 1 token between two addresses, should be reported by the indexer through an unshielded transaction event for the source address | — |
| ↓ | a successful unshielded transaction transferring 1 token between two addresses, should be reported by the indexer through an unshielded transaction event for the destination address | — |
| ↓ | a successful unshielded transaction transferring 1 token between two addresses, should have transferred 1 token from the source to the destination address | — |
| ↓ | a successful unshielded transaction transferring 1 token between two addresses, should report the token type under test on every created and spent output | — |
| ↓ | a successful unshielded transaction transferring 1 token between two addresses, should be reported by the indexer through a progress update event for the source address | — |
| ↓ | a successful unshielded transaction transferring 1 token between two addresses, should be reported by the indexer through a progress update event for the destination address | — |

### Night transactions

File name: `tests/e2e/night-transactions.test.ts`

#### Unshielded NIGHT Transactions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses, should be reported by the indexer through a block query by hash | 43 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses, should be reported by the indexer through a transaction query by hash | 30 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses, should be reported by the indexer through an unshielded transaction event for the source address | 1 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses, should be reported by the indexer through an unshielded transaction event for the destination address | 2 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses, should have transferred 1 STAR from the source to the destination address | 27 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses, should report the token type under test on every created and spent output | 20 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses, should be reported by the indexer through a progress update event for the source address | 5005 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses, should be reported by the indexer through a progress update event for the destination address | 1 ms |
| ✓ | the genesis block of a chain minted with NIGHT alone, should report the all-zeros NIGHT token type on its outputs | 2 ms |
| ✓ | the dust activity of a confirmed NIGHT transaction, should increase the dust commitment Merkle tree end index | 40 ms |
| ✓ | a confirmed unshielded transfer streamed to address subscriptions, should emit UnshieldedTransaction only for the target wallet (A, B1) | 19889 ms |
| ✓ | a confirmed unshielded transfer streamed to address subscriptions, should emit UnshieldedTransaction only for the target wallet (A, B2) | 17424 ms |
| ✓ | an address with no transaction history, should emit only ProgressUpdate for empty wallet | 2598 ms |

### Segment probe contract actions

File name: `tests/e2e/segment-probe-contract-actions.test.ts`

#### Segment-split Contract Actions

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | a transaction to deploy a segment-split smart contract, should be reported by the indexer through a transaction query by hash | — |
| ↓ | a transaction to deploy a segment-split smart contract, should be reported by the indexer through a block query by hash | — |
| ↓ | a transaction to deploy a segment-split smart contract, should be reported by the indexer through a contract action query by address | — |
| ↓ | a transaction to call the burnWithGuaranteed circuit on the deployed segment-split smart contract, should be reported by the indexer through a transaction query by hash | — |
| ↓ | a transaction to call the burnWithGuaranteed circuit on the deployed segment-split smart contract, should be reported by the indexer through a block query by hash | — |
| ↓ | a transaction to call the burnWithGuaranteed circuit on the deployed segment-split smart contract, should be reported by the indexer through a contract action query by address | — |

### Shielded transactions

File name: `tests/e2e/shielded-transactions.test.ts`

#### Shielded Transactions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a successful shielded transaction transferring 1 Shielded Token between two wallets, should be reported by the indexer through a block query by hash | 33 ms |
| ✓ | a successful shielded transaction transferring 1 Shielded Token between two wallets, should be reported by the indexer through a transaction query by hash | 22 ms |
| ✓ | a successful shielded transaction transferring 1 Shielded Token between two wallets, should stream Zswap events followed by DustSpendProcessed after a shielded transaction | 531 ms |
| ✓ | a successful shielded transaction transferring 1 Shielded Token between two wallets, should increase the zswap Merkle tree end index | 18 ms |
| ✓ | a successful shielded transaction transferring 1 Shielded Token between two wallets, should increase the dust commitment Merkle tree end index | 15 ms |
| ✓ | a confirmed shielded transfer streamed to wallet sessions by viewing key, should stream the transaction to the source viewing key with a matching hash | 3909 ms |
| ✓ | a confirmed shielded transfer streamed to wallet sessions by viewing key, should stream the transaction to the destination viewing key with a matching hash | 559 ms |
| ✓ | a confirmed shielded transfer streamed to wallet sessions by viewing key, should not stream the transaction to an unrelated viewing key | 20551 ms |

### Subscription polling

File name: `tests/e2e/subscription-polling.test.ts`

#### Subscription Polling

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | an unshielded transactions subscription for a never-used address, should emit progress immediately and back off while the address is idle | 90136 ms |
| ✓ | an unshielded transactions subscription for a never-used address, should reset the polling interval when the address receives a transaction | 154810 ms |
| ✓ | the keep-alive heartbeat of a shielded transactions subscription, should refresh wallet activity once at subscription start and then throttle | 92768 ms |
| □ | the keep-alive heartbeat of a shielded transactions subscription, should keep a subscribed wallet active beyond the wallet-indexer inactivity TTL | — |
| ✓ | a shielded transactions subscription on an idle wallet, should emit progress immediately and keep emitting while idle | 24672 ms |

### Unshielded balance e2e

File name: `tests/e2e/unshielded-balance-e2e.test.ts`

#### Unshielded Balance Indexing (Deploy + Mint + Verify) [#1253 Scaffold]

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | reports the contract remainder after mint-to-self then partial send | — |

### Toolkit: Dust balance

File name: `tests/e2e/toolkit/dust-balance.test.ts`

#### Dust Balance Query Using Toolkit

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a dust balance query with a valid wallet seed, should respond with a dust balance according to the requested schema | 425 ms |

### Toolkit: Key material

File name: `tests/e2e/toolkit/key-material.test.ts`

#### Key Material Derivation Validation

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a midnight shielded address, should show with the expected prefix for the current network ID | 55 ms |
| ✓ | a midnight shielded address, should show with the expected prefix for all network IDs | 281 ms |
| ✓ | a midnight unshielded address, should show with the expected prefix for the current network ID | 58 ms |
| ✓ | a midnight unshielded address, should show with the expected prefix for all network IDs | 248 ms |
| ✓ | a midnight viewing key, should show with the expected prefix for the current network ID | 48 ms |
| ✓ | a midnight viewing key, should show with the expected prefix for all network IDs | 226 ms |

### Toolkit: Show wallet queries

File name: `tests/e2e/toolkit/show-wallet-queries.test.ts`

#### Show Wallet Queries Using Toolkit

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | private wallet state query using toolkit, should respond with a private wallet state according to the requested schema | 466 ms |
| ✓ | public wallet state query using toolkit, should respond with a public wallet state according to the requested schema | 706 ms |
