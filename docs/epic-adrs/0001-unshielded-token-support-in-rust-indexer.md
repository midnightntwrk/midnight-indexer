# ADR: Unshielded Token Support in Rust Indexer

- **Status**: Proposed
- **Date**: 10 March 2025
- **Ticket**: [PM-14978](https://shielded.atlassian.net/browse/PM-14978) \[Indexer\] Preparation Unshielded Token
- **Reviewers**: Andrzej Kopec, Heiko Seeberger
- **Assigner**: Sean Kwak

## 1. Context

Midnight is to support both **shielded** tokens, using the **Zswap** protocol described in the whitepaper
(see [Zswap: zk-SNARK Based Non-Interactive Multi-Asset Swaps](https://iohk.io/en/research/library/papers/zswap-zk-snark-based-non-interactive-multi-asset-swaps)),
and **unshielded** tokens, such as **Night** (Midnight’s native unshielded token).

Currently, our **Rust Indexer**:
- Parses blocks/transactions focusing on Zswap shielded coins, with minimal logic for unshielded tokens (e.g., Night).
- Stores contract actions (for contract-based tokens).
- Offers queries/subscriptions for shielded events only (`wallet` subscription endpoint for shielded addresses, plus `blocks` and `contracts`).

With upcoming unshielded token features, the Indexer must:
1. **Detect & Store** unshielded outputs (UTXOs), including each UTXO’s **owner** address and token type.
2. **Track Spent vs. Unspent** state.
3. **Map each UTXO** to its creating and spending transaction(s).
4. **Handle Partial Transaction Success** (some transaction segments might fail, so the ledger omits those outputs).
5. **Expose** relevant GraphQL queries (list tokens, show transactions, search by address) and **add a new subscription** for real-time unshielded updates.

The above enhancements require:
- **New or extended database schema**
- **Extended parsing** (to handle unshielded outputs/spends)
- **GraphQL** expansions: queries + a separate subscription for unshielded addresses

### References & Dependencies
- **Ledger Specs**:
  - [Night (unshielded) tokens specification](https://github.com/input-output-hk/midnight-architecture/blob/main/specification/night.md)
  - [Contracts and unshielded token ownership](https://github.com/input-output-hk/midnight-architecture/blob/main/specification/contracts.md)
- **Jira Tickets**:
  - PM-14980: "Index unshielded token data to collect data necessary to provide needed APIs"
  - PM-13979: "List Unshielded Tokens by address"
  - PM-13980: "View Historical Transaction Details"
  - PM-13982: "Support unshielded token ownership by contracts"
  - PM-14184: "Unshielded wallet synchronization benchmark"
- **Existing Code**:
  - `chain-indexer` crate for block/transaction processing
  - `indexer-api` crate for GraphQL & subscriptions
  - Postgres schema for `transactions`, `blocks`, `contract_actions`, etc.

## 2. Decision
1. We will **extend** the **Postgres schema** to hold `unshielded_utxos`.
2. We will update The **GraphQL Query API** :
    - We provide a new `unshieldedUtxos(address: UnshieldedAddress!)` query (plus subscription).
      (Note: The "address" must be in Bech32m format, not hex.)
    - We **enhance** the existing `transactions(...)` query to allow an `address` argument that filters transactions by whether they create or spend unshielded UTXOs belonging to that address.
3. We will update The **GraphQL Subscription API** :
    - A new subscription (e.g. `subscription { unshieldedUtxos(address: UnshieldedAddress!) }`) that streams newly created/spent UTXOs for that address in Bech32m format, letting wallet clients track changes in real time.
    - Not rename the existing `wallet` subscription yet; we will introduce a separate unshielded subscription rather than unify shielded/unshielded.
      They must remain distinct because the underlying keys are unrelated. Otherwise, the indexer would require a way to correlate them, which we do not want—this will simplify future changes to the trust model.
4. We will **Maintain** a separate table (or extended columns) for unshielded *UTXOs*,
   not a different "unshielded transaction" entity—since any single transaction can contain both
   shielded and unshielded parts. We may store a flag on the existing `transactions` table
   if needed to note presence of unshielded outputs, but the transaction itself is still just "a Midnight tx."
5. We will **Parse** unshielded data from each block’s extrinsics/ledger details. This includes:
    - A new "UnshieldedOffer / UtxoOutput" (similar to the Zswap logic) or "Night" output that references a Bech32m address.
      (Note: BLS is not itself an address format; BLS is an elliptic-curve family used internally for keys.)
    - A "UtxoSpend" or similar event indicating the coin is spent by a subsequent transaction.
      **Updated Snippet for GraphQL Queries (Aligned with Existing Indexer–API Conventions)**
6. We will **not** introduce a separate `unshieldedTransactions(...)` query. Instead, we can:

### Database Schema

```sql
CREATE TABLE unshielded_utxos (
   id BIGSERIAL PRIMARY KEY,

   -- The transaction that created this UTXO
   creating_transaction_id BIGINT NOT NULL REFERENCES transactions(id),
   --(Note: "id" here refers to our internal DB primary key for the transactions table, not the "merged" transaction’s identifier in ledger terms. We store multiple identifiers in a separate array or mapping, but the Indexer uses an auto-increment PK for referencing.)

    -- The index (0-based) within the above transaction
   output_index INT NOT NULL,

   -- The address that owns this UTXO, stored as raw bytes (e.g. bech32 decoded)
   owner_address BYTEA NOT NULL,

   -- The token type, e.g. "Night" or user-defined, stored as raw bytes (32 bytes)
   token_type BYTEA NOT NULL,

   -- The UTXO value (quantity of tokens)
   value NUMERIC(38, 0) NOT NULL,

   -- If spent, references the transaction that consumed this UTXO
   spending_transaction_id BIGINT REFERENCES transactions(id),

   UNIQUE (creating_transaction_id, output_index)
);

CREATE INDEX unshielded_owner_idx ON unshielded_utxos(owner_address);
CREATE INDEX unshielded_token_type_idx ON unshielded_utxos(token_type);
CREATE INDEX unshielded_spent_idx ON unshielded_utxos(spending_transaction_id);
```

- Removed spent_block_id and spent_timestamp: since the team has decided not to handle reorg/rollback logic on unshielded tokens, we do not need those columns.
- Partial success/failure: I think, the ledger omits failed-segment outputs from the final block data. Hence, the indexer only sees "valid unshielded outputs". If part of a transaction fails (fallible section), the ledger’s final application will mark those sections as failed. In practice, no UTXOs from failed segments are ‘activated’, so they do not appear in the final block data. The node also reports partial vs. total success (e.g. `PartialSuccess { segmentsSucceeded: ... }`). We’ll rely on that event/log data to index only the actually changed UTXOs.
- **Why separate table**?
  - Helps keep unshielded token data distinct from shielded transactions. The existing `transactions` table can remain for "top-level" block/tx references.
- **Spent vs. Unspent**:
  - `spending_transaction_id` is `NULL` until the UTXO is spent.
  - Q: Could also add a boolean `spent` but `spending_transaction_id IS NOT NULL` can be enough? A: We won’t add a separate boolean. spending_transaction_id provides that info while also linking to the consuming transaction, which is helpful for the GraphQL API.

We will definitely index owner_address because it’s the most common query. A combined (owner_address, token_type) index might be added if needed—likely for explorers focusing on a single token type such as NIGHT.

### Reorgs / Rollbacks?

Finalization by definition means the chain will not revert those blocks. (Though in certain consensus protocols, 'finality' can be probabilistic.) The Rust Indexer will not implement an explicit rollback for these finalized blocks in mainnet usage. If reorg or rollback becomes a future requirement, we can revisit.

If we end up having the need to have reorg/rollback due to edge case scenarios that we haven't foreseen, we can revisit this PR discussion.

### Reading Unshielded Data from Blocks
- The **node** or **ledger** might supply a structure similar to `UnshieldedOutput` or `NightOutput` in the block. Per recent node clarifications, the node will emit exactly one event for each transaction containing unshielded outputs. This event includes a vector of all relevant UTXOs (scale-encoded). If multiple addresses receive tokens in the same transaction, they appear together in this single event. If multiple transactions occur in one block, the node may produce multiple such events.
- Our existing block-parsing logic in `chain-indexer/src/infra/node.rs` can be extended:

```rust
fn parse_unshielded_outputs(
    block: &SubxtBlock
) -> Vec<UnshieldedUtxo> {
    // 1. inspect each extrinsic or event for a night or unshielded type
    // 2. decode the "owner_address", "token_type", "value", etc.
    // 3. store them in a Vec<UnshieldedUtxo> as placeholders
}
```

- Then, once we store the `transactions` row, we do:

```rust
for (index, utxo) in unshielded_outputs.iter().enumerate() {
  // Insert into unshielded_utxos with creating_transaction_id and output_index
}
```

### Partial Success/Failure

- For partial success logic, the Indexer must reflect that if a transaction fails in the fallible section, **unshielded outputs** from that fail section aren’t added.
- Implementation detail:
    - The node or ledger may produce an event marking which segments of the transaction were successful. The Indexer can use that to decide whether to insert or skip.
    - If the ledger data simply excludes the outputs, then parse logic sees no unshielded outputs for those segments.


### GraphQL Query API Update
We propose adding two queries for unshielded tokens and utilising the existing transaction query API with optional types :

1. **`unshieldedUtxos`** : Returns all unshielded UTXOs for the given Bech32m address.
    - Each `UnshieldedUtxo` includes:
        - The `Night` case: the `token_type` field is a 32-byte array of zeros.
        - The **value** (numeric)
        - In GraphQL, we typically return an object link (e.g., createdAtTransaction) so that the client can fetch more transaction details in one query. Similarly for spentAtTransaction. That eliminates the need for separate IDs in a REST style approach.
        - A **reference** to the spending `Transaction`, if any
        - Possibly a `spent` boolean or `spendingTransactionId` field
```graphql
type Query {
  """
  Retrieve all unshielded UTXOs (both spent and unspent) associated with a given address.
  This includes token type, value, creation/spending transaction references, etc.
  Optionally supports an offset (block or transaction offset) for pagination or chronological limiting.
  """
  unshieldedUtxos(address: UnshieldedAddress!, offset: UnshieldedOffsetInput): [UnshieldedUtxo!]!
}
```
**How the indexer resolves `unshieldedUtxos(...)`:**

i) Take the `address` argument (in bech32 form).

ii) Query the `unshielded_utxos` table filtering by `owner_address = :addressBytes`. 

iii) Return rows with relevant columns:
    - `spending_transaction_id` can be null or not null.
    - Possibly join or call out to get the transaction hash for `creating_transaction_id` or `spending_transaction_id`, if needed for display.

2.  **`transactions`** : 
Instead of a separate unshieldedTransactions query, we will augment the existing transactions(...) query to allow filtering by an unshielded address. Currently, we have:

```graphql
type Query {
    transactions(hash: HexEncoded, identifier: HexEncoded): [Transaction!]!
}
```

We will add an optional address: UnshieldedAddress parameter:
```graphql

"""
  `UnshieldedAddress` is a custom scalar for an unshielded address in
  Bech32m format, e.g. `mn_addr_test1wehcv...`. It cannot be a shielded
  viewing key or contract address, and must decode into raw bytes to match
  the `owner_address` column in our DB.
"""
scalar UnshieldedAddress

type Query {
  """
  Returns all transactions, optionally filtered by:
  - hash: a transaction hash
  - identifier: a unique identifier
  - address: an unshielded address (Bech32m) representing unshielded UTXOs.
    Only transactions that create/spend UTXOs for this address are returned.
  """
  transactions(
    hash: HexEncoded,
    identifier: HexEncoded,
    address: UnshieldedAddress
  ): [Transaction!]!
```

Address filter: If address is provided, only those transactions creating or spending unshielded outputs for that address are returned.
Transaction type remains the same (we do not introduce a new "unshielded transaction" concept).
Introspection: Within each returned Transaction, clients can see unshielded inputs/outputs in fields like unshieldedCreatedOutputs and unshieldedSpentOutputs.

**Query Behavior**:

- If the `address` argument is **omitted**, it behaves as before (returns all transactions matching hash or identifier).
- If `address` is **provided**, the resolver does:
    1. Query the `unshielded_utxos` table to find any `creating_transaction_id` or `spending_transaction_id` referencing that address as `owner_address`.
    2. Gather those transaction IDs.
    3. Return the corresponding Transaction rows (plus any fields from the normal indexing logic).

**Inside each Transaction** result, the fields `unshieldedCreatedOutputs` and `unshieldedSpentOutputs` are computed by joining against `unshielded_utxos`:

- `unshieldedCreatedOutputs` = rows where `creating_transaction_id == thisTransaction.id`.
- `unshieldedSpentOutputs` = rows where `spending_transaction_id == thisTransaction.id`.

Graphql Query Example :
```graphql
query transactions($address: UnshieldedAddress) {
    hash
    applyStage
    unshieldedCreatedOutputs {
      owner
      value
    }
    unshieldedSpentOutputs {
      owner
      value
    }
}
```

Required Types in both queries :
```graphql
scalar UnshieldedAddress

type UnshieldedUtxo {
    owner: String!           # bech32-encoded address
    value: String!
    tokenType: String!       # hex or other representation
    intentHash: String!      # from the ledger’s transaction
    outputIndex: Int!
    createdAtTransaction: Transaction!
    spentAtTransaction: Transaction
}

type Transaction {
    hash: HexEncoded!
    protocolVersion: Int!
    applyStage: ApplyStage!
    identifiers: [HexEncoded!]!
    raw: HexEncoded!
    contractCalls: [ContractCallOrDeploy!]!
    merkleTreeRoot: HexEncoded!
    
    # New fields for unshielded
    unshieldedCreatedOutputs: [UnshieldedUtxo!]!
    unshieldedSpentOutputs: [UnshieldedUtxo!]!
}
```

This design matches the established pattern for offset-based queries (e.g., `BlockOffsetInput` or `TransactionOffsetInput` in our existing GraphQL). While we store unshielded data in its own table (for UTXO-like outputs),
the same `Transaction` can contain both shielded and unshielded parts.
Hence, from an API perspective, we do not treat them as separate transaction types—
we simply store different categories of outputs and link them back to the same `Transaction`.

- If our primary client needs only to see transactions (and the relevant unshielded outputs within them), then yes, transactions(address:) + the new unshieldedCreatedOutputs, unshieldedSpentOutputs fields is enough.
- If our client wants a more direct listing of unspent outputs (like a "UTXO model" approach) without pulling every transaction:
    A dedicated unshieldedUtxos(address:, offset:) can be more convenient and efficient.
    You avoid reading a big list of transactions only to filter out the outputs you want.

### GraphQL Subscription API Update
We propose a new subscription dedicated to unshielded addresses, analogous (but not identical) to the existing `wallet` subscription used for shielded addresses:

```graphql
scalar UnshieldedAddress

type Subscription {
    """
    This will emit one event per transaction that creates or spends unshielded UTXOs for the given address.
    Each event includes newly created and spent UTXOs for that transaction,
    or a PROGRESS event for keep-alives or sync updates.

    The provided address argument must be a valid unshielded address
    in Bech32m format (ex: `mn_addr...`).
    """
    unshieldedUtxos(address: UnshieldedAddress!): UnshieldedUtxoEvent!
}

enum UnshieldedUtxoEventType {
    UPDATE
    """
    This event type indicates a status or heartbeat message.
    For instance, during initial sync or when no new transactions are found,
    the indexer can emit PROGRESS events to let the client know it's still alive
    or to convey partial synchronization progress.

    For periods where no new unshielded transactions occur, or during initial synchronisation,
    the indexer may emit PROGRESS events. These have type = PROGRESS,
    an empty list of createdUtxos/spentUtxos, and the last relevant transaction
    for this address. The purpose is to inform subscribers that the service
    is active or still syncing historical data.
    """
    PROGRESS
}

type UnshieldedUtxoEvent {
    eventType: UnshieldedUtxoEventType!
    transaction: Transaction!
    createdUtxos: [UnshieldedUtxo!]!
    spentUtxos: [UnshieldedUtxo!]!
}
```

**Subscription Behaviour**:

- The indexer emits an event whenever a new UTXO is created or an existing UTXO is spent for `owner_address == address`.
- The wallet listening on `unshieldedUtxos(address: "mn_addr_something...")` can then update its standalone state of UTXOs in real time.

**Why separate from `wallet` subscription?**
- The existing `wallet(sessionId: HexEncoded!)` subscription is purely for shielded addresses/viewing keys.
- Unshielded addresses differ in format (bech32) and logic.
- The node lumps all unshielded outputs from a single transaction into one event.
- We keep them distinct for clarity, deferring any renaming or unification to a future iteration.

## 4. Alternatives Considered

1. **JSON Column**
    - Could store unshielded outputs in a JSON array within `transactions`. This is less query-friendly.
2. **Reuse `transactions` Table for UTXO Rows**
    - Potentially store each unshielded output as a row in `transactions`.
    - This complicates the structure, since `transactions` is top-level.

**Chosen**: A dedicated `unshielded_utxos` table to keep a normalized approach.

## 5. Consequences

- **Pros**:
    - Normalized, easy to query unshielded data (by address, token, spent vs. unspent).
    - Clear separation from shielded indexing logic.
    - Straightforward to integrate partial success logic (failed segments produce no UTXOs).
    - (New) Subscription approach lets wallets avoid polling.
- **Cons**:
    - Additional table (some overhead).
    - We must confirm node emits unshielded events or extrinsics we can parse unambiguously.
    - Two separate subscription endpoints for shielded vs. unshielded addresses.

## 6. Work Plan (High-Level)

1. **DB Schema Migrations**
    - Create `unshielded_utxos` table with needed columns, indexes.
2. **Block Parsing**
    - Extend `chain-indexer` logic to detect unshielded outputs/spends in final ledger data, ignoring any failed segments.
    - Insert UTXOs (with `creating_transaction_id`) for newly created ones.
    - Update `spending_transaction_id` if an output is consumed.
3. **GraphQL & Subscription**
    - (New) `unshieldedUtxos(...)` query for direct listing.
    - Optionally extend `transactions(address:)`.
    - (New) subscription for real-time unshielded token changes: `unshieldedUtxos(address: ...)`.
4. **Testing & QA**
    - Integration tests for block parser & DB state.
    - Query tests verifying correct data retrieval.
    - Subscription E2E tests to confirm events on creation/spend.
    - (Future) PM-14184 unshielded wallet sync benchmark.

## 7. Open Questions

1. **Node Data Format**: 
   - Q: Which final events or extrinsics specifically indicate unshielded outputs/spends?
   - A: unshieldedOutputs: Map<TokenType, bigint>, [Midnight-ledger-prototype PR 341](https://github.com/midnightntwrk/midnight-ledger-prototype/pull/341/files). This aligns with the node’s plan to produce scale-encoded data listing unshielded outputs, i.e. `Map<TokenType, bigint>` or a vector of UTXOs. The Indexer will decode that event to obtain `(owner_address, token_type, value)` for each UTXO.
2. **Subscription Batching**:
   - Per the node team, **one event** is created for **each transaction** that has unshielded outputs/spends, bundling **all** of those UTXOs in a single vector. If multiple transactions in a block produce unshielded outputs, we’ll see multiple events in that block. This clarifies that there is no further "batching" across transactions or blocks.
3. **Edge Cases**: Large merges or partial merges (swaps) producing multiple unshielded outputs?

## 8. Decision

Proceed with:

- **Dedicated** `unshielded_utxos` table.
- **GraphQL**: new query (`unshieldedUtxos(...)`), complemented `transaction(...)` and new subscription (`unshieldedUtxos(address: ...)`) for real-time updates.

## 9. Status History

- **Proposed**: 10 March 2025