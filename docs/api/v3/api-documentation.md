# Midnight Indexer API Documentation v3

The Midnight Indexer API exposes a GraphQL API that enables clients to query and subscribe to blockchain data—blocks, transactions, contracts, DUST generation, and shielded/unshielded transaction events—indexed from the Midnight blockchain. These capabilities facilitate both historical lookups and real-time monitoring.

**Version Information:**
- Current API version: v3
- Previous version v1 redirects to v3 automatically
- Version v2 was skipped during migration

**Disclaimer:**
The examples provided here are illustrative and may need updating if the API changes. Always consider [`indexer-api/graphql/schema-v3.graphql`](../../../indexer-api/graphql/schema-v3.graphql) as the primary source of truth. Adjust queries as necessary to match the latest schema.

## GraphQL Schema

The GraphQL schema is defined in [`indexer-api/graphql/schema-v3.graphql`](../../../indexer-api/graphql/schema-v3.graphql). It specifies all queries, mutations, subscriptions, and their types, including arguments and return structures.

## Overview of Operations

### Queries
Fetch blocks, transactions, contract actions, and DUST generation status:
- `block(offset: BlockOffset): Block` - Retrieve the latest block or a specific block by hash or height
- `transactions(offset: TransactionOffset!): [Transaction!]!` - Look up transactions by their hash or identifier
- `contractAction(address: HexEncoded!, offset: ContractActionOffset): ContractAction` - Inspect the current state of a contract action
- `dustGenerationStatus(cardanoStakeKeys: [HexEncoded!]!): [DustGenerationStatus!]!` - Query DUST generation status for Cardano stake keys

### Mutations
Manage wallet sessions:
- `connect(viewingKey: ViewingKey!): HexEncoded!` - Creates a session associated with a viewing key
- `disconnect(sessionId: HexEncoded!): Unit!` - Ends a previously established session

### Subscriptions
Receive real-time updates:
- `blocks(offset: BlockOffset): Block!` - Stream newly indexed blocks
- `contractActions(address: HexEncoded!, offset: BlockOffset): ContractAction!` - Stream contract actions
- `shieldedTransactions(sessionId: HexEncoded!, index: Int): ShieldedTransactionsEvent!` - Stream shielded transaction updates
- `unshieldedTransactions(address: UnshieldedAddress!, transactionId: Int): UnshieldedTransactionsEvent!` - Stream unshielded transaction events
- `dustLedgerEvents(id: Int): DustLedgerEvent!` - Stream DUST ledger events
- `zswapLedgerEvents(id: Int): ZswapLedgerEvent!` - Stream Zswap ledger events

## API Endpoints

**HTTP (Queries & Mutations):**
```
POST https://<host>:<port>/api/v3/graphql
Content-Type: application/json
```

**WebSocket (Subscriptions):**
```
wss://<host>:<port>/api/v3/graphql/ws
Sec-WebSocket-Protocol: graphql-transport-ws
```

## Core Scalars

- `HexEncoded`: Hex-encoded bytes (for hashes, addresses, session IDs)
- `ViewingKey`: A viewing key in hex or Bech32 format for wallet sessions
- `Unit`: An empty return type for mutations that do not return data
- `UnshieldedAddress`: An unshielded address in Bech32m format (e.g., `mn_addr_test1...`)

## Input Types

### BlockOffset (oneOf)
Used to specify a block by either hash or height:
```graphql
input BlockOffset @oneOf {
  hash: HexEncoded    # The block hash
  height: Int          # The block height
}
```

### TransactionOffset (oneOf)
Used to specify a transaction by either hash or identifier:
```graphql
input TransactionOffset @oneOf {
  hash: HexEncoded       # The transaction hash
  identifier: HexEncoded # The transaction identifier
}
```

### ContractActionOffset (oneOf)
Used to specify a contract action location:
```graphql
input ContractActionOffset @oneOf {
  blockOffset: BlockOffset           # Query by block
  transactionOffset: TransactionOffset # Query by transaction
}
```

## Core Types

### Block Type
Represents a blockchain block:
```graphql
type Block {
  hash: HexEncoded!              # The block hash
  height: Int!                   # The block height
  protocolVersion: Int!          # The protocol version
  timestamp: Int!                # The UNIX timestamp
  author: HexEncoded             # The block author (optional)
  ledgerParameters: HexEncoded!  # Ledger parameters for this block
  parent: Block                  # Reference to the parent block
  transactions: [Transaction!]!  # Transactions within this block
}
```

