# Indexer Cost Model Tickets - Detailed Technical Analysis
**Date**: September 30, 2025
**For**: Heiko Seeberger
**From**: Sean Kwak
**Repository**: midnight-indexer (this repo)
**Epic**: PM-14165 (Cost Model)

## Executive Summary

The Indexer needs to extract, store, and expose transaction cost data from the Midnight blockchain. This analysis is based on:
- **Ledger specification**: `midnight-ledger/spec/cost-model.md`
- **Ledger implementation**: `midnight-ledger/ledger/src/structure.rs` (v6.1.0)
- **Current indexer code**: `midnight-indexer/` (this repository)
- **Node implementation**: `midnight-node` (v0.16.3)

**Critical Discovery**: Two tickets (PM-19727 and PM-19761) are already complete! LedgerParameters are already available in LedgerState after each block since ledger v6.1.0. This significantly simplifies the implementation.

## Original Ticket Creation Rationale

These 8 tickets were created for the PM-14165 (Cost Model) epic based on a systematic analysis of the cost model architecture across multiple repositories:

### Discovery Process:
1. **Ledger Analysis** (`midnight-ledger/`):
   - Studied `spec/cost-model.md` - found 5-dimensional cost model
   - Examined `ledger/src/structure.rs` - discovered Transaction::cost() method
   - Reviewed `ledger/src/semantics.rs` - found post_block_update() fee adjustment

2. **Node Investigation** (`midnight-node/`):
   - Checked event emissions - node provides block and transaction data
   - Analyzed RPC interfaces - no direct cost exposure found
   - Reviewed transaction processing - costs calculated internally but not exposed

3. **Wallet Requirements** (`midnight-wallet/` + Slack discussions):
   - Simon's explicit request for fee calculation before submission
   - Need for historical fee data for prediction
   - UX requirement to show fee breakdowns

4. **Indexer Capabilities** (`midnight-indexer/`):
   - Existing DUST event tracking infrastructure
   - GraphQL API patterns already established
   - Storage abstraction supporting both PostgreSQL and SQLite

### Ticket Design Principles:
1. **Layered Approach**: Start with data storage (tables), then extraction, calculation, aggregation, and finally API exposure
2. **Dependency Chain**: Each ticket builds on the previous ones, creating a clear implementation path
3. **Separation of Concerns**: Each ticket handles a specific aspect (storage, calculation, API, documentation)
4. **Risk Distribution**: High-risk items (PM-19728) are isolated to allow parallel work on lower-risk tickets

