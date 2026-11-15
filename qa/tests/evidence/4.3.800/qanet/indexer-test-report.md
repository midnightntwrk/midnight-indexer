# Indexer Test Execution Report

## Run Details

| Field | Value |
| --- | --- |
| Component Under Test | Indexer / Node |
| Indexer Version | 4.3.800 |
| Node Version | 1.0.400 (`1.0.400-c338b9ac`) |
| Test Suites | smoke, integration, e2e |
| Tests | Indexer GraphQL API, integration with Node and end-to-end transaction flows |
| Environment | qanet |
| Date | Tuesday 27 Oct 2026 |
| QA Contact | Oleksandr Romanov (oleksandr.romanov@shielded.io) |

## Summary

| Suite | Test Files | Passed | Failed | Skipped | Todo | Total | Duration | Run at |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- |
| smoke | 3 passed, 1 skipped (4) | 18 | 0 | 1 | 0 | 19 | 3.27s | 15:44 EET |
| integration | 21 passed, 1 failed (retested on preview) (22) | 195 | 1 | 1 | 0 | 197 | 699.99s | 15:50 EET |
| e2e | 8 passed, 3 skipped (11) | 53 | 0 | 11 | 1 | 65 | 573.29s | 16:03 EET |
| **Total** | | **266** | **1** | **13** | **1** | **281** | | |

> Legend: ✓ passed · ✗ failed · ↓ skipped · □ todo.
> Duration is the total test time reported in each suite's `junit-test-results.xml`.

## Failures

### ✗ contract queries a contract query by address should return non-empty unshieldedBalances for contracts known to hold tokens

- Suite: integration
- File: `tests/integration/basic/queries/contract-queries.test.ts:146`

NOTE: 
- Root cause: indexer qanet was not synced from scratch, so the fix have not been applied.
- Fix have been retested against preview environment, indexer 4.3.800 (synced from 0) - and it successfully works.

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

### Graphql healthchecks

File name: `tests/smoke/graphql-healthchecks.test.ts`

#### Graphql Health Checks

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | an introspection message sent to the websocket channel, should return the supported graphql schema | 716 ms |
| ✓ | an introspection message sent to the websocket channel, should return an error, given the depth of the query is > 15 | 698 ms |
| ✓ | an introspection request sent to the http channel, should return the supported graphql schema | 310 ms |
| ✓ | an introspection request sent to the http channel, should return an error, given the depth of the query is > 15 | 73 ms |

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
| ✓ | RegularTransaction.merkleTreeRoot, should be marked deprecated with reason "Use zswapMerkleTreeRoot instead" | 290 ms |
| ✓ | RegularTransaction.startIndex, should be marked deprecated with reason "Use zswapStartIndex instead" | 68 ms |
| ✓ | RegularTransaction.endIndex, should be marked deprecated with reason "Use zswapEndIndex instead" | 67 ms |
| ✓ | RelevantTransaction.collapsedMerkleTree, should be marked deprecated with reason "Use zswapCollapsedUpdate instead" | 62 ms |
| ✓ | ShieldedTransactionsProgress.highestEndIndex, should be marked deprecated with reason "Use highestZswapEndIndex instead" | 62 ms |
| ✓ | ShieldedTransactionsProgress.highestCheckedEndIndex, should be marked deprecated with reason "Use highestCheckedZswapEndIndex instead" | 61 ms |
| ✓ | ShieldedTransactionsProgress.highestRelevantEndIndex, should be marked deprecated with reason "Use highestRelevantZswapEndIndex instead" | 61 ms |
| ✓ | RegularTransaction.fees, should be marked deprecated with reason "Use fee instead" | 62 ms |
| ✓ | TransactionFees.estimatedFees, should be marked deprecated with reason "Use paidFees instead" | 66 ms |

### Service healthchecks

File name: `tests/smoke/service-healthchecks.test.ts`

#### Service Health Checks

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a request to the /ready endpoint, should return a 200 status code OK | 285 ms |
| ✓ | a request to an unrecognised path /api/v3/__regression_unknown_path, should not 308 to a version-double-prefixed Location | 196 ms |
| ✓ | a request to an unrecognised path /api/v3/__regression_unknown_path, should terminate when redirects are followed | 61 ms |
| ✓ | a request to an unrecognised path /api/v4/__regression_unknown_path, should not 308 to a version-double-prefixed Location | 66 ms |
| ✓ | a request to an unrecognised path /api/v4/__regression_unknown_path, should terminate when redirects are followed | 59 ms |

## Integration results

### Block queries

File name: `tests/integration/basic/queries/block-queries.test.ts`