### Transaction Interface
Base interface for all transaction types:
```graphql
interface Transaction {
  id: Int!
  hash: HexEncoded!
  protocolVersion: Int!
  raw: HexEncoded!
  block: Block!
  contractActions: [ContractAction!]!
  unshieldedCreatedOutputs: [UnshieldedUtxo!]!
  unshieldedSpentOutputs: [UnshieldedUtxo!]!
  zswapLedgerEvents: [ZswapLedgerEvent!]!
  dustLedgerEvents: [DustLedgerEvent!]!
}
```

### RegularTransaction Type
A regular Midnight transaction with full details:
```graphql
type RegularTransaction implements Transaction {
  # All fields from Transaction interface plus:
  transactionResult: TransactionResult!
  identifiers: [HexEncoded!]!
  merkleTreeRoot: HexEncoded!
  startIndex: Int!  # Zswap state start index
  endIndex: Int!    # Zswap state end index
  fees: TransactionFees!
}
```

### SystemTransaction Type
A system-generated transaction:
```graphql
type SystemTransaction implements Transaction {
  # Implements all Transaction interface fields
  # No additional fields beyond the interface
}
```

### TransactionResult Type
The result of applying a transaction to the ledger state:
```graphql
type TransactionResult {
  status: TransactionResultStatus!  # SUCCESS, PARTIAL_SUCCESS, or FAILURE
  segments: [Segment!]              # Segment results for partial success
}

type Segment {
  id: Int!
  success: Boolean!
}
```

### TransactionFees Type
Fee information for a transaction:
```graphql
type TransactionFees {
  paidFees: String!      # Actual fees paid in DUST
  estimatedFees: String! # Estimated fees calculated in DUST
}
```

## Contract Action Types

### ContractAction Interface
Base interface for all contract actions:
```graphql
interface ContractAction {
  address: HexEncoded!
  state: HexEncoded!
  chainState: HexEncoded!
  transaction: Transaction!
  unshieldedBalances: [ContractBalance!]!
}
```

### Contract Action Implementations

#### ContractDeploy
Initial contract deployment:
```graphql
type ContractDeploy implements ContractAction {
  # All fields from ContractAction interface
  # Unshielded balances are always empty on deployment
}
```

#### ContractCall
Invocation of a contract's entry point:
```graphql
type ContractCall implements ContractAction {
  # All fields from ContractAction interface plus:
  entryPoint: String!      # The entry point being called
  deploy: ContractDeploy!  # Reference to the contract's deployment
}
```

#### ContractUpdate
State update to an existing contract:
```graphql
type ContractUpdate implements ContractAction {
  # All fields from ContractAction interface
  # Balances reflect state after the update
}
```

### ContractBalance Type
Token balance held by a contract:
```graphql
type ContractBalance {
  tokenType: HexEncoded!  # Token type identifier
  amount: String!         # Balance amount (supports u128 values)
}
```

## Unshielded Token Types

### UnshieldedUtxo
Represents an unshielded UTXO:
```graphql
type UnshieldedUtxo {
  owner: UnshieldedAddress!          # Owner's Bech32m address
  tokenType: HexEncoded!             # Token type identifier
  value: String!                      # UTXO value (supports u128)
  intentHash: HexEncoded!             # Hash of the creating intent
  outputIndex: Int!                   # Index within creating transaction
  ctime: Int                          # Creation time in seconds
  initialNonce: HexEncoded!           # Initial nonce for DUST tracking
  registeredForDustGeneration: Boolean! # DUST generation registration status
  createdAtTransaction: Transaction!  # Creating transaction
  spentAtTransaction: Transaction     # Spending transaction (null if unspent)
}
```

### UnshieldedTransaction
A transaction with unshielded operations:
```graphql
type UnshieldedTransaction {
  transaction: Transaction!       # The transaction
  createdUtxos: [UnshieldedUtxo!]! # UTXOs created
  spentUtxos: [UnshieldedUtxo!]!   # UTXOs spent
}
```

## DUST Generation Types