### Why 8 Tickets:
- **PM-19726**: Foundation - database schema for all cost data (discovered need from ledger spec)
- **PM-19727**: Data source - getting parameters from blockchain (Simon's wallet requirement)
- **PM-19728**: Core computation - most complex challenge (ledger has methods but node doesn't expose)
- **PM-19729**: Aggregation - block-level metrics (network operators need this)
- **PM-19730**: API layer - exposing collected data (wallet/explorer integration)
- **PM-19731**: User feature - showing fee breakdowns (user support requirement)
- **PM-19732**: Integration - connecting with DUST system (reconciliation need)
- **PM-19733**: Documentation - ensuring proper API usage (developer adoption)

## Original JIRA Ticket Descriptions with Business Justification

### PM-19726: Add Cost Model Tables (3 points)
**Business Need:**
- **Wallet Requirement**: Simon's message (Sept 29) - wallet needs historical fee data to predict costs
- **Ledger Spec**: `midnight-ledger/spec/cost-model.md` defines 5 cost dimensions that must be tracked
- **Node Events**: `midnight-node` emits cost data in events that needs persistent storage

**Description:**
Create database schema to store cost model data including transaction costs, block capacity usage, and fee analysis.

**Acceptance Criteria:**
- Database migration script created with cost model tables
- Tables support both PostgreSQL (cloud) and SQLite (standalone) modes
- Proper foreign key relationships to existing transaction and block tables
- Indexes optimized for common query patterns

**Source References:**
- `midnight-ledger/base-crypto/src/cost_model.rs:152` - SyntheticCost structure
- `midnight-indexer/indexer-common/migrations/postgres/001_initial.sql` - existing pattern

### PM-19727: Extract Cost Model Parameters from Node (1 point) ✅
**Business Need:**
- **Wallet Critical**: Simon explicitly requested this - "wallet needs to calculate fees before submission"
- **Dynamic Fees**: Since ledger v6.1.0, fees change EVERY BLOCK via `post_block_update()`
- **User Transparency**: Users need to know current fee prices before transacting

**Description:**
Extract and store LedgerParameters (cost model configuration) from the blockchain for each block.

**Acceptance Criteria:**
- Parameters extracted from LedgerState after each block
- Parameters stored in block_parameters table
- Serialization/deserialization methods implemented
- Parameters available for cost calculations

**Source References:**
- `midnight-ledger/ledger/src/semantics.rs:902` - post_block_update() implementation
- `midnight-ledger/ledger/src/structure.rs:266` - LedgerParameters structure
- Slack thread: Simon's request for wallet fee calculation

### PM-19728: Calculate Transaction Cost Dimensions (8 points)
**Business Need:**
- **Fee Transparency**: Users asking "why did I pay X DUST?" need breakdown
- **Network Analysis**: Operators need to identify which operations are expensive
- **Wallet Optimization**: Wallet can optimize transaction construction knowing cost drivers

**Description:**
Calculate and store the 5-dimensional cost breakdown for each transaction (read_time, compute_time, block_usage, bytes_written, bytes_churned).

**Acceptance Criteria:**
- Extract validation and application costs for each transaction
- Store detailed cost breakdown in transaction_costs table
- Handle both shielded and unshielded transactions
- Calculate dominant cost dimension for each transaction

**Source References:**
- `midnight-ledger/ledger/src/structure.rs:1700` - Transaction::cost() method
- `midnight-ledger/base-crypto/src/cost_model.rs` - cost calculation logic
- `midnight-wallet` needs this for transaction optimization

### PM-19729: Store Block Capacity Usage (5 points)
**Business Need:**
- **Network Health**: Operators need to monitor block utilization trends
- **Fee Adjustment**: Understand why fees are increasing/decreasing
- **Capacity Planning**: Identify when network needs scaling

**Description:**
Aggregate transaction costs to calculate and store block capacity usage metrics.

**Acceptance Criteria:**
- Sum all transaction costs per block
- Calculate utilization percentage for each dimension
- Identify bottleneck dimension limiting block capacity
- Store metrics in block_capacity_usage table

**Source References:**
- `midnight-ledger/spec/cost-model.md` - block capacity limits
- `midnight-node` block fullness tracking for fee adjustment
- Network operators' dashboard requirements

### PM-19730: Extend GraphQL API for Cost Model (5 points)
**Business Need:**
- **Wallet Integration**: Wallet team needs GraphQL access to fee data
- **Explorer Requirements**: Block explorers need to show cost information
- **Developer Tools**: dApp developers need cost data for optimization

**Description:**
Add GraphQL schema and resolvers to expose cost model data through the API.

**Acceptance Criteria:**
- New GraphQL types for cost dimensions and parameters
- Queries for transaction costs and block capacity
- Subscription support for real-time cost updates
- Efficient resolvers avoiding N+1 queries

**Source References:**
- `midnight-indexer/indexer-api/graphql/schema-v1.graphql` - existing schema
- `midnight-wallet` GraphQL client requirements
- Explorer UI mockups showing cost data

### PM-19731: Add Fee Breakdown to Transaction Details (3 points)
**Business Need:**
- **User Support**: Support team needs to explain fees to users
- **Wallet UX**: Show users what they're paying for (computation vs storage vs I/O)
- **Developer Debugging**: Developers need to understand transaction costs

**Description:**
Show detailed fee breakdown by cost dimension for each transaction in the API.

**Acceptance Criteria:**
- Calculate fee contribution from each cost dimension
- Identify dominant dimension driving the fee
- Compare calculated vs actual fees paid
- Expose breakdown in transaction GraphQL type

**Source References:**
- `midnight-ledger/ledger/src/semantics.rs` - fee calculation per dimension
- Wallet UI requirements for fee display
- Support tickets about "unexplained" fees

### PM-19732: Cost Model + DUST Generation Integration (5 points)
**Business Need:**
- **Reconciliation**: Finance team needs to verify DUST burns match calculated fees
- **Overpayment Analysis**: Identify users paying more than necessary
- **Protocol Economics**: Verify fee mechanism working as designed

**Description:**
Integrate cost model data with DUST generation events to analyze fee payment patterns.

**Acceptance Criteria:**
- Link transaction costs with DUST spend events
- Track overpayments and underpayments
- Identify transactions with insufficient fees
- Provide fee sufficiency analysis in API

**Source References:**
- `midnight-indexer` existing DUST event tracking (PM-19400)
- `midnight-ledger` fee payment validation
- Economic model documentation requiring fee analysis

### PM-19733: Document Cost Model API (2 points)
**Business Need:**
- **Developer Adoption**: External developers need clear documentation
- **Wallet Team**: Wallet team requested examples for integration
- **API Consistency**: Maintain quality of existing API documentation

**Description:**
Update API documentation with cost model features and usage examples.

**Acceptance Criteria:**
- Document all new GraphQL types and fields
- Provide example queries for common use cases
- Explain cost model concepts for API consumers
- Include performance considerations

**Source References:**
- `midnight-indexer/docs/api/v1/api-documentation.md` - existing docs
- Wallet team's integration requirements
- Developer portal documentation standards

## Major Update: PM-19727 & PM-19761 Already Implemented!

**Timeline**: September 29-30, 2025
- **Sept 29**: Discovery during Slack discussion with Simon
- **Sept 29**: Implementation of both tickets
- **Sept 30**: PR #384 created and merged (PR #382 had branch issues)
- **Sept 30**: PR #385 created for field renaming (pending)

During Slack discussion with Simon Gellis about wallet fee calculation needs, we discovered that:

1. **LedgerParameters change EVERY BLOCK** (since ledger v6.1.0)
   - Not just on SystemTransaction::OverwriteParameters
   - Updated via `post_block_update()` based on block fullness
   - Contains all fee prices and cost model data

2. **Data Already Available in Indexer**
   - LedgerState already contains the parameters
   - No RPC or external calls needed
   - Just needed to serialize and expose via GraphQL

**Implementation (Completed in PR #384, merged September 30):**

```rust
// Domain type for versioned parameters (indexer-common/src/domain/ledger/ledger_state.rs)
#[derive(Debug, Clone)]
pub enum LedgerParameters {
    V6(midnight_ledger_prototype::ledger::LedgerParameters),
}

impl LedgerParameters {
    pub fn serialize(&self) -> Result<Vec<u8>, Error> {
        match self {
            Self::V6(params) => bincode::serialize(params)
                .map_err(|e| Error::Serialization(e.to_string()))
        }
    }
}

// Extraction after block processing (refactored by Heiko)
pub fn post_apply_transactions(
    &mut self,
    block_timestamp: u64,
) -> Result<LedgerParameters, Error> {
    match self {
        Self::V6 { ledger_state, block_fullness } => {
            // block_fullness accumulated during transaction processing
            let timestamp = timestamp_v6(block_timestamp);
            let ledger_state = ledger_state
                .post_block_update(timestamp, *block_fullness)
                .map_err(|error| Error::BlockLimitExceeded(error.into()))?;

            let ledger_parameters = ledger_state.parameters.deref().to_owned();

            *self = Self::V6 {
                ledger_state,
                block_fullness: Default::default(), // Reset for next block
            };

            Ok(LedgerParameters::V6(ledger_parameters))
        }
    }
}
```

**Block Fullness Tracking:**
```rust
// Block fullness is accumulated as transactions are applied
// Each transaction contributes to the block's resource usage
// This cumulative fullness is then used to adjust fees for the NEXT block
```

**Story Points:**
- PM-19727: **1** (correctly estimated - extraction was simpler than initially understood)
- PM-19761: **1** (new ticket from Simon - simple GraphQL field addition)
- **Combined: 2 story points total** (implemented in ~3 hours)

## Background: What the Cost Model Represents

### Five Cost Dimensions (from midnight-ledger/spec/cost-model.md)
```rust
// From: midnight-ledger/base-crypto/src/cost_model.rs:152
pub struct SyntheticCost {
    pub read_time: CostDuration,     // I/O read time (picoseconds)
    pub compute_time: CostDuration,  // CPU time (picoseconds)
    pub block_usage: u64,            // Bytes in block
    pub bytes_written: u64,          // Persistent storage (net bytes written)
    pub bytes_churned: u64,          // Temporary storage
}
```

### Dynamic Fee Adjustment Algorithm

The ledger implements dynamic fee adjustment to maintain 50% block utilization:

```rust
// From midnight-ledger/ledger/src/semantics.rs:902-920
pub fn post_block_update(
    mut self,
    timestamp: Timestamp,
    block_fullness: Fraction,
) -> Result<Self> {
    let adjusted_fee_prices = self.parameters.fee_prices.adjust(
        self.parameters.price_adjustment_parameter,
        block_fullness,
        self.parameters.cost_dimension_min_ratio,
    );

    self.parameters = Arc::new(LedgerParameters {
        fee_prices: adjusted_fee_prices,
        ..(*self.parameters).clone()
    });

    Ok(self)
}
```

**Key Insight**: Fee prices adjust EVERY BLOCK based on:
- Target utilization: 50%
- Adjustment parameter: Controls adjustment speed
- Min ratio: Prevents any dimension from becoming free

## Ticket-by-Ticket Analysis (Updated)

### PM-19726: Add Cost Model Tables (3 points) ✅

**Status**: Design ready, straightforward implementation

**What it does**: Creates database schema to store detailed cost model data.

**Required Tables**:
```sql
-- Core parameter storage (now updated every block)
CREATE TABLE block_parameters (
    block_id BIGINT PRIMARY KEY REFERENCES blocks(id),
    raw BYTEA NOT NULL  -- Serialized LedgerParameters (already done!)
);

-- Additional detail tables for cost analysis
CREATE TABLE transaction_costs (
    transaction_id BIGINT PRIMARY KEY REFERENCES transactions(id),
    validation_cost JSONB,    -- SyntheticCost for validation phase
    application_cost JSONB,   -- SyntheticCost for application phase
    total_cost JSONB,         -- Combined costs
    fees_paid BYTEA,          -- Actual fees paid (u128 as bytes)
    dominant_dimension TEXT   -- Which dimension drove the fee
);

CREATE TABLE block_capacity_usage (
    block_id BIGINT PRIMARY KEY REFERENCES blocks(id),
    read_time_used BIGINT,     -- picoseconds used
    compute_time_used BIGINT,
    block_bytes_used BIGINT,
    bytes_written BIGINT,
    bytes_churned BIGINT,
    utilization_percentages JSONB  -- per dimension
);

-- Fee sufficiency tracking
CREATE TABLE fee_analysis (
    transaction_id BIGINT PRIMARY KEY REFERENCES transactions(id),
    calculated_fee BYTEA,      -- What should have been paid
    actual_fee_paid BYTEA,     -- What was actually paid
    overpayment_amount BYTEA,  -- Difference if overpaid
    fee_sufficient BOOLEAN
);
```

**Complexity**: SIMPLE - Standard database migration pattern

---

### PM-19727: Extract Cost Model Parameters from Node (1 point) ✅ DONE!

**Status**: COMPLETED in PR #384 (originally PR #382, recreated due to branch issue)

**What was done**:
- Added LedgerParameters enum with versioning (V6 variant)
- Parameters extracted after every block's `post_block_update()`
- Stored in new `block_parameters` table using bincode serialization
- No RPC needed - data already in LedgerState!

**Implementation Details**:
- Initially saved parameters outside transaction (fixed by reviewer)
- Heiko refactored to return LedgerParameters from `post_apply_transactions()`
- Pattern: Keep domain types unserialized, serialize only at storage boundary

**Key Learning**: We misunderstood the system - parameters update EVERY block, not just on special transactions.

---

### PM-19761: Expose LedgerParameters in GraphQL API (1 point) ✅ DONE!

**Status**: COMPLETED in PR #384

**Business Need:**
- **Simon's Request**: Created during implementation discussion - "expose parameters field on Block type"
- **Wallet Integration**: Wallet needs GraphQL access to current fee prices
- **Separation of Concerns**: Separate from PM-19727 as this is pure API work

**Description:**
Add GraphQL field to expose LedgerParameters on Block type.

**Acceptance Criteria:**
- New field `ledgerParameters` on Block GraphQL type
- Returns hex-encoded serialized parameters
- Efficient resolver implementation

**What was done**:
- Added `ledgerParameters` field to Block type (renamed from `parameters` per Simon)
- Implemented async resolver in `indexer-api/src/infra/api/v1/block.rs`
- Schema regenerated using `just generate-indexer-api-schema`

**Source References:**
- Simon's Slack request for GraphQL exposure
- `indexer-api/graphql/schema-v1.graphql` - field addition
- PR #385 - field renaming to `ledgerParameters`

---

### PM-19728: Calculate Transaction Cost Dimensions (8 points)

**What it does**: Extract the 5-dimensional cost breakdown for each transaction.

**Challenge**: The ledger calculates costs internally but doesn't expose them in transaction results.

**Current Ledger Code** (midnight-ledger/ledger/src/structure.rs:1700):
```rust
impl Transaction {
    pub fn cost(
        &self,
        params: &LedgerParameters,
        enforce_time_to_dismiss: bool,
    ) -> Result<SyntheticCost, FeeCalculationError> {
        // Calculates validation + application costs
        let validation_cost = self.validation_cost(params)?;
        let application_cost = self.application_cost(params)?;
        Ok(validation_cost + application_cost)
    }
}
```

**Implementation Options**:

**Option 1: Reconstruct in Indexer (Complex)**
```rust
// Would need to deserialize stored transaction and reconstruct
async fn calculate_transaction_cost(
    tx_raw: &[u8],
    params: &LedgerParameters,
) -> Result<SyntheticCost> {
    // Deserialize to ledger's Transaction type
    let tx: midnight_ledger::Transaction = deserialize(tx_raw)?;

    // Use ledger's cost calculation
    let cost = tx.cost(params, true)?;

    Ok(cost)
}
```

**Option 2: Node Modification (Preferred)**
Request node team to include cost in TransactionApplied event:
```rust
// In node's transaction processing
let cost = transaction.cost(&ledger_params)?;
events.push(TransactionApplied {
    hash,
    result,
    cost: Some(cost),  // NEW FIELD
});
```

**Option 3: Approximate from Fees**
```rust
// Simpler but less accurate - derive from fees paid
fn approximate_cost_from_fee(
    fee_paid: u128,
    fee_prices: &FeePrices,
) -> SyntheticCost {
    // This loses dimension breakdown
    // Only shows total, not individual dimensions
}
```

**Recommendation**: Coordinate with node team for Option 2, implement Option 3 as temporary solution

**Updated Complexity**: HIGH if Option 1, MODERATE if Option 2, LOW if Option 3

---

### PM-19729: Store Block Capacity Usage (5 points)

**What it does**: Aggregates all transaction costs per block to track capacity usage.

**Implementation**:
```rust
async fn calculate_block_capacity(
    block_id: u64,
    storage: &impl Storage,
) -> Result<()> {
    // Sum all transaction costs in block
    let transactions = storage.get_block_transactions(block_id).await?;
    let mut total_usage = SyntheticCost::zero();

    for tx in transactions {
        if let Some(cost) = storage.get_transaction_cost(tx.id).await? {
            total_usage = total_usage.add(cost);
        }
    }

    // Get limits from stored parameters
    let params = storage.get_block_parameters(block_id).await?;
    let limits = deserialize_limits(&params)?;

    // Calculate utilization
    let utilization = UtilizationMetrics {
        read_time_percent: (total_usage.read_time * 100) / limits.read_time,
        compute_time_percent: (total_usage.compute_time * 100) / limits.compute_time,
        // ... other dimensions
    };

    storage.save_block_capacity_usage(block_id, total_usage, utilization).await?;
}
```

**Depends on**: PM-19728 (need transaction costs first)

**Complexity**: MODERATE - Straightforward once transaction costs available

---

### PM-19730: Extend GraphQL API for Cost Model (5 points)

**What it does**: Exposes all cost data via GraphQL.

**New Schema Additions**:
```graphql
type Block {
    # ... existing fields ...
    ledgerParameters: HexEncoded  # ✅ Already done!
    capacityUsage: BlockCapacity  # New
}

type BlockCapacity {
    usage: CostDimensions!
    limits: CostDimensions!
    utilizationPercentages: UtilizationMetrics!
}

type CostDimensions {
    readTime: String!      # Picoseconds as string
    computeTime: String!
    blockUsage: String!    # Bytes
    bytesWritten: String!
    bytesChurned: String!
}

type UtilizationMetrics {
    readTimePercent: Float!
    computeTimePercent: Float!
    blockUsagePercent: Float!
    bytesWrittenPercent: Float!
    bytesChurnedPercent: Float!
    bottleneckDimension: String!  # Which dimension is limiting
}

type Transaction {
    # ... existing fields ...
    costBreakdown: TransactionCost
}

type TransactionCost {
    validationCost: CostDimensions!
    applicationCost: CostDimensions!
    totalCost: CostDimensions!
    feesPaid: String!
    feeSufficient: Boolean!
}

extend type Query {
    # Get parameters at specific block (already have data!)
    costModelAt(blockHeight: Int!): CostModelParameters

    # Analyze capacity trends
    capacityTrend(fromBlock: Int!, toBlock: Int!): [BlockCapacity!]!

    # Find bottleneck transactions
    highCostTransactions(
        blockHeight: Int!
        dimension: CostDimension!
        limit: Int
    ): [Transaction!]!
}

enum CostDimension {
    READ_TIME
    COMPUTE_TIME
    BLOCK_USAGE
    BYTES_WRITTEN
    BYTES_CHURNED
}
```

**Complexity**: MODERATE - Multiple new types but standard GraphQL patterns

---

### PM-19731: Add Fee Breakdown to Transaction Details (3 points)

**What it does**: Shows how fees map to each cost dimension.

**Implementation**:
```rust
fn calculate_fee_breakdown(
    cost: &SyntheticCost,
    prices: &FeePrices,
) -> FeeBreakdown {
    // Calculate fee per dimension
    let read_fee = multiply_fixed_point(cost.read_time, prices.read_price);
    let compute_fee = multiply_fixed_point(cost.compute_time, prices.compute_price);
    let block_fee = cost.block_usage * prices.block_usage_price;
    let write_fee = cost.bytes_written * prices.write_price;
    let churn_fee = cost.bytes_churned * prices.churn_price;

    // Find dominant dimension
    let fees = vec![
        ("read", read_fee),
        ("compute", compute_fee),
        ("block", block_fee),
        ("write", write_fee),
        ("churn", churn_fee),
    ];
    let dominant = fees.iter().max_by_key(|(_, fee)| fee).unwrap().0;

    FeeBreakdown {
        per_dimension: fees.into_iter().collect(),
        total: read_fee + compute_fee + block_fee + write_fee + churn_fee,
        dominant_dimension: dominant.to_string(),
    }
}
```

**Depends on**: PM-19728 (need cost breakdown)

**Complexity**: SIMPLE - Just calculation and display

---

### PM-19732: Cost Model + DUST Generation Integration (5 points)

**What it does**: Links calculated fees with actual DUST payments.

**Key Analysis**:
```rust
async fn analyze_fee_payment(
    tx_id: u64,
    storage: &impl Storage,
) -> Result<FeePaymentAnalysis> {
    // Get calculated cost
    let cost = storage.get_transaction_cost(tx_id).await?;
    let params = storage.get_block_parameters_for_tx(tx_id).await?;
    let calculated_fee = calculate_total_fee(&cost, &params.fee_prices);

    // Get actual DUST payment
    let dust_events = storage.get_dust_events_for_tx(tx_id).await?;
    let actual_payment = extract_dust_payment(&dust_events)?;

    // Compare
    Ok(FeePaymentAnalysis {
        calculated: calculated_fee,
        actual: actual_payment,
        overpayment: actual_payment.saturating_sub(calculated_fee),
        sufficient: actual_payment >= calculated_fee,
    })
}
```

**Integration Points**:
- Link transaction costs with DUST spend events
- Track overpayments and underpayments
- Identify transactions with insufficient fees

**Complexity**: MODERATE - Requires joining data from multiple sources

---

### PM-19733: Document Cost Model API (2 points)

**What it does**: Update existing API documentation with cost model features.

**Implementation**: Simply update `midnight-indexer/docs/api/v1/api-documentation.md` once all indexer changes are complete.

**Documentation to Add**:
- `ledgerParameters` field on Block type (already done)
- Transaction cost fields (once PM-19728 is implemented)
- Block capacity usage (once PM-19729 is implemented)
- Query examples for cost model data

**Example additions to api-documentation.md**:
   ```graphql
   # Get current fee prices
   query CurrentPrices {
     block(offset: { height: -1 }) {
       ledgerParameters  # Already available!
     }
   }

   # Check transaction cost (when implemented)
   query TransactionCost($hash: String!) {
     transaction(hash: $hash) {
       costBreakdown {
         totalCost {
           computeTime
           blockUsage
         }
         feesPaid
         feeSufficient
       }
     }
   }
   ```

**Complexity**: SIMPLE - Standard documentation

## Revised Implementation Order

### Phase 0: Already Complete! ✅
- PM-19727: Extract parameters (DONE in PR #382)
- PM-19761: Expose in GraphQL (DONE in PR #382)

### Phase 1: Foundation (3-4 days)
1. PM-19726: Add remaining tables for cost tracking

### Phase 2: Core Challenge (1-2 weeks)
2. PM-19728: Calculate transaction costs (highest risk)
   - Spike: Try Option 1 (reconstruction)
   - Fallback: Implement Option 3 (approximation)
   - Ideal: Coordinate Option 2 (node modification)

### Phase 3: Aggregation & Analysis (1 week)
3. PM-19729: Block capacity tracking
4. PM-19731: Fee breakdown
5. PM-19732: DUST integration

### Phase 4: API & Docs (3-4 days)
6. PM-19730: Complete GraphQL API
7. PM-19733: Documentation

## Updated Risk Analysis

### Reduced Risks
- ✅ Parameter extraction already solved
- ✅ No RPC needed for parameters
- ✅ GraphQL foundation in place

### Remaining High Risk
- **PM-19728**: Transaction cost calculation
  - May require node changes
  - Or complex transaction reconstruction
  - Fallback: Use fee approximation

### Mitigation Strategy
1. **Immediate Spike**: Test transaction reconstruction (1-2 days)
2. **Early Node Engagement**: If needed, request cost in events
3. **Phased Delivery**:
   - Phase 1: Parameters only (DONE)
   - Phase 2: Fee tracking without breakdown
   - Phase 3: Full cost dimension analysis

## Updated Time Estimates

### Already Delivered
- 2 tickets (PM-19727, PM-19761): 1 story point, ~1 hour

### Remaining Work
- **Optimistic**: 2-3 weeks if transaction reconstruction works
- **Realistic**: 3-4 weeks including node coordination
- **Conservative**: 4-5 weeks if complex reconstruction needed

### MVP Options
1. **Minimal** (1 week): Just track fees paid vs parameters
2. **Standard** (2 weeks): Add capacity tracking
3. **Complete** (3-4 weeks): Full dimension breakdown

## Key Technical Insights

### Why Parameters Update Every Block

**Source**: Discovered during Simon's Slack discussion (Sept 29) and confirmed in code:
- `midnight-ledger/ledger/src/semantics.rs:902-920` - post_block_update() implementation
- `midnight-ledger/ledger/src/structure.rs:3500-3525` - FeePrices::adjust() method
- `midnight-ledger/spec/cost-model.md` - Section on "Dynamic Fee Adjustment"

```rust
// From: midnight-ledger/ledger/src/semantics.rs:902-920
// Called after EVERY block since ledger v6.1.0
pub fn post_block_update(
    mut self,
    timestamp: Timestamp,
    block_fullness: Fraction,  // How full was the block
) -> Result<Self> {
    let adjusted_fee_prices = self.parameters.fee_prices.adjust(
        self.parameters.price_adjustment_parameter,  // Speed of adjustment
        block_fullness,                             // Current utilization
        self.parameters.cost_dimension_min_ratio,   // Floor prices
    );
    // ... parameters updated with new prices
}

// From: midnight-ledger/ledger/src/structure.rs:3515
// The actual adjustment calculation
let adjustment_factor = calculate_adjustment(
    block_fullness,     // Actual usage / capacity
    target_fullness,    // 0.5 (50% target)
    adjustment_speed    // How fast to adjust
);
```

This maintains optimal block utilization by:
- Increasing fees when blocks are full (fullness > 50%)
- Decreasing fees when blocks are empty (fullness < 50%)
- Adjusting each dimension independently based on usage

### Transaction Cost Structure

**Source**: Analyzed from multiple ledger files:
- `midnight-ledger/ledger/src/structure.rs:1700-1750` - Transaction::cost() method
- `midnight-ledger/ledger/src/structure.rs:1800-1900` - validation_cost() and application_cost()
- `midnight-ledger/spec/cost-model.md` - Section 3: "Transaction Cost Components"
- `midnight-ledger/base-crypto/src/cost_model.rs:50-200` - Cost calculation implementations

```rust
// From: midnight-ledger/ledger/src/structure.rs:1700
pub fn cost(
    &self,
    params: &LedgerParameters,
    enforce_time_to_dismiss: bool,
) -> Result<SyntheticCost, FeeCalculationError> {
    let validation_cost = self.validation_cost(params)?;
    let application_cost = self.application_cost(params)?;
    Ok(validation_cost + application_cost)
}

// Cost Components discovered from code analysis:
Total Cost = Validation Cost + Application Cost

Validation Cost (from validation_cost() method):
- Signature verification (compute_time) - cryptographic operations
- Input validation (read_time) - reading UTXOs from state
- Well-formedness checks (compute_time) - structure validation

Application Cost (from application_cost() method):
- State updates (bytes_written) - new UTXOs, contract state
- Contract execution (compute_time) - ZkVM execution
- Temporary storage (bytes_churned) - intermediate computations
```

### Fee Calculation Formula

**Source**: Formula extracted from:
- `midnight-ledger/ledger/src/structure.rs:3400-3450` - FeePrices::fee() method
- `midnight-ledger/base-crypto/src/cost_model.rs:250-300` - fee calculation logic
- `midnight-ledger/spec/cost-model.md` - Section 5: "Fee Calculation"
- Heiko's explanation in Slack thread (Sept 29): "fees are the max across all dimensions"

```rust
// From: midnight-ledger/ledger/src/structure.rs:3420
impl FeePrices {
    pub fn fee(&self, cost: &SyntheticCost) -> u128 {
        // Take the MAXIMUM fee across all dimensions
        let read_fee = multiply_fixed_point(cost.read_time, self.read_price);
        let compute_fee = multiply_fixed_point(cost.compute_time, self.compute_price);
        let block_fee = cost.block_usage * self.block_usage_price;
        let write_fee = cost.bytes_written * self.write_price;
        let churn_fee = cost.bytes_churned * self.churn_price;

        // User pays the MAX, not the sum!
        max(read_fee, compute_fee, block_fee, write_fee, churn_fee)
    }
}

// Conceptual formula (but actual implementation uses MAX):
Fee = MAX(
    read_time * read_price,
    compute_time * compute_price,
    block_usage * block_price,
    bytes_written * write_price,
    bytes_churned * churn_price
)

// This ensures users pay for the "bottleneck" resource
// preventing any single dimension from becoming a spam vector
```

## Dependencies & Coordination

### What We Have
- ✅ Ledger v6.1.0 with cost model
- ✅ Parameters in every block
- ✅ DUST event tracking
- ✅ GraphQL infrastructure

### What We Need
- Transaction cost data (from node or reconstruction)
- Possible node event enhancement

### Teams to Coordinate
- **Node Team**: For transaction cost exposure
- **Wallet Team**: To consume new API
- **DevOps**: For migration deployment

## Lessons Learned from Implementation

### Key Discoveries:
1. **Dynamic Fees**: LedgerParameters change EVERY BLOCK, not just on special transactions
2. **No RPC Needed**: Parameters already available in LedgerState
3. **Fee Calculation**: Uses MAX across dimensions, not SUM
4. **Block Fullness**: Accumulated during transaction processing, used for next block's fees

### Team Contributions:
- **Heiko**: Refactored to return LedgerParameters from post_apply_transactions, enforced domain patterns
- **Simon**: Requested field renaming to `ledgerParameters`, created PM-19761 ticket
- **Agron**: Provided context on cost model design in Slack discussions

## Conclusion

The discovery that LedgerParameters are already available dramatically simplifies the implementation. We've already delivered 2 tickets with minimal effort.

The main remaining challenge is PM-19728 (transaction cost calculation), which drives most other features. Once we solve that - either through reconstruction or node modification - the remaining implementation is straightforward.

**Story Points Summary**:
- **Original 8 tickets total**: 32 story points
  - PM-19726: 3 pts
  - PM-19727: 1 pt ✅ (DONE)
  - PM-19728: 8 pts
  - PM-19729: 5 pts
  - PM-19730: 5 pts
  - PM-19731: 3 pts
  - PM-19732: 5 pts
  - PM-19733: 2 pts
- **Additional ticket**: PM-19761: 1 pt ✅ (DONE)
- **Total delivered**: 2 story points
- **Remaining**: 30 story points
- **Timeline**: 3-4 weeks for complete implementation

The indexer is well-positioned to support the cost model, with most infrastructure already in place.