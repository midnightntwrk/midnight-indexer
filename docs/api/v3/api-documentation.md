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

- **Queries**: Fetch blocks, transactions, contract actions, and DUST generation status.
  Examples:
    - Retrieve the latest block or a specific block by hash or height.
    - Look up transactions by their hash or identifier.
    - Inspect the current state of a contract action at a given block or transaction offset.
    - Query unshielded token balances held by contracts.
    - Query DUST generation status for Cardano stake keys.

- **Mutations**: Manage wallet sessions.
    - `connect(viewingKey: ViewingKey!)`: Creates a session associated with a viewing key.
    - `disconnect(sessionId: HexEncoded!)`: Ends a previously established session.

- **Subscriptions**: Receive real-time updates.
    - `blocks`: Stream newly indexed blocks.
    - `contractActions(address, offset)`: Stream contract actions.
    - `shieldedTransactions(sessionId, ...)`: Stream shielded transaction updates, including relevant transactions and optional progress updates.
    - `unshieldedTransactions(address)`: Stream unshielded transaction events for a specific address.
    - `dustLedgerEvents(id)`: Stream DUST ledger events.
    - `zswapLedgerEvents(id)`: Stream Zswap ledger events.

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

- `HexEncoded`: Hex-encoded bytes (for hashes, addresses, session IDs).
- `ViewingKey`: A viewing key in hex or Bech32 format for wallet sessions.
- `Unit`: An empty return type for mutations that do not return data.
- `UnshieldedAddress`: An unshielded address in Bech32m format (e.g., `mn_addr_test1...`). Used for unshielded token operations.

## Input Types

### BlockOffset (oneOf)
Used to specify a block by either hash or height:
- `hash`: HexEncoded - The block hash
- `height`: Int - The block height

### TransactionOffset (oneOf)
Used to specify a transaction by either hash or identifier:
- `hash`: HexEncoded - The transaction hash
- `identifier`: HexEncoded - The transaction identifier

### ContractActionOffset (oneOf)
Used to specify a contract action location:
- `blockOffset`: BlockOffset - Query by block (hash or height)
- `transactionOffset`: TransactionOffset - Query by transaction (hash or identifier)

## Example Queries and Mutations

**Note:** These are examples only. Refer to the schema file to confirm exact field names and structures.

### block(offset: BlockOffset): Block

Query a block by offset. If no offset is provided, the latest block is returned.

**Example:**

Query by height:

```graphql
query {
  block(offset: { height: 3 }) {
    hash
    height
    protocolVersion
    timestamp
    author
    parent {
      hash
    }
    transactions {
      id
      hash
      transactionResult {
        status
        segments {
          id
          success
        }
      }
    }
  }
}
```

### transactions(offset: TransactionOffset!): [Transaction!]!

Fetch transactions by hash or by identifier. Returns an array of transactions matching the criteria.

**Note:** The `fees` field is now available on transactions, providing both `paidFees` and `estimatedFees` information.

**Example (by hash):**

```graphql
query {
  transactions(offset: { hash: "3031323..." }) {
    id
    hash
    protocolVersion
    merkleTreeRoot
    block {
      height
      hash
    }
    identifiers
    raw
    contractActions {
      __typename
      ... on ContractDeploy {
        address
        state
        zswapState
        unshieldedBalances {
          tokenType
          amount
        }
      }
      ... on ContractCall {
        address
        state
        entryPoint
        zswapState
        unshieldedBalances {
          tokenType
          amount
        }
      }
      ... on ContractUpdate {
        address
        state
        zswapState
        unshieldedBalances {
          tokenType
          amount
        }
      }
    }
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
    unshieldedCreatedOutputs {
      owner
      value
      tokenType
      intentHash
      outputIndex
    }
    unshieldedSpentOutputs {
      owner
      value
      tokenType
      intentHash
      outputIndex
    }
  }
}
```

**Example (by identifier):**
```graphql
query {
  transactions(offset: { identifier: "abc123..." }) {
    id
    hash
    unshieldedCreatedOutputs {
      owner
      value
      tokenType
    }
    unshieldedSpentOutputs {
      owner
      value
      tokenType
    }
  }
}
```


### contractAction(address: HexEncoded!, offset: ContractActionOffset): ContractAction

Retrieve the latest known contract action at a given offset (by block or transaction). If no offset is provided, returns the latest state.

**Example (latest):**

```graphql
query {
  contractAction(address: "3031323...") {
    __typename
    ... on ContractDeploy {
      address
      state
      zswapState
      unshieldedBalances {
        tokenType
        amount
      }
    }
    ... on ContractCall {
      address
      state
      zswapState
      entryPoint
      unshieldedBalances {
        tokenType
        amount
      }
    }
    ... on ContractUpdate {
      address
      state
      zswapState
      unshieldedBalances {
        tokenType
        amount
      }
    }
  }
}
```