### DustGenerationStatus
DUST generation status for a Cardano stake key:
```graphql
type DustGenerationStatus {
  cardanoStakeKey: HexEncoded!  # The Cardano stake key
  dustAddress: HexEncoded       # Associated DUST address (if registered)
  registered: Boolean!          # Registration status
  nightBalance: String!         # NIGHT balance backing generation
  generationRate: String!       # Generation rate in Specks per second
  currentCapacity: String!      # Current DUST capacity
}
```

### DustLedgerEvent Interface
Base interface for DUST ledger events:
```graphql
interface DustLedgerEvent {
  id: Int!              # Event ID
  raw: HexEncoded!      # Raw event data
  maxId: Int!           # Maximum ID of all DUST events
}
```

### DUST Event Implementations

#### DustInitialUtxo
Initial DUST UTXO creation:
```graphql
type DustInitialUtxo implements DustLedgerEvent {
  # All fields from DustLedgerEvent interface plus:
  output: DustOutput!  # The DUST output details
}

type DustOutput {
  nonce: HexEncoded!  # 32-byte nonce
}
```

#### DustGenerationDtimeUpdate
DUST generation decay time update:
```graphql
type DustGenerationDtimeUpdate implements DustLedgerEvent {
  # Implements all DustLedgerEvent interface fields
}
```

#### DustSpendProcessed
DUST spend processing event:
```graphql
type DustSpendProcessed implements DustLedgerEvent {
  # Implements all DustLedgerEvent interface fields
}
```

#### ParamChange
DUST parameter change event:
```graphql
type ParamChange implements DustLedgerEvent {
  # Implements all DustLedgerEvent interface fields
}
```

## Zswap Types

### ZswapLedgerEvent
A Zswap-related ledger event:
```graphql
type ZswapLedgerEvent {
  id: Int!           # Event ID
  raw: HexEncoded!   # Raw event data
  maxId: Int!        # Maximum ID of all Zswap events
}
```

## Shielded Transaction Types

### ShieldedTransactionsEvent (Union)
Events for shielded transaction subscriptions:
```graphql
union ShieldedTransactionsEvent = RelevantTransaction | ShieldedTransactionsProgress
```

### RelevantTransaction
A transaction relevant to the subscribing wallet:
```graphql
type RelevantTransaction {
  transaction: RegularTransaction!        # The relevant transaction
  collapsedMerkleTree: CollapsedMerkleTree # Optional merkle tree update
}

type CollapsedMerkleTree {
  startIndex: Int!         # Zswap state start index
  endIndex: Int!           # Zswap state end index
  update: HexEncoded!      # The update data
  protocolVersion: Int!    # Protocol version
}
```

### ShieldedTransactionsProgress
Progress information for shielded transaction indexing:
```graphql
type ShieldedTransactionsProgress {
  highestEndIndex: Int!          # Highest end index of all transactions
  highestCheckedEndIndex: Int!   # Highest checked for relevance
  highestRelevantEndIndex: Int!  # Highest relevant for wallet
}
```

## Unshielded Transaction Events

### UnshieldedTransactionsEvent (Union)
Events for unshielded transaction subscriptions:
```graphql
union UnshieldedTransactionsEvent = UnshieldedTransaction | UnshieldedTransactionsProgress
```

### UnshieldedTransactionsProgress
Progress information for unshielded indexing:
```graphql
type UnshieldedTransactionsProgress {
  highestTransactionId: Int!  # Highest known transaction ID for address
}
```

## Query Examples

### Query Latest Block
```graphql
query {
  block {
    hash
    height
    protocolVersion
    timestamp
    author
    ledgerParameters
    parent {
      hash
      height
    }
    transactions {
      hash
      __typename
      ... on RegularTransaction {
        fees {
          paidFees
          estimatedFees
        }
        transactionResult {
          status
        }
      }
    }
  }
}
```

### Query Block by Height
```graphql
query {
  block(offset: { height: 100 }) {
    hash
    height
    transactions {
      hash
      contractActions {
        __typename
        address
        unshieldedBalances {
          tokenType
          amount
        }
      }
    }
  }
}
```