#### Block Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a block query without parameters, should return the latest known block | 423 ms |
| ✓ | a block query without parameters, should respond with a block according to the requested schema | 223 ms |
| ✓ | a block query by hash, should return the block with that hash, given that block exists | 146 ms |
| ✓ | a block query by hash, should return blocks according to the requested schema | 159 ms |
| ✓ | a block query by hash, should return a null block, given a block with that hash doesn't exist | 67 ms |
| ✓ | a block query by hash, should return an error, when the hash is invalid (malformed) | 328 ms |
| ✓ | a block query by height, should return the block with that height, given a valid height | 147 ms |
| ✓ | a block query by height, should return a blocks according to the requested schema | 163 ms |
| ✓ | a block query by height, should return the genesis block, given height=0 is requested | 619 ms |
| ✓ | a block query by height, should return an empty body answer, given that block height requested is the maximum available height | 71 ms |
| ✓ | a block query by height, should return an error, given an invalid height | 216 ms |
| ✓ | a block query by height and hash, should return an error, as only one parameter at a time can be used | 349 ms |
| ✓ | block-level tree end indexes, should return non-negative integers for all three tree end index fields | 95 ms |
| ✓ | block-level tree end indexes, should return non-decreasing tree end indexes from parent to child block | 163 ms |

#### Genesis Block

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a block query to the genesis block, should contain transactions with pre-fund wallet utxos | 696 ms |
| ✓ | a block query to the genesis block, should contain utxos related to exactly 4 pre-fund wallets | 289 ms |
| ✓ | a block query to the genesis block, should contain utxos with exactly 1 token type | 313 ms |
| ✓ | a block query to the genesis block, should contain utxos sorted by outputIndex in ascending order | 274 ms |
| ✓ | a block query to the genesis block, should contain valid index ranges on regular transactions | 251 ms |
| ✓ | a block query to the genesis block, should return block-level end indexes >= max RegularTransaction end indexes | 236 ms |

### Contract queries

File name: `tests/integration/basic/queries/contract-queries.test.ts`

#### Contract Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a contract query by address, should return success as they are valid contract addresses | 610 ms |
| ✓ | a contract query by address, should return the most recent action for a contract with multiple actions | 97 ms |
| ✓ | a contract query by address, should return null when contract with that address does not exist | 75 ms |
| ✓ | a contract query by address, should return an error for malformed addresses | 1414 ms |
| ✗ | a contract query by address, should return non-empty unshieldedBalances for contracts known to hold tokens | 213 ms |
| ✓ | a contract query by address and offset, should return the correct action using exact block hash where it was included | 162 ms |
| ✓ | a contract query by address and offset, should return the latest state using a future block hash | 101 ms |
| ✓ | a contract query by address and offset, should respond with a contract action according to the expected schema | 110 ms |
| ✓ | a contract query by address and offset, should return the correct action using exact block height where it was included | 132 ms |
| ✓ | a contract query by address and offset, should return the latest state using a future block height | 88 ms |
| ✓ | a contract query by address and offset, should return the most recent contract action for that address before the specified block | 213 ms |
| ✓ | a contract query by address and offset, should return null when contract with valid address and valid offset does not exist | 75 ms |
| ✓ | a contract query by address and offset, should return null when contract with valid address and non-existing hash does not exist | 308 ms |
| ✓ | a contract query by address and offset, should return error when contract with invalid address and valid hash | 63 ms |
| ✓ | a contract query by address and offset, should return error when contract with invalid address and non-existing hash | 66 ms |
| ✓ | a contract query by address and offset, should return error when contract with valid address and invalid hash | 65 ms |
| ✓ | a contract query by address and offset, should return error when contract with invalid address and invalid hash | 328 ms |
| ✓ | a contract query by address and offset, should return null when contract with valid address and valid height does not exist | 66 ms |
| ✓ | a contract query by address and offset, should return null when contract with valid address and non-existing height does not exist | 67 ms |
| ✓ | a contract query by address and offset, should return error when contract with invalid address and valid height | 63 ms |
| ✓ | a contract query by address and offset, should return error when contract with invalid address and invalid height | 208 ms |
| ✓ | a contract query by address and offset, should return error for negative height | 64 ms |
| ✓ | a contract query by address and offset, should return error for non-integer height | 64 ms |
| ✓ | a contract query by address and offset, should return error for extremely large height | 65 ms |
| ✓ | a contract query by address and offset, should return null when using a block hash from before the action existed | 309 ms |

### Dust commitment update queries

File name: `tests/integration/basic/queries/dust-commitment-update-queries.test.ts`

