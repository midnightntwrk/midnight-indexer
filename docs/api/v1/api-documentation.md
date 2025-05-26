# Midnight Indexer API Documentation v1

The Midnight Indexer API exposes a GraphQL API that enables clients to query and subscribe to blockchain data—blocks, transactions, contracts, and wallet-related events—indexed from the Midnight blockchain. These capabilities facilitate both historical lookups and real-time monitoring.

**Disclaimer:**  
The examples provided here are illustrative and may need updating if the API changes. Always consider [`indexer-api/graphql/schema-v1.graphql`](../../../indexer-api/graphql/schema-v1.graphql) as the primary source of truth. Adjust queries as necessary to match the latest schema.

## GraphQL Schema

The GraphQL schema is defined in [`indexer-api/graphql/schema-v1.graphql`](../../../indexer-api/graphql/schema-v1.graphql). It specifies all queries, mutations, subscriptions, and their types, including arguments and return structures.

## Overview of Operations

- **Queries**: Fetch blocks, transactions, contract actions, and unshielded tokens.  
  Examples:
    - Retrieve the latest block or a specific block by hash or height.
    - Look up transactions by their hash or identifier.
    - Filter transactions by unshielded address to see token transfers.
    - List all unshielded UTXOs (spent and unspent) for a given address.
    - Inspect the current state of a contract action at a given block or transaction offset.

- **Mutations**: Manage wallet sessions.
    - `connect(viewingKey: ViewingKey!)`: Creates a session associated with a viewing key.
    - `disconnect(sessionId: HexEncoded!)`: Ends a previously established session.

- **Subscriptions**: Receive real-time updates.
    - `blocks`: Stream newly indexed blocks.
    - `contractActions(address, offset)`: Stream contract actions.
    - `wallet(sessionId, ...)`: Stream wallet updates, including relevant transactions and optional progress updates.
    - `unshieldedUtxos(address)`: Stream unshielded UTXO creation and spending events for a specific address.

## API Endpoints

**HTTP (Queries & Mutations):**
```
POST https://<host>:<port>/api/v1/graphql
Content-Type: application/json
```

**WebSocket (Subscriptions):**
```
wss://<host>:<port>/api/v1/graphql/ws
Sec-WebSocket-Protocol: graphql-transport-ws
```

## Core Scalars

- `HexEncoded`: Hex-encoded bytes (for hashes, addresses, session IDs).
- `ViewingKey`: A viewing key in hex or Bech32 format for wallet sessions.
- `ApplyStage`: Enumerated stages of transaction application. This scalar represents the outcome of transaction processing on the chain ("Success", "PartialSuccess" or "Failure").
- `Unit`: An empty return type for mutations that do not return data.
- `UnshieldedAddress`: An unshielded address in Bech32m format (e.g., `mn_addr_test1...`). Used for unshielded token operations.

## Example Queries and Mutations

**Note:** These are examples only. Refer to the schema file to confirm exact field names and structures.

### block(offset: BlockOffset): Block

**Parameters** (BlockOffset is a oneOf):
- `hash: HexEncoded` – The block hash.
- `height: Int` – The block height (number).

If no offset is provided, the latest block is returned.

**Example:**

Query by height:

```graphql
query {
  block(offset: { height: 3 }) {
    hash
    height
    protocolVersion
    timestamp
    parent {
      hash
    }
    transactions {
      hash
      applyStage
    }
  }
}
```

### transactions(offset: TransactionOffset!, address: UnshieldedAddress): [Transaction!]!

Fetch transactions by hash or by identifier using a TransactionOffset object. The offset must include either a hash or an identifier, but not both.

Optionally, you can filter by an unshielded address to retrieve only transactions that create or spend unshielded UTXOs for that address.

**Parameters:**
- `offset`: Required. Either a transaction hash or identifier.
- `address`: Optional. An unshielded address (Bech32m format) to filter transactions.

**Example (by hash):**