**Example (by block height):**

```graphql
query {
  contractAction(
    address: "3031323...",
    offset: { blockOffset: { height: 10 } }
  ) {
    __typename
    ... on ContractDeploy {
      address
      state
      zswapState
      unshieldedBalances {
        tokenType
        amount
      }
    }
    ... on ContractCall {
      address
      state
      zswapState
      entryPoint
      unshieldedBalances {
        tokenType
        amount
      }
    }
    ... on ContractUpdate {
      address
      state
      zswapState
      unshieldedBalances {
        tokenType
        amount
      }
    }
  }
}
```

### dustGenerationStatus(cardanoStakeKeys: [HexEncoded!]!): [DustGenerationStatus!]!

Query DUST generation status for one or more Cardano stake keys.

**Example:**

```graphql
query {
  dustGenerationStatus(
    cardanoStakeKeys: [
      "0xae78b8d48d620fdf78e30ddb79c442066bd93f1f4f1919efc4373e6fed6cc665"
    ]
  ) {
    cardanoStakeKey
    dustAddress
    registered
    nightBalance
    generationRate
    currentCapacity
  }
}
```

**DUST Generation Parameters:**
- Generation rate: 8,267 Specks per Star per second
- Maximum capacity: 5 DUST per NIGHT
- The `registered` field indicates if stake key is registered via NativeTokenObservation pallet
- Registration data comes from Cardano mainnet via bridge

## Contract Action Types

All ContractAction types (ContractDeploy, ContractCall, ContractUpdate) implement the ContractAction interface with these common fields:
- `address`: The contract address (HexEncoded)
- `state`: The contract state (HexEncoded)
- `zswapState`: The contract-specific zswap state at this action (HexEncoded)
- `transaction`: The transaction that contains this action

Contract actions can be one of three types:
- **ContractDeploy**: Initial contract deployment
- **ContractCall**: Invocation of a contract's entry point
- **ContractUpdate**: State update to an existing contract

Each type implements the ContractAction interface but may have additional fields. For example, ContractCall includes an `entryPoint` field and a reference to its associated `deploy`.

All contract action types include an `unshieldedBalances` field that returns the token balances held by the contract:

- **ContractDeploy**: Always returns empty balances (contracts are deployed with zero balance).
- **ContractCall**: Returns balances after the call execution (may be modified by `unshielded_inputs`/`unshielded_outputs`).
- **ContractUpdate**: Returns balances after the maintenance update.

#### ContractBalance Type

```graphql
type ContractBalance {
  tokenType: HexEncoded!  # Token type identifier
  amount: String!         # Balance amount (supports u128 values)
}
```


## Block Type

The Block type represents a blockchain block:
- `hash`: The block hash (HexEncoded)
- `height`: The block height (Int!)
- `protocolVersion`: The protocol version (Int!)
- `timestamp`: The UNIX timestamp (Int!)
- `author`: The block author (HexEncoded, optional)
- `parent`: Reference to the parent block (Block, optional)
- `transactions`: Array of transactions within this block ([Transaction!]!)

## Transaction Type

The Transaction type represents a blockchain transaction with its associated data:
- `id`: The transaction ID (Int!)
- `hash`: The transaction hash (HexEncoded)
- `protocolVersion`: The protocol version (Int!)
- `transactionResult`: The result of applying the transaction to the ledger state
- `fees`: Fee information including both paid and estimated fees
- `identifiers`: Transaction identifiers array ([HexEncoded!]!)
- `raw`: The raw transaction content (HexEncoded)
- `merkleTreeRoot`: The merkle-tree root (HexEncoded)
- `block`: Reference to the block containing this transaction
- `contractActions`: Array of contract actions within this transaction
- `unshieldedCreatedOutputs`: UTXOs created by this transaction
- `unshieldedSpentOutputs`: UTXOs spent by this transaction

### TransactionResult Type

The result of applying a transaction to the ledger state:
- `status`: TransactionResultStatus (SUCCESS, PARTIAL_SUCCESS, or FAILURE)
- `segments`: Optional array of segment results for partial success cases

### TransactionFees Type

Fee information for a transaction:
- `paidFees`: The actual fees paid for this transaction in DUST (String)
- `estimatedFees`: The estimated fees that was calculated for this transaction in DUST (String)

## Unshielded Token Types

### UnshieldedUtxo

Represents an unshielded UTXO (Unspent Transaction Output):
- `owner`: The owner's address in Bech32m format
- `intentHash`: The hash of the intent that created this output (HexEncoded)
- `value`: The UTXO value as a string (to support u128)
- `tokenType`: The token type identifier (HexEncoded)
- `outputIndex`: The index of this output within its creating transaction
- `createdAtTransaction`: Reference to the transaction that created this UTXO
- `spentAtTransaction`: Reference to the transaction that spent this UTXO (null if unspent)

