# Complete DUST API Design

## Design Philosophy
- Maintain all requirements from DUST specification
- Include historical Merkle root access for wallet proofs
- Convert large/unbounded datasets to subscriptions

## API Design

### Queries (Small/Bounded Data)

```graphql
type Query {
  """
  Get current DUST system state. Single object.
  """
  currentDustState: DustSystemState!
  
  """
  Get DUST generation status for specific stake keys. Max 10 items.
  """
  dustGenerationStatus(
    cardanoStakeKeys: [String!]!  # Max 10 keys
  ): [DustGenerationStatus!]!
  
  """
  Get historical Merkle tree root for a specific timestamp.
  CRITICAL: Wallets need this for generating proofs with historical data.
  """
  dustMerkleRoot(
    treeType: DustMerkleTreeType!
    timestamp: Int!
  ): HexEncoded
}
```

### Subscriptions (Large/Unbounded Data)

```graphql
type Subscription {
  """
  Stream generation info for wallet reconstruction.
  Replaces generationInfoByDustAddress query.
  """
  dustGenerationInfo(
    address: String!
    fromIndex: Int = 0
    onlyActive: Boolean! = true
  ): DustGenerationInfoEvent!
  
  """
  Stream transactions by nullifier prefix.
  Replaces transactionsByNullifierPrefix query.
  """
  dustTransactionsByNullifierPrefix(
    prefixes: [String!]!      # Max 10 prefixes
    minPrefixLength: Int! = 8
    fromBlock: Int = 0
  ): DustTransactionEvent!
  
  """
  Stream DUST spending transactions for an address.
  Tracks DUST UTXO lifecycle.
  """
  dustSpendingTransactions(
    address: String!
    fromTransactionId: Int = 0
  ): DustSpendingEvent!
  
  """
  Stream Merkle tree updates for wallet sync.
  """
  dustMerkleUpdates(
    startIndex: Int!
  ): DustMerkleUpdate!
  
  """
  Stream registration changes.
  """
  registrationUpdates(
    cardanoStakeKeys: [String!]!  # Max 100 keys
  ): RegistrationUpdate!
}
```

### Data Types

```graphql
# Query Types
type DustSystemState {
  commitmentTreeRoot: HexEncoded!
  generationTreeRoot: HexEncoded!
  blockHeight: Int!
  timestamp: Int!
  totalActiveRegistrations: Int!
}

type DustGenerationStatus {
  cardanoStakeKey: String!
  dustAddress: String
  isRegistered: Boolean!
  generationRate: String!     # Specks per second
  currentCapacity: String!    # Current DUST capacity
  nightBalance: String!       # NIGHT backing generation
}

# Subscription Event Types
union DustGenerationInfoEvent = DustGenerationInfo | DustGenerationInfoProgress

union DustTransactionEvent = DustTransaction | DustTransactionProgress

union DustSpendingEvent = DustSpending | DustSpendingProgress

# Data Types
type DustGenerationInfo {
  """
  Night UTXO hash (or cNIGHT hash for Cardano).
  """
  nightUtxoHash: HexEncoded!
  """
  Generation value in Specks (u128 as string).
  """
  value: String!
  """
  DUST public key of owner.
  """
  owner: String!
  """
  Initial nonce for DUST chain.
  """
  nonce: HexEncoded!
  """
  Creation time (UNIX timestamp).
  """
  ctime: Int!
  """
  Destruction time. Null if still generating.
  """
  dtime: Int
  """
  Index in generation Merkle tree.
  """
  merkleIndex: Int!
}

type DustGenerationInfoProgress {
  highestIndex: Int!
  activeGenerations: Int!
}

type DustTransaction {
  transaction: Transaction!
  matchingNullifierPrefixes: [String!]!
}

type DustTransactionProgress {
  highestBlock: Int!
  matchedCount: Int!
}

type DustSpending {
  transaction: Transaction!
  commitment: HexEncoded!
  nullifier: HexEncoded!
  value: String!              # Amount spent
  fee: String!                # Fee paid (v_fee)
  owner: String!              # DUST address
}

type DustSpendingProgress {
  highestTransactionId: Int!
  totalSpent: Int!
}

type DustMerkleUpdate {
  treeType: DustMerkleTreeType!
  index: Int!
  collapsedUpdate: HexEncoded!
  blockHeight: Int!
}

type RegistrationUpdate {
  cardanoStakeKey: String!
  dustAddress: String
  isActive: Boolean!
  timestamp: Int!
}

enum DustMerkleTreeType {
  COMMITMENT
  GENERATION
}
```

## Storage Layer (this is for heiko)

```rust
#[trait_variant::make(Send)]
pub trait Storage
where 
    Self: Clone + Send + Sync + 'static
{
    // Bounded queries
    async fn get_current_dust_state(&self) -> Result<DustSystemState, sqlx::Error>;
    
    async fn get_dust_generation_status_batch(
        &self,
        cardano_stake_keys: &[String]  // Max 10
    ) -> Result<Vec<DustGenerationStatus>, sqlx::Error>;
    
    async fn get_dust_merkle_root_at_timestamp(
        &self,
        tree_type: DustMerkleTreeType,
        timestamp: i64,
    ) -> Result<Option<Vec<u8>>, sqlx::Error>;
    
    // Streaming methods for subscriptions
    fn stream_dust_generation_info(
        &self,
        address: &str,
        from_index: i64,
        only_active: bool,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustGenerationInfo, sqlx::Error>>;
    
    fn stream_dust_transactions_by_prefix(
        &self,
        prefixes: &[String],
        min_prefix_length: usize,
        from_block: i64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustTransaction, sqlx::Error>>;
    
    fn stream_dust_spending_transactions(
        &self,
        address: &str,
        from_transaction_id: i64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<DustSpending, sqlx::Error>>;
}
```
