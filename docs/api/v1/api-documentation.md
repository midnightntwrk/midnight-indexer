# Midnight Indexer API Documentation v1

The Midnight Indexer API exposes a GraphQL interface that enables clients to query and subscribe to blockchain data—blocks, transactions, contracts, and wallet-related events—indexed from the Midnight blockchain. These capabilities facilitate both historical lookups and real-time monitoring.

**Disclaimer:**  
The examples provided here are illustrative and may need updating if the API changes. Always consider [`indexer-api/graphql/schema-v1.graphql`](../../../indexer-api/graphql/schema-v1.graphql) as the primary source of truth. Adjust queries as necessary to match the latest schema.

## GraphQL Schema

The GraphQL schema is defined in [`indexer-api/graphql/schema-v1.graphql`](../../../indexer-api/graphql/schema-v1.graphql). It specifies all queries, mutations, subscriptions, and their types, including arguments and return structures.

## Overview of Operations

- **Queries**: Fetch blocks, transactions, and contract states.  
  Examples:
    - Retrieve the latest block or a specific block by height/hash.
    - Look up transactions by their hash or identifier.
    - Inspect the current state of a contract at a given block or transaction offset.

- **Mutations**: Manage wallet sessions.
    - `connect(viewingKey: ViewingKey!)`: Creates a session associated with a viewing key.
    - `disconnect(sessionId: HexEncoded!)`: Ends a previously established session.

- **Subscriptions**: Receive real-time updates.
    - `blocks`: Stream newly indexed blocks.
    - `contracts(address, offset)`: Stream contract state changes.
    - `wallet(sessionId, ...)`: Stream wallet updates, including relevant transactions and optional progress updates.

## API Endpoints

**HTTP (Queries & Mutations):**
```
POST http://<host>:<port>/api/v1/graphql
Content-Type: application/json
```

**WebSocket (Subscriptions):**
```
ws://<host>:<port>/api/v1/graphql/ws
Sec-WebSocket-Protocol: graphql-transport-ws
```

## Core Scalars

- `HexEncoded`: Hex-encoded bytes (for hashes, addresses, session IDs).
- `ViewingKey`: A viewing key in hex or Bech32 format for wallet sessions.
- `ApplyStage`: Enumerated stages of transaction application (e.g., Success, Failure).
- `Unit`: An empty return type for mutations that do not return data.

## Example Queries and Mutations

**Note:** These are examples only. Refer to the schema file to confirm exact field names and structures.

### block(offset: BlockOffsetInput): Block

**Parameters** (BlockOffsetInput is a oneOf):
- `hash: HexEncoded` – The block hash.
- `height: Int` – The block height (number).

If no offset is provided, the latest block is returned.

**Example:**

Query by height:

```graphql
query {
  block(offset: {height: 3}) {
    hash
    height
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

### transactions(hash: HexEncoded, identifier: HexEncoded): [Transaction!]!

Fetch transactions by hash or by identifier. One of the parameters must be provided, but not both. Returns an array since a hash may map to multiple related actions.

**Note:** This field is deprecated in favour of a future `v2/transaction` query.

**Example:**

```graphql
query {
  transactions(hash: "78f3543c77c2...") {
    hash
    block {
      height
      hash
    }
    identifiers
    raw
    contractCalls {
      __typename
      ... on ContractDeploy {
        address
        state
        zswapChainState
      }
      ... on ContractCall {
        address
        state
        entryPoint
        zswapChainState
      }
    }
  }
}
```

### contract(address: HexEncoded!, offset: ContractOffset): ContractCallOrDeploy

Retrieve the latest known state of a contract at a given offset (by block or transaction). If no offset is provided, returns the latest state.

**Example (latest):**

```graphql
query {
  contract(address: "0x1") {
    __typename
    address
    state
    zswapChainState
  }
}
```

**Example (by block height):**

```graphql
query {
  contract(
    address: "0x1", 
    offset: { blockOffsetInput: { height: 10 } }
  ) {
    __typename
    address
    state
    zswapChainState
  }
}
```

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

OR

```graphql
mutation {
  # Provide the hex format:
  connect(viewingKey: "000300386224d330...") 
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

If the session does not exist, an error is returned.

## Subscriptions: Real-time Updates

Subscriptions use a WebSocket connection following the `graphql-transport-ws` protocol. After connecting and sending a `connection_init` message, the client can start subscription operations.

### Blocks Subscription

`blocks(offset: BlockOffsetInput): Block!`

Subscribe to new blocks. The `offset` parameter lets you start receiving from a given block (by height or hash). If omitted, starts from the latest block.

**Example:**

```json
{
  "id": "1",
  "type": "start",
  "payload": {
    "query": "subscription { blocks(offset: {height:10}) { hash height timestamp transactions { hash } } }"
  }
}
```

When a new block is indexed, the client receives a `next` message:

### Contracts Subscription

`contracts(address: HexEncoded!, offset: BlockOffsetInput): ContractCallOrDeploy!`

Subscribes to state changes of a given contract from a specific point. New contract states (deploys, calls, updates) are pushed as they occur.

**Example:**

```json
{
  "id": "2",
  "type": "start",
  "payload": {
    "query": "subscription { contracts(address:\"0x1\", offset: {height:1}) { __typename address state } }"
  }
}
```

### Wallet Subscription

`wallet(sessionId: HexEncoded!, index: Int, sendProgressUpdates: Boolean): WalletSyncEvent!`

Subscribes to wallet updates. This includes relevant transactions and possibly Merkle tree updates (`ZswapChainStateUpdate`), as well as `ProgressUpdate` events if `sendProgressUpdates` is set to `true`. The `index` parameter can be used to resume from a certain point.

Adjust `index` and `offset` arguments as needed.

**Example:**

```json
{
  "id": "3",
  "type": "start",
  "payload": {
    "query": "subscription { wallet(sessionId:\"1CYq6ZsLmn\", index:100, sendProgressUpdates:true) { __typename ... on ViewingUpdate { index update { __typename ... on RelevantTransaction { transaction { hash } } } } ... on ProgressUpdate { synced total } } }"
  }
}
```

**Responses** may vary depending on what is happening in the chain:
- A `ViewingUpdate` with new relevant transactions or a collapsed Merkle tree update.
- A `ProgressUpdate` indicating synchronization progress.

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