## DUST Generation Types

### DustGenerationStatus

DUST generation status for a Cardano stake key:
- `cardanoStakeKey`: The Cardano stake key (HexEncoded)
- `dustAddress`: Associated DUST address if registered (HexEncoded, optional)
- `registered`: Whether this stake key is registered (Boolean!)
- `nightBalance`: NIGHT balance backing generation (String)
- `generationRate`: Generation rate in Specks per second (String)
- `currentCapacity`: Current DUST capacity (String)

### DustLedgerEvent

DUST ledger events include:
- `DustInitialUtxo`: Initial DUST UTXO creation event
- `DustGenerationDtimeUpdate`: DUST generation decay time update
- `DustSpendProcessed`: DUST spend processing event
- `ParamChange`: DUST parameter change event

All DUST ledger event types share common fields:
- `id`: Event ID (Int!)
- `raw`: Raw event data (HexEncoded)
- `maxId`: Maximum ID of all DUST events (Int!)

## Mutations

Mutations allow the client to connect a wallet (establishing a session) and disconnect it.

### connect(viewingKey: ViewingKey!): HexEncoded!

Establishes a session for a given wallet viewing key in **either** bech32m or hex format. Returns the session ID.

**Viewing Key Format Support**
- **Bech32m** (preferred): A base-32 encoded format with a human-readable prefix, e.g., `mn_shield-esk_dev1...`
- **Hex** (fallback): A hex-encoded string representing the key bytes.

**Example:**

```graphql
mutation {
  # Provide the bech32m format:
  connect(viewingKey: "mn_shield-esk1abcdef...")
}
```

**Response:**
```json
{
  "data": {
    "connect": "sessionIdHere"
  }
}
```

Use this `sessionId` for shielded transactions subscriptions.

### disconnect(sessionId: HexEncoded!): Unit!

Ends an existing session.

**Example:**

When done:
```graphql
mutation {
  disconnect(sessionId: "sessionIdHere")
}
```

## Subscriptions: Real-time Updates