### Query Transactions by Hash
```graphql
query {
  transactions(offset: { hash: "0x1234..." }) {
    id
    hash
    __typename
    ... on RegularTransaction {
      fees {
        paidFees
        estimatedFees
      }
      transactionResult {
        status
        segments {
          id
          success
        }
      }
    }
    unshieldedCreatedOutputs {
      owner
      value
      tokenType
      registeredForDustGeneration
    }
  }
}
```

### Query Contract Action
```graphql
query {
  contractAction(address: "0xabc123...") {
    __typename
    address
    state
    chainState
    transaction {
      hash
      block {
        height
      }
    }
    unshieldedBalances {
      tokenType
      amount
    }
    ... on ContractCall {
      entryPoint
      deploy {
        address
      }
    }
  }
}
```

### Query DUST Generation Status
```graphql
query {
  dustGenerationStatus(
    cardanoStakeKeys: [
      "0xae78b8d48d620fdf78e30ddb79c442066bd93f1f4f1919efc4373e6fed6cc665"
    ]
  ) {
    cardanoStakeKey
    dustAddress
    registered  # Note: was 'isRegistered' in feature branch
    nightBalance
    generationRate
    currentCapacity
  }
}
```

**DUST Generation Notes:**
- Generation rate: 8,267 Specks per Star per second
- Maximum capacity: 5 DUST per NIGHT
- The `registered` field indicates if stake key is registered via NativeTokenObservation pallet
- Registration data comes from Cardano mainnet via bridge

## Mutation Examples

### Connect Wallet
```graphql
mutation {
  # Using Bech32m viewing key
  connect(viewingKey: "mn_shield-esk1abcdef...")
}
```

Response:
```json
{
  "data": {
    "connect": "0x1234abcd..."  // Session ID
  }
}
```

### Disconnect Wallet
```graphql
mutation {
  disconnect(sessionId: "0x1234abcd...")
}
```

## Subscription Examples