#### Dust Commitment Merkle Tree Update Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a collapsed update query with valid index range, should return a collapsed update for a valid index range | 458 ms |
| ✓ | a collapsed update query with valid index range, should respond with a collapsed update according to the expected schema | 71 ms |
| ✓ | a collapsed update query with valid index range, should return a collapsed update for the full genesis dust range | 693 ms |
| ✓ | a collapsed update query with equal start and end indices, should return a valid update when startIndex equals endIndex | 73 ms |
| ✓ | a collapsed update query idempotency, should return identical results for the same query parameters | 161 ms |
| ✓ | a collapsed update query with invalid index range, should return an error when startIndex is greater than endIndex | 68 ms |
| ✓ | a collapsed update query with invalid index range, should return an error when indices are negative | 66 ms |
| ✓ | a collapsed update query with invalid index range, should return an error when endIndex is beyond the indexed range | 71 ms |

### Dust generation update queries

File name: `tests/integration/basic/queries/dust-generation-update-queries.test.ts`

#### Dust Generation Merkle Tree Update Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a collapsed update query with valid index range, should return a collapsed update for a valid index range | 311 ms |
| ✓ | a collapsed update query with valid index range, should respond with a collapsed update according to the expected schema | 77 ms |
| ✓ | a collapsed update query with valid index range, should return a collapsed update for the full genesis dust range | 382 ms |
| ✓ | a collapsed update query with equal start and end indices, should return a valid update when startIndex equals endIndex | 69 ms |
| ✓ | a collapsed update query idempotency, should return identical results for the same query parameters | 163 ms |
| ✓ | a collapsed update query with invalid index range, should return an error when startIndex is greater than endIndex | 62 ms |
| ✓ | a collapsed update query with invalid index range, should return an error when indices are negative | 66 ms |
| ✓ | a collapsed update query with invalid index range, should return an error when endIndex is beyond the indexed range | 3961 ms |

### Dust generations queries

File name: `tests/integration/basic/queries/dust-generations-queries.test.ts`