Subscriptions use a WebSocket connection following the [GraphQL over WebSocket](https://github.com/enisdenjo/graphql-ws/blob/master/PROTOCOL.md) protocol. After connecting and sending a `connection_init` message, the client can start subscription operations.

### Blocks Subscription

`blocks(offset: BlockOffset): Block!`

Subscribe to new blocks. The `offset` parameter lets you start receiving from a given block (by height or hash). If omitted, starts from the latest block.

**Example:**

```json
{
  "id": "1",
  "type": "start",
  "payload": {
    "query": "subscription { blocks(offset: { height: 10 }) { hash height protocolVersion timestamp author parent { hash } transactions { id hash } } }"
  }
}
```

When a new block is indexed, the client receives a `next` message.

### Contract Actions Subscription

`contractActions(address: HexEncoded!, offset: BlockOffset): ContractAction!`

Subscribes to contract actions for a particular address. New contract actions (calls, updates) are pushed as they occur.

**Example:**

```json
{
  "id": "2",
  "type": "start",
  "payload": {
    "query": "subscription { contractActions(address:\"3031323...\", offset: { height: 1 }) { __typename ... on ContractDeploy { address state zswapState unshieldedBalances { tokenType amount } } ... on ContractCall { address state zswapState entryPoint unshieldedBalances { tokenType amount } } ... on ContractUpdate { address state zswapState unshieldedBalances { tokenType amount } } } }"
  }
}
```

### Shielded Transactions Subscription

`shieldedTransactions(sessionId: HexEncoded!, index: Int, sendProgressUpdates: Boolean): ShieldedTransactionsEvent!`

Subscribes to shielded transaction updates. This includes relevant transactions and possibly Merkle tree updates, as well as `ShieldedTransactionsProgress` events if `sendProgressUpdates` is set to `true`, which is also the default. The `index` parameter can be used to resume from a certain point.

Adjust `index` and `offset` arguments as needed.

**Example:**

```json
{
  "id": "3",
  "type": "start",
  "payload": {
    "query": "subscription { shieldedTransactions(sessionId: \"1CYq6ZsLmn\", index: 100) { __typename ... on ViewingUpdate { index update { __typename ... on MerkleTreeCollapsedUpdate { start end update protocolVersion } ... on RelevantTransaction { start end transaction { id hash } } } } ... on ShieldedTransactionsProgress { highestIndex highestRelevantIndex highestRelevantWalletIndex } } }"
  }
}
```

**Event Types:**

**ShieldedTransactionsEvent** (union type):
- `ViewingUpdate`: Contains relevant transactions and/or collapsed Merkle tree updates
  - `index`: Next start index into the zswap state (Int!)
  - `update`: Array of ZswapChainStateUpdate items ([ZswapChainStateUpdate!]!)
    - `MerkleTreeCollapsedUpdate`: Merkle tree update
      - `start`: Start index (Int!)
      - `end`: End index (Int!)
      - `update`: Hex-encoded merkle-tree collapsed update (HexEncoded)
      - `protocolVersion`: Protocol version (Int!)
    - `RelevantTransaction`: Transaction relevant to the wallet
      - `start`: Start index (Int!)
      - `end`: End index (Int!)
      - `transaction`: The relevant transaction (Transaction!)
- `ShieldedTransactionsProgress`: Synchronization progress information
  - `highestIndex`: The highest end index of all currently known transactions (Int!)
  - `highestRelevantIndex`: The highest end index of all currently known relevant transactions (Int!)
  - `highestRelevantWalletIndex`: The highest end index for this particular wallet (Int!)

### Unshielded Transactions Subscription

`unshieldedTransactions(address: UnshieldedAddress!, transactionId: Int): UnshieldedTransactionsEvent!`

Subscribes to unshielded transaction events for a specific address. Emits events whenever transactions involve unshielded UTXOs for the given address.

**Parameters:**
- `address`: The unshielded address to monitor (must be in Bech32m format).
- `transactionId`: Optional. The transaction ID to start from (defaults to 0).

**Example:**

```json
{
  "id": "4",
  "type": "start",
  "payload": {
    "query": "subscription { unshieldedTransactions(address: \"mn_addr_test1...\") { __typename ... on UnshieldedTransaction { transaction { hash block { height } } createdUtxos { owner value tokenType intentHash outputIndex } spentUtxos { owner value tokenType intentHash outputIndex } } ... on UnshieldedTransactionsProgress { highestTransactionId } } }"
  }
}
```

**Event Types:**

- **UnshieldedTransaction**: When UTXOs are created or spent, includes transaction details and affected UTXOs
- **UnshieldedTransactionsProgress**: Periodic synchronization progress updates

**UnshieldedTransactionsEvent**

Event payload for the unshielded transaction subscription:
- `UnshieldedTransaction`: Contains transaction details and UTXOs created/spent
  - `transaction`: The transaction that created and/or spent UTXOs
  - `createdUtxos`: UTXOs created in this transaction for the subscribed address
  - `spentUtxos`: UTXOs spent in this transaction for the subscribed address
- `UnshieldedTransactionsProgress`: Progress information
  - `highestTransactionId`: The highest transaction ID of all currently known transactions for the subscribed address

### DUST Ledger Events Subscription

`dustLedgerEvents(id: Int): DustLedgerEvent!`

Subscribe to DUST ledger events. The `id` parameter allows resuming from a specific event.

**Example:**

```json
{
  "id": "5",
  "type": "start",
  "payload": {
    "query": "subscription { dustLedgerEvents { id __typename ... on DustInitialUtxo { output { nonce } } raw maxId } }"
  }
}
```

### Zswap Ledger Events Subscription

`zswapLedgerEvents(id: Int): ZswapLedgerEvent!`

Subscribe to Zswap ledger events. The `id` parameter allows resuming from a specific event.

**Example:**

```json
{
  "id": "6",
  "type": "start",
  "payload": {
    "query": "subscription { zswapLedgerEvents { id raw maxId } }"
  }
}
```

## Query Limits Configuration

The server may apply limitations to queries (e.g. `max-depth`, `max-fields`, `timeout`, and complexity cost). Requests that violate these limits return errors indicating the reason (too many fields, too deep, too costly, or timed out).

**Example error:**

```json
{
  "data": null,
  "errors": [
    {
      "message": "Query has too many fields: 20. Max fields: 10."
    }
  ]
}
```

## Authentication

- Shielded transactions subscription requires a `sessionId` from the `connect` mutation.

## Regenerating the Schema

If you modify the code defining the GraphQL schema, regenerate it:
```bash
just generate-indexer-api-schema
```
This ensures the schema file stays aligned with code changes.

## Migration from v1

If migrating from API v1:
1. Update endpoint URLs from `/v1/graphql` to `/v3/graphql` (though v1 redirects automatically)
2. Review field name changes (e.g., `chainState` → `zswapState` in contract actions)
3. Test thoroughly as some response structures may have evolved

## Related Documentation

- [DUST Generation Status Details](../../interactions/dust-generation-status/dust-generation-status-api-documentation.md)
- [QA Testing Guide](../../interactions/dust-generation-status/dust-generation-status-qa-testing-guide.md)

## Conclusion

This document offers a few hand-picked examples and an overview of available operations. For the most accurate and comprehensive reference, consult the schema file. As the API evolves, remember to validate these examples against the schema and update them as needed.