```graphql
query {
  transactions(offset: { hash: "3031323..." }) {
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
        chainState
      }
      ... on ContractCall {
        address
        state
        entryPoint
        chainState
      }
      ... on ContractUpdate {
        address
        state
        chainState
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

**Example (filtered by unshielded address):**
```graphql
query {
  transactions(
    offset: { identifier: "abc123..." },
    address: "mn_addr_test1..."
  ) {
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
    address
    state
    chainState
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
    address
    state
    chainState
  }
}
```

## Contract Action Types

All ContractAction types (ContractDeploy, ContractCall, ContractUpdate) implement the ContractAction interface with these common fields:
- `address`: The contract address (HexEncoded)
- `state`: The contract state (HexEncoded)
- `chainState`: The chain state at this action (HexEncoded)
- `transaction`: The transaction that contains this action

Contract actions can be one of three types:
- **ContractDeploy**: Initial contract deployment
- **ContractCall**: Invocation of a contract's entry point
- **ContractUpdate**: State update to an existing contract

Each type implements the ContractAction interface but may have additional fields. For example, ContractCall includes an `entryPoint` field and a reference to its associated `deploy`.

### unshieldedUtxos(address: UnshieldedAddress!, offset: UnshieldedOffset): [UnshieldedUtxo!]!

Retrieve all unshielded UTXOs (both spent and unspent) associated with a given address.

**Parameters:**
- `address`: Required. The unshielded address in Bech32m format.
- `offset`: Optional. Either a BlockOffset or TransactionOffset to query from a specific point.

**Example (all UTXOs for an address):**

```graphql
query {
  unshieldedUtxos(address: "mn_addr_test1...") {
    owner
    intentHash
    value
    tokenType
    outputIndex
    createdAtTransaction {
      hash
      block {
        height
      }
    }
    spentAtTransaction {
      hash
    }
  }
}
```

**Example (with block offset):**

```graphql
query {
  unshieldedUtxos(
    address: "mn_addr_test1...",
    offset: { blockOffset: { height: 100 } }
  ) {
    value
    tokenType
    spentAtTransaction {
      hash
    }
  }
}
```

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

### UnshieldedOffset

A oneOf input type for querying unshielded UTXOs from a specific point:
- `blockOffset`: Query from a specific block (by hash or height)
- `transactionOffset`: Query from a specific transaction (by hash or identifier)

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

### disconnect(sessionId: HexEncoded!): Unit!

Ends an existing session.

**Example:**

Use this `sessionId` for wallet subscriptions.

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
    "query": "subscription { blocks(offset: { height: 10 }) { hash height timestamp transactions { hash } } }"
  }
}
```

When a new block is indexed, the client receives a `next` message.

### Contracts Subscription

`contractActions(address: HexEncoded!, offset: BlockOffset): ContractAction!`

Subscribes to contract actions for a particular address. New contract actions (calls, updates) are pushed as they occur.

**Example:**

```json
{
  "id": "2",
  "type": "start",
  "payload": {
    "query": "subscription { contractActions(address:\"3031323...\", offset: { height: 1 }) { __typename address state } }"
  }
}
```

### Wallet Subscription

`wallet(sessionId: HexEncoded!, index: Int, sendProgressUpdates: Boolean): WalletSyncEvent!`

Subscribes to wallet updates. This includes relevant transactions and possibly Merkle tree updates, as well as `ProgressUpdate` events if `sendProgressUpdates` is set to `true`, which is also the default. The `index` parameter can be used to resume from a certain point.

Adjust `index` and `offset` arguments as needed.

**Example:**

```json
{
  "id": "3",
  "type": "start",
  "payload": {
    "query": "subscription { wallet(sessionId: \"1CYq6ZsLmn\", index: 100) { __typename ... on ViewingUpdate { index update { __typename ... on RelevantTransaction { transaction { hash } } } } ... on ProgressUpdate { highestIndex highestRelevantIndex highestRelevantWalletIndex } } }"
  }
}
```

**Responses** may vary depending on what is happening in the chain:
- A `ViewingUpdate` with new relevant transactions or a collapsed Merkle tree update.
- A `ProgressUpdate` providing synchronization progress with fields like `highestIndex`, `highestRelevantIndex`, and `highestRelevantWalletIndex`.

### Unshielded UTXOs Subscription

`unshieldedUtxos(address: UnshieldedAddress!): UnshieldedUtxoEvent!`

Subscribes to unshielded UTXO events for a specific address. Emits events whenever unshielded UTXOs are created or spent for the given address.

**Parameters:**
- `address`: The unshielded address to monitor (must be in Bech32m format).

**Example:**

```json
{
  "id": "4",
  "type": "start",
  "payload": {
    "query": "subscription { unshieldedUtxos(address: \"mn_addr_test1...\") { eventType transaction { hash block { height } } createdUtxos { owner value tokenType intentHash outputIndex } spentUtxos { owner value tokenType intentHash outputIndex } } }"
  }
}
```
**Response Types:**

- UPDATE: Indicates a transaction that created or spent UTXOs for the address.
- PROGRESS: Status message for synchronization progress or keep-alive.

**Each event includes:**

- The transaction that triggered the event
- Lists of created and spent UTXOs in that transaction

**UnshieldedUtxoEvent**

Event payload for the unshielded UTXO subscription:
- `eventType`: Either `UPDATE` (for actual changes) or `PROGRESS` (for status messages)
- `transaction`: The transaction associated with this event
- `createdUtxos`: List of UTXOs created in this transaction for the subscribed address
- `spentUtxos`: List of UTXOs spent in this transaction for the subscribed address

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

- Wallet subscription requires a `sessionId` from the `connect` mutation.

### Regenerating the Schema

If you modify the code defining the GraphQL schema, regenerate it:
```bash
just generate-indexer-api-schema
```
This ensures the schema file stays aligned with code changes.

## Conclusion

This document offers a few hand-picked examples and an overview of available operations. For the most accurate and comprehensive reference, consult the schema file. As the API evolves, remember to validate these examples against the schema and update them as needed.