#### Dust Generations Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a dust generations query with a valid Cardano reward address, should return dust generations for a registered address | 320 ms |
| ✓ | a dust generations query with a valid Cardano reward address, should respond with dust generations according to the expected schema | 74 ms |
| ✓ | a dust generations query with a valid Cardano reward address, should return registrations with valid fields for a registered address | 80 ms |
| ✓ | a dust generations query with multiple addresses, should return dust generations for multiple addresses | 75 ms |
| ✓ | a dust generations query with non-registered address, should return empty registrations for a non-registered address | 66 ms |
| ✓ | a dust generations query with invalid input, should return an empty result for an empty addresses array | 65 ms |
| ✓ | a dust generations query with invalid input, should return an error for a malformed address | 264 ms |
| ✓ | a dustGenerations query with a multi-UTXO stake key (#926), should aggregate nightBalance across all backing cNIGHT UTXOs | 194 ms |
| ✓ | backwards compatibility with dustGenerationStatus, should return consistent data from both endpoints for a registered address | 91 ms |

### Dust queries

File name: `tests/integration/basic/queries/dust-queries.test.ts`

#### Dust Generation Status Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a dust generation status query with a valid Cardano reward address, should respond with a dust generation status response according to the requested schema | 310 ms |
| ✓ | a dust generation status query with a valid Cardano reward address, should report registered status for a registered Cardano reward address | 72 ms |
| ✓ | a dust generation status query with a valid Cardano reward address, should give the DUST destination address for the expected network when Cardano reward address is registered | 69 ms |
| ✓ | a dust generation status query with a valid Cardano reward address, should correctly indicate registration status for a non-registered key | 66 ms |
| ✓ | a dust generation status query with a valid Cardano reward address, should indicate zero generation for registered address without cNIGHT balance | 69 ms |
| ✓ | a dust generation status query with a valid Cardano reward address, should report the correct value of max capacity for registered address with cNIGHT | 68 ms |
| ✓ | a dust generation status query with a valid Cardano reward address, should correctly indicate deregistered status for a previously registered Cardano reward address | 78 ms |
| ✓ | a dust generation status query with multiple valid Cardano reward addresses, should return statuses for multiple Cardano reward addresses in order | 127 ms |
| ✓ | a dust generation status query with multiple valid Cardano reward addresses, should respond with dust generation statuses according to the requested schema for multiple addresses | 90 ms |
| ✓ | a dust generation status query with multiple valid Cardano reward addresses, should return an error given the number of addresses is greater than 10 | 75 ms |
| ✓ | a dust generation status query with malformed reward addresses, should return an error when the address is in plain hex string format | 69 ms |
| ✓ | a dust generation status query with empty list of reward addresses, should return an empty list of dust generation statuses | 66 ms |
| ✓ | a dust generation status query with a Cardano payment address, should return an error as only Cardano reward addresses are supported | 62 ms |
| ✓ | a dust generation status query with a Cardano reward address not meant for this network, should return an error reporting the target network mismatch | 71 ms |
| ✓ | a dust generation status query with duplicate reward addresses, should handle duplicate Cardano reward addresses appropriately | 76 ms |

### Graphql http compression

File name: `tests/integration/basic/queries/graphql-http-compression.test.ts`

#### Graphql Http Response Compression

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a GraphQL HTTP request with Accept-Encoding: gzip, should return a gzip-compressed response | 311 ms |
| ✓ | a GraphQL HTTP request with Accept-Encoding: br, should return a brotli-compressed response | 68 ms |
| ✓ | a GraphQL HTTP request with Accept-Encoding: zstd, should return a zstd-compressed response | 72 ms |
| ✓ | a GraphQL HTTP request with Accept-Encoding: identity, should return an uncompressed response | 262 ms |

### Spo epoch series queries

File name: `tests/integration/basic/queries/spo-epoch-series-queries.test.ts`

#### SPO Epoch Series Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a registered totals series query with an epoch span within the maximum, should accept a range covering a single epoch | 321 ms |
| ✓ | a registered totals series query with an epoch span within the maximum, should accept a range whose span equals the maximum | 75 ms |
| ✓ | a registered totals series query with an epoch span within the maximum, should accept a permitted span that does not start at epoch zero | 72 ms |
| ✓ | a registered totals series query with an epoch span exceeding the maximum, should reject a range one epoch beyond the maximum span | 64 ms |
| ✓ | a registered totals series query with an epoch span exceeding the maximum, should reject a reversed range whose span exceeds the maximum | 66 ms |
| ✓ | a registered totals series query with an epoch span exceeding the maximum, should reject a range far beyond the maximum span without returning data | 65 ms |
| ✓ | a registered SPO series query with an epoch span exceeding the maximum, should reject a range beyond the maximum span | 72 ms |
| ✓ | a registered SPO series query with an epoch span exceeding the maximum, should accept a range whose span equals the maximum | 149 ms |
| ✓ | a registered presence query with an epoch span exceeding the maximum, should reject a range beyond the maximum span | 70 ms |
| ✓ | a registered presence query with an epoch span exceeding the maximum, should accept a range whose span equals the maximum | 95 ms |

### Transaction queries

File name: `tests/integration/basic/queries/transaction-queries.test.ts`

#### Transaction Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a transaction query by hash, should return the transaction with that hash, given that transaction exists | 2953 ms |
| ✓ | a transaction query by hash, should return an empty transaction list, given a transaction with that hash doesn't exist | 169 ms |
| ✓ | a transaction query by hash, should return an error, given a hash is invalid (malformed) | 628 ms |
| ✓ | a transaction query by hash, should return an error when called with an empty offset object | 86 ms |
| ✓ | a transaction query by identifier, should return the transaction with that identifier, given that transaction exists | 2571 ms |
| ✓ | a transaction query by identifier, should return an empty list of transactions, given a transaction with that identifier doesn't exist | 78 ms |
| ✓ | a transaction query by identifier, should return an error, given an invalid identifier | 183 ms |
| ✓ | a transaction query by hash and identifier, should return an error, as only one parameter at a time can be used | 62 ms |

#### Genesis Transactions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | transaction queries to the genesis block transactions, should return utxos related to 4 pre-fund wallets | 2701 ms |
| ✓ | transaction queries to the genesis block transactions, should return utxos with 1 token type | 3147 ms |
| ✓ | transaction queries to the genesis block transactions, should return utxos sorted by outputIndex in ascending order | 2884 ms |
| ✓ | transaction queries to the genesis block transactions, should return system transactions when queried by hash | 3304 ms |
| ✓ | transaction queries to the genesis block transactions, should not contain RegularTransaction-specific fields on system transactions | 2460 ms |
| ✓ | schema validation, should respond with full transaction data according to the expected schema | 17 ms |
| ✓ | schema validation, should respond with system transactions according to the expected schema | 1 ms |
| ✓ | schema validation, should contain both regular and system transactions | 0 ms |
| ✓ | schema validation, should respond with regular transactions according to the expected schema | 3 ms |
| ✓ | schema validation, should respond with nested ledger events and unshielded outputs according to the expected schema | 1 ms |

### Zswap collapsed update queries

File name: `tests/integration/basic/queries/zswap-collapsed-update-queries.test.ts`

#### Zswap Merkle Tree Collapsed Update Queries

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a collapsed update query with valid index range, should return a collapsed update for a valid index range | 348 ms |
| ✓ | a collapsed update query with valid index range, should respond with a collapsed update according to the expected schema | 94 ms |
| ✓ | a collapsed update query with valid index range, should return a collapsed update for the full genesis zswap range | 973 ms |
| ✓ | a collapsed update query with equal start and end indices, should return a valid update when startIndex equals endIndex | 164 ms |
| ✓ | a collapsed update query idempotency, should return identical results for the same query parameters | 166 ms |
| ✓ | a collapsed update query with invalid index range, should return an error when startIndex is greater than endIndex | 103 ms |
| ✓ | a collapsed update query with invalid index range, should return an error when indices are negative | 127 ms |
| ✓ | a collapsed update query with invalid index range, should return an error when endIndex is beyond the indexed range | 79 ms |

### Block subscriptions

File name: `tests/integration/basic/subscriptions/block-subscriptions.test.ts`

#### Block Subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a subscription to block updates without parameters, should stream blocks starting from the latest block | 1907 ms |
| ✓ | a subscription to block updates without parameters, should stream blocks adhering to the expected schema | 933 ms |
| ✓ | a subscription to block updates by hash, should stream blocks starting from the block with that hash, given that hash exists | 2031 ms |
| ✓ | a subscription to block updates by hash, should return an error message, given that hash doesn't exist | 695 ms |
| ✓ | a subscription to block updates by hash, should return an error message, given that hash is invalid | 750 ms |
| ✓ | a subscription to block updates by height, should stream blocks from the block with that height, given that height exists | 1275 ms |
| ✓ | a subscription to block updates by height, should return an error message, given that height is higher than the latest block height | 1097 ms |
| ✓ | a subscription to block updates by height, should return an error message, given that height is invalid | 772 ms |
| ✓ | a subscription to block updates by height and hash, should return an error message, as only one parameter at a time can be used | 706 ms |
| ↓ | regular transaction fees, should report ledger paidFees and estimatedFees on regular transactions | — |

### Contract subscriptions

File name: `tests/integration/basic/subscriptions/contract-subscriptions.test.ts`

#### Contract Action Subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a subscription to contract action updates without parameters, should stream contract actions from the latest available block | 883 ms |
| ✓ | a subscription to contract action updates without parameters, should stream contract actions adhering to the expected schema | 779 ms |
| ✓ | a subscription to contract action updates with block hash offset, should stream historical and new contract actions from a specific block hash | 793 ms |

### Dust generations subscriptions

File name: `tests/integration/basic/subscriptions/dust-generations-subscriptions.test.ts`

#### Dust Generations Subscription

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | streaming dust generation entries, should stream dust generation events for a valid dust address in bech32m format | 2016 ms |
| ✓ | fresh subscription dtime delivery (#1167), fresh startIndex=0 subscription delivers DustGenerationDtimeUpdateItem for a wallet with spent NIGHT | 6573 ms |
| ✓ | fresh subscription dtime delivery (#1167), resumption startIndex>0 delivers only in-range items and completes correctly | 6455 ms |
| ✓ | subscription error handling, should return an error for an invalid dust address | 720 ms |
| ✓ | subscription error handling, should return an error for a valid address that is meant for another networkid | 995 ms |
| ✓ | subscription error handling, should return an error for a valid dust address passed in hex format | 700 ms |
| ✓ | transactionHash on dust generation events (#1114), first item transactionHash resolves via transactions(offset) | 1610 ms |

### Dust ledger subscriptions

File name: `tests/integration/basic/subscriptions/dust-ledger-subscriptions.test.ts`

#### Dust Ledger Event Subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a subscription to dust ledger events without offset (default replay), streams events in ledger order | 910 ms |
| ✓ | subscription with explicit offset, streams events in ledger order starting from the specified ID | 942 ms |
| ✓ | subscription with explicit offset, validates historical dust events against schema | 1044 ms |
| ✓ | subscription error handling, should return an error for unknown field | 714 ms |
| ✓ | subscription error handling, rejects negative offset ID with an error | 699 ms |

### Dust nullifier subscriptions

File name: `tests/integration/basic/subscriptions/dust-nullifier-subscriptions.test.ts`

#### Dust Nullifier Transactions Subscription

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | streaming dust nullifier transactions with block range, should stream transactions within a block range and complete | 932 ms |
| ✓ | subscription error handling, should return an error for empty nullifier prefixes | 686 ms |
| ✓ | subscription error handling, should return an error when fromBlock is greater than toBlock | 677 ms |
| ✓ | transactionHash on dust nullifier events (#1114), first event transactionHash resolves via transactions(offset) | 1101 ms |
| ✓ | transaction reference on dust nullifier events (#1115), first event transaction.hash matches transactionHash | 988 ms |

### Shielded nullifier subscriptions

File name: `tests/integration/basic/subscriptions/shielded-nullifier-subscriptions.test.ts`

#### Shielded Nullifier Transactions Subscription

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | streaming shielded nullifier transactions with block range, should stream transactions within a block range and complete | 939 ms |
| ✓ | subscription error handling, should return an error for empty nullifier prefixes | 692 ms |
| ✓ | subscription error handling, should return an error for an empty-string nullifier prefix element | 687 ms |
| ✓ | subscription error handling, should return an error when fromBlock is greater than toBlock | 695 ms |
| ✓ | transactionHash on shielded nullifier events (#1114), first event transactionHash resolves via transactions(offset) | 1162 ms |
| ✓ | transaction reference on shielded nullifier events (#1115), first event transaction.hash matches transactionHash | 950 ms |

### Shielded transaction subscriptions

File name: `tests/integration/basic/subscriptions/shielded-transaction-subscriptions.test.ts`

#### Shielded Transaction Subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | opening a session with viewing key, should return a session ID, given a valid viewing key | 782 ms |
| ✓ | opening a session with viewing key, should return an error, given an unsupported hex format viewing key | 692 ms |
| ✓ | opening a session with viewing key, should return an error, given an invalid viewing key | 696 ms |
| ✓ | opening a session with viewing key, should return an error, given a valid viewing key meant for a different network | 1249 ms |
| ✓ | closing a session with session ID, should terminate the session successfully, given a valid session ID | 850 ms |
| ✓ | closing a session with session ID, should return an error, given an invalid session ID | 687 ms |
| ✓ | a subscription to wallet updates providing viewing key only, should stream wallet events starting from the beginning, given there are relevant transactions | 2766 ms |
| ✓ | a subscription to wallet updates providing viewing key only, should stream shielded transaction events adhering to the expected schema | 3801 ms |
| ✓ | a subscription to wallet updates providing viewing key only, should be able to use highestZswapEndIndex from progress event in collapsed update query | 1218 ms |
| ✓ | a subscription to wallet updates providing viewing key only, should reject shieldedTransactions subscription when using expired session ID | 986 ms |

### Subscription quotas

File name: `tests/integration/basic/subscriptions/subscription-quotas.test.ts`

#### Subscription Quotas

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | per-connection concurrent subscription cap, should reject a subscription beyond the per-connection cap | 1717 ms |
| ✓ | per-connection concurrent subscription cap, should free a slot when an active subscription is closed | 2716 ms |
| ✓ | per-session subscription creation rate limit, should reject shielded subscription creations beyond the per-session rate limit | 1831 ms |
| ✓ | per-session subscription creation rate limit, should allow a new shielded subscription after the rate limit refills | 9875 ms |
| ✓ | per-session subscription creation rate limit, should keep a live shielded subscription streaming while creations are rate-limited | 27379 ms |

### Unshielded transaction subscriptions

File name: `tests/integration/basic/subscriptions/unshielded-transaction-subscriptions.test.ts`

#### Unshielded Transaction Subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a subscription to unshielded transaction events by address, should stream unshielded transaction events related to that address, given that address has transactions | 225 ms |
| ✓ | a subscription to unshielded transaction events by address, should stream unshielded transaction events up to highest transaction id | 8602 ms |
| ✓ | a subscription to unshielded transaction events by address, should stream unshielded transaction events that adhere to the expected schema | 159 ms |
| ✓ | a subscription to unshielded transaction events by address, should only return a transaction progress message with highest transaction = 0, given that address does not have transactions | 5065 ms |
| ✓ | a subscription to unshielded transaction events by address, should return an error message, given the address provided is in hex format | 113 ms |
| ✓ | a subscription to unshielded transaction events by address, should return an error message, given the address provided is for another network | 592 ms |
| ✓ | a subscription to unshielded transaction events by address and transaction id, should return a stream of transactions containing that address, starting from transaction id = 0 | 8845 ms |
| ✓ | a subscription to unshielded transaction events by address and transaction id, should return an error message, given the transaction id provided is negative | 137 ms |
| ✓ | a subscription to unshielded transaction events by address and transaction id, should only return a transaction progress message without streaming transactions, given that the transaction id provided is bigger number | 5057 ms |
| ✓ | a subscription to unshielded transaction events by address and transaction id, should start a transaction stream from the given transaction id | 5270 ms |
| ✓ | a subscription to unshielded transaction events by address and transaction id, should return an error message, given the address is provided in hex format | 108 ms |

### Wallet connect options

File name: `tests/integration/basic/subscriptions/wallet-connect-options.test.ts`

#### Wallet Connect Options (StartIndex)

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | opening a session with startIndex, should accept startIndex = 0 (no-op equivalent of unset options) | 792 ms |
| ✓ | opening a session with startIndex, should skip historical scan when startIndex equals the current tip | 1592 ms |
| ✓ | opening a session with startIndex, should accept startIndex past the current tip (fast-forward) | 1608 ms |

### Websocket keepalive

File name: `tests/integration/basic/subscriptions/websocket-keepalive.test.ts`

#### Websocket Keepalive

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | an idle socket without the client keepalive, should be closed while idle past the idle limit | 152606 ms |
| ✓ | an idle socket with the client keepalive, should stay open while idle past the idle limit | 150646 ms |

### Zswap events subscriptions

File name: `tests/integration/basic/subscriptions/zswap-events-subscriptions.test.ts`

#### Zswap Ledger Event Subscriptions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a subscription to zswap ledger events without offset (default replay), streams events in ledger order | 891 ms |
| ✓ | subscription with explicit offset, streams events starting from the specified ID | 1064 ms |
| ✓ | subscription with explicit offset, validates historical zswap events against schema | 892 ms |
| ✓ | subscription error handling, should return an error for unknown field | 708 ms |
| ✓ | subscription error handling, rejects negative offset ID with an error | 707 ms |

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
| ✓ | a transaction to deploy a counter smart contract, should be reported by the indexer through a transaction query by hash | 75 ms |
| ✓ | a transaction to deploy a counter smart contract, should be reported by the indexer through a block query by hash | 95 ms |
| ✓ | a transaction to deploy a counter smart contract, should be reported by the indexer through a contract action query by address | 77 ms |
| ✓ | a transaction to call the store circuit on the deployed counter smart contract, should be reported by the indexer through a transaction query by hash | 210 ms |
| ✓ | a transaction to call the store circuit on the deployed counter smart contract, should be reported by the indexer through a block query by hash | 78 ms |
| ✓ | a transaction to call the store circuit on the deployed counter smart contract, should be reported by the indexer through a contract action query by address | 272 ms |
| ✓ | a transaction to update the counter smart contract, should be reported by the indexer through a transaction query by hash | 76 ms |
| ✓ | a transaction to update the counter smart contract, should be reported by the indexer through a block query by hash | 148 ms |
| ✓ | a transaction to update the counter smart contract, should be reported by the indexer through a contract action query by address | 147 ms |

### Custom unshielded token transactions

File name: `tests/e2e/custom-unshielded-token-transactions.test.ts`

#### Unshielded Custom Token Transactions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a successful unshielded transaction transferring 1 token between two addresses, should be reported by the indexer through a block query by hash | 88 ms |
| ✓ | a successful unshielded transaction transferring 1 token between two addresses, should be reported by the indexer through a transaction query by hash | 79 ms |
| ✓ | a successful unshielded transaction transferring 1 token between two addresses, should be reported by the indexer through an unshielded transaction event for the source address | 2 ms |
| ✓ | a successful unshielded transaction transferring 1 token between two addresses, should be reported by the indexer through an unshielded transaction event for the destination address | 2 ms |
| ✓ | a successful unshielded transaction transferring 1 token between two addresses, should have transferred 1 token from the source to the destination address | 85 ms |
| ✓ | a successful unshielded transaction transferring 1 token between two addresses, should report the token type under test on every created and spent output | 86 ms |
| ✓ | a successful unshielded transaction transferring 1 token between two addresses, should be reported by the indexer through a progress update event for the source address | 25017 ms |
| ✓ | a successful unshielded transaction transferring 1 token between two addresses, should be reported by the indexer through a progress update event for the destination address | 1 ms |

### Night transactions

File name: `tests/e2e/night-transactions.test.ts`

#### Unshielded NIGHT Transactions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses, should be reported by the indexer through a block query by hash | 91 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses, should be reported by the indexer through a transaction query by hash | 91 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses, should be reported by the indexer through an unshielded transaction event for the source address | 1 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses, should be reported by the indexer through an unshielded transaction event for the destination address | 2 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses, should have transferred 1 STAR from the source to the destination address | 101 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses, should report the token type under test on every created and spent output | 78 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses, should be reported by the indexer through a progress update event for the source address | 25013 ms |
| ✓ | a successful unshielded transaction transferring 1 STAR between two addresses, should be reported by the indexer through a progress update event for the destination address | 3 ms |
| ✓ | the genesis block of a chain minted with NIGHT alone, should report the all-zeros NIGHT token type on its outputs | 1 ms |
| ✓ | the dust activity of a confirmed NIGHT transaction, should increase the dust commitment Merkle tree end index | 277 ms |
| ✓ | a confirmed unshielded transfer streamed to address subscriptions, should emit UnshieldedTransaction only for the target wallet (A, B1) | 45992 ms |
| ✓ | a confirmed unshielded transfer streamed to address subscriptions, should emit UnshieldedTransaction only for the target wallet (A, B2) | 20013 ms |
| ✓ | an address with no transaction history, should emit only ProgressUpdate for empty wallet | 2674 ms |

### Segment probe contract actions

File name: `tests/e2e/segment-probe-contract-actions.test.ts`

#### Segment-split Contract Actions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a transaction to deploy a segment-split smart contract, should be reported by the indexer through a transaction query by hash | 80 ms |
| ✓ | a transaction to deploy a segment-split smart contract, should be reported by the indexer through a block query by hash | 99 ms |
| ✓ | a transaction to deploy a segment-split smart contract, should be reported by the indexer through a contract action query by address | 81 ms |
| ✓ | a transaction to call the burnWithGuaranteed circuit on the deployed segment-split smart contract, should be reported by the indexer through a transaction query by hash | 76 ms |
| ✓ | a transaction to call the burnWithGuaranteed circuit on the deployed segment-split smart contract, should be reported by the indexer through a block query by hash | 104 ms |
| ✓ | a transaction to call the burnWithGuaranteed circuit on the deployed segment-split smart contract, should be reported by the indexer through a contract action query by address | 73 ms |

### Shielded transactions

File name: `tests/e2e/shielded-transactions.test.ts`

#### Shielded Transactions

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a successful shielded transaction transferring 1 Shielded Token between two wallets, should be reported by the indexer through a block query by hash | 140 ms |
| ✓ | a successful shielded transaction transferring 1 Shielded Token between two wallets, should be reported by the indexer through a transaction query by hash | 95 ms |
| ✓ | a successful shielded transaction transferring 1 Shielded Token between two wallets, should stream Zswap events followed by DustSpendProcessed after a shielded transaction | 841 ms |
| ✓ | a successful shielded transaction transferring 1 Shielded Token between two wallets, should increase the zswap Merkle tree end index | 283 ms |
| ✓ | a successful shielded transaction transferring 1 Shielded Token between two wallets, should increase the dust commitment Merkle tree end index | 82 ms |
| ✓ | a confirmed shielded transfer streamed to wallet sessions by viewing key, should stream the transaction to the source viewing key with a matching hash | 895 ms |
| ✓ | a confirmed shielded transfer streamed to wallet sessions by viewing key, should stream the transaction to the destination viewing key with a matching hash | 906 ms |
| ✓ | a confirmed shielded transfer streamed to wallet sessions by viewing key, should not stream the transaction to an unrelated viewing key | 20802 ms |

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

#### Unshielded Balance Indexing (Deploy + Mint + Verify) [#1253 Scaffold]

| Status | Name | Duration |
| :---: | --- | ---: |
| ↓ | reports the contract remainder after mint-to-self then partial send | — |

### Toolkit: Dust balance

File name: `tests/e2e/toolkit/dust-balance.test.ts`

#### Dust Balance Query Using Moth

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a dust balance query with a valid wallet seed, should respond with a dust balance according to the requested schema | 30179 ms |

### Toolkit: Key material

File name: `tests/e2e/toolkit/key-material.test.ts`

#### Key Material Derivation Validation

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | a midnight shielded address, should show with the expected prefix for the current network ID | 64 ms |
| ✓ | a midnight shielded address, should show with the expected prefix for all network IDs | 363 ms |
| ✓ | a midnight unshielded address, should show with the expected prefix for the current network ID | 53 ms |
| ✓ | a midnight unshielded address, should show with the expected prefix for all network IDs | 367 ms |
| ✓ | a midnight viewing key, should show with the expected prefix for the current network ID | 43 ms |
| ✓ | a midnight viewing key, should show with the expected prefix for all network IDs | 332 ms |

### Toolkit: Show wallet queries

File name: `tests/e2e/toolkit/show-wallet-queries.test.ts`

#### Show Wallet Queries Using Moth

| Status | Name | Duration |
| :---: | --- | ---: |
| ✓ | private wallet state query using moth, should respond with a private wallet state according to the requested schema | 28952 ms |
| ✓ | public wallet state query using moth, should respond with a public wallet state according to the requested schema | 92 ms |