Subscriptions use WebSocket connections following the [GraphQL over WebSocket](https://github.com/enisdenjo/graphql-ws/blob/master/PROTOCOL.md) protocol.

### Subscribe to Blocks
```json
{
  "id": "1",
  "type": "start",
  "payload": {
    "query": "subscription { blocks { hash height timestamp transactions { hash } } }"
  }
}
```

### Subscribe to Contract Actions
```json
{
  "id": "2",
  "type": "start",
  "payload": {
    "query": "subscription { contractActions(address: \"0xabc123...\") { __typename address state ... on ContractCall { entryPoint } } }"
  }
}
```

### Subscribe to Shielded Transactions
```json
{
  "id": "3",
  "type": "start",
  "payload": {
    "query": "subscription { shieldedTransactions(sessionId: \"0x1234...\") { __typename ... on RelevantTransaction { transaction { hash endIndex } } ... on ShieldedTransactionsProgress { highestEndIndex highestRelevantEndIndex } } }"
  }
}
```

### Subscribe to Unshielded Transactions
```json
{
  "id": "4",
  "type": "start",
  "payload": {
    "query": "subscription { unshieldedTransactions(address: \"mn_addr_test1...\") { __typename ... on UnshieldedTransaction { transaction { hash } createdUtxos { value tokenType } spentUtxos { value tokenType } } } }"
  }
}
```

### Subscribe to DUST Ledger Events
```json
{
  "id": "5",
  "type": "start",
  "payload": {
    "query": "subscription { dustLedgerEvents { id __typename ... on DustInitialUtxo { output { nonce } } } }"
  }
}
```

### Subscribe to Zswap Ledger Events
```json
{
  "id": "6",
  "type": "start",
  "payload": {
    "query": "subscription { zswapLedgerEvents { id raw maxId } }"
  }
}
```

## Query Limits and Complexity

The server applies the following limitations to queries:
- **Maximum depth**: Nested query depth limit
- **Maximum fields**: Total number of fields requested
- **Timeout**: Query execution time limit
- **Complexity cost**: Calculated cost based on query structure

Requests violating these limits return errors indicating the reason.

Example error:
```json
{
  "data": null,
  "errors": [
    {
      "message": "Query complexity of 600 exceeds maximum complexity of 550"
    }
  ]
}
```

## Authentication and Sessions

### Shielded Transactions
Shielded transaction subscriptions require a valid session ID obtained from the `connect` mutation.

### Session Management
1. Call `connect` mutation with a viewing key to obtain a session ID
2. Use the session ID for shielded transaction subscriptions
3. Call `disconnect` mutation when finished to clean up the session

## Type System Notes

### Interface Implementations
- **Transaction**: Implemented by `RegularTransaction` and `SystemTransaction`
- **ContractAction**: Implemented by `ContractDeploy`, `ContractCall`, and `ContractUpdate`
- **DustLedgerEvent**: Implemented by `DustInitialUtxo`, `DustGenerationDtimeUpdate`, `DustSpendProcessed`, and `ParamChange`

### Union Types
- **ShieldedTransactionsEvent**: Can be `RelevantTransaction` or `ShieldedTransactionsProgress`
- **UnshieldedTransactionsEvent**: Can be `UnshieldedTransaction` or `UnshieldedTransactionsProgress`

### OneOf Input Types
Input types marked with `@oneOf` directive require exactly one field to be provided:
- **BlockOffset**: Either `hash` OR `height`
- **TransactionOffset**: Either `hash` OR `identifier`
- **ContractActionOffset**: Either `blockOffset` OR `transactionOffset`

## Regenerating the Schema

If you modify the code defining the GraphQL schema, regenerate it:
```bash
just generate-indexer-api-schema
```

This ensures the schema file stays aligned with code changes.

## Common Use Cases

### 1. Monitor New Blocks
```graphql
subscription {
  blocks {
    height
    timestamp
    transactions {
      hash
      __typename
      ... on RegularTransaction {
        transactionResult { status }
        fees { paidFees }
      }
    }
  }
}
```

### 2. Track DUST Generation
```graphql
query TrackDustGeneration($stakeKeys: [HexEncoded!]!) {
  dustGenerationStatus(cardanoStakeKeys: $stakeKeys) {
    cardanoStakeKey
    registered
    nightBalance
    generationRate
    currentCapacity
    dustAddress
  }
}
```

### 3. Monitor Contract State
```graphql
subscription WatchContract($address: HexEncoded!) {
  contractActions(address: $address) {
    __typename
    state
    chainState
    unshieldedBalances {
      tokenType
      amount
    }
    ... on ContractCall {
      entryPoint
    }
  }
}
```

### 4. Full Wallet Sync
```graphql
# Step 1: Connect
mutation Connect($vk: ViewingKey!) {
  connect(viewingKey: $vk)
}

# Step 2: Subscribe to shielded transactions
subscription SyncWallet($sessionId: HexEncoded!) {
  shieldedTransactions(sessionId: $sessionId) {
    __typename
    ... on RelevantTransaction {
      transaction {
        hash
        endIndex
      }
      collapsedMerkleTree {
        update
      }
    }
    ... on ShieldedTransactionsProgress {
      highestEndIndex
      highestRelevantEndIndex
    }
  }
}
```

## Testing and Development

### Local Testing
The API can be tested locally using tools like:
- GraphQL Playground (built-in at `/v3/graphql` in development mode)
- [Insomnia](https://insomnia.rest/) or [Postman](https://www.postman.com/) for HTTP queries
- [wscat](https://github.com/websockets/wscat) for WebSocket subscriptions

### Example Test Queries
See [`indexer-tests/e2e.graphql`](../../../indexer-tests/e2e.graphql) for comprehensive test queries used in the indexer's end-to-end tests.

## Migration from v1

If migrating from API v1:
1. Update endpoint URLs from `/v1/graphql` to `/v3/graphql` (though v1 redirects automatically)
2. Review field name changes (e.g., `isRegistered` → `registered` in DUST types)
3. Test thoroughly as some response structures may have evolved

## Related Documentation

- [DUST Generation Status Details](../../interactions/dust-generation-status/dust-generation-status-api-documentation.md)
- [QA Testing Guide](../../interactions/dust-generation-status/dust-generation-status-qa-testing-guide.md)
- [GraphQL Schema](../../../indexer-api/graphql/schema-v3.graphql)

## Conclusion

This document provides a comprehensive overview of the Midnight Indexer v3 GraphQL API. For the most accurate and up-to-date reference, always consult the schema file at [`indexer-api/graphql/schema-v3.graphql`](../../../indexer-api/graphql/schema-v3.graphql). As the API evolves, validate these examples against the schema and update them as needed.