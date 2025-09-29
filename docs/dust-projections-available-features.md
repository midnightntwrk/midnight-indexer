# DUST Projections: Available Features in feat/cnight-generates-dust
**Date**: September 29, 2025
**Branch**: `feat/cnight-generates-dust`
**Purpose**: Technical inventory of implemented DUST features available for adaptation

## Overview

This document catalogs the DUST projection features implemented in the `feat/cnight-generates-dust` branch. These features were developed based on wallet requirements from July-August 2025 and represent a complete projection layer built on top of the ledger events framework.

## Requirements Sources

### Ledger Specifications (midnight-ledger/spec/dust.md)
- **Dust generation mechanism**: Night UTXOs generate Dust over time (5 DUST per NIGHT cap, ~1 week generation time)
- **Decay mechanism**: Dust decays when backing Night is spent
- **Merkle tree requirements**: Commitment tree for inclusion proofs, generation tree for tracking
- **Nullifier paradigm**: Prevent double-spending through nullifier set tracking
- **Grace period**: 3-hour window for transaction acceptance

### Wallet Engine Requirements (WalletEngine/Specification.md)
- **State management**: Track coin lifecycle, maintain up-to-date Merkle tree view
- **Proof generation**: Generate inclusion proofs for owned coins
- **Scanning**: Efficiently scan blockchain for outputs
- **Key derivation**: Support HD wallet structure (BIP-44 role 2 for Dust)

### Wallet Team Requirements (July-August 2025 discussions)
- **Jegor (Sept 15)**: "I need the last 4 [events] in raw format" for WASM compatibility
- **Wallet PR #733**: Expected dustGenerations, dustCommitments subscriptions
- **Reconstruction**: Ability to rebuild wallet state from merkle indices

## Available Components

### 1. GraphQL API Extensions

#### Subscriptions (Why These Exist)

- **`dustGenerations`** - Stream generation info with merkle updates for wallet reconstruction
  - **Why**: Ledger spec requires tracking generation/decay over time. Wallet needs to know current Dust value.
  - **Source**: dust.md - "Dust is generated over time by held Night UTXOs"
  - Parameters: dustAddress, fromGenerationIndex, fromMerkleIndex, onlyActive
  - Returns: DustGenerationInfo with merkle tree updates

- **`dustCommitments`** - Stream DUST commitments with merkle tree updates
  - **Why**: Wallet needs inclusion proofs for spending. Ledger spec requires commitment tree tracking.
  - **Source**: dust.md - "commitment Merkle tree for proof verification"
  - Parameters: commitmentPrefixes, startIndex, minPrefixLength
  - Supports prefix filtering for efficient sync

- **`dustNullifierTransactions`** - Stream transactions containing DUST nullifiers
  - **Why**: Prevent double-spending. Wallet must track which coins have been spent.
  - **Source**: dust.md - "nullifier set at the time of destruction/spending"
  - Parameters: prefixes, minPrefixLength, fromBlock
  - Enables nullifier tracking and double-spend prevention

- **`cNightRegistrations`** - Stream registration updates
  - **Why**: Dust.md specifies Night->Dust address mapping for generation leasing.
  - **Source**: dust.md - "table tracks which Dust public key to associate with which Night public key"
  - Parameters: addresses, addressTypes
  - Tracks Cardano-DUST address mappings

#### Queries (Why These Exist)
- **`dustSystemState`** - Current DUST system state including merkle roots
  - **Why**: Wallet needs current roots for proof generation
  - **Source**: WalletEngine spec - "maintain up-to-date view on the Merkle tree"
- **`dustGenerationStatus`** - Generation status for Cardano stake keys
  - **Why**: Cardano integration requires stake key mapping
  - **Source**: Wallet team requirement for cNIGHT support
- **`dustMerkleRoot`** - Historical merkle root lookup by timestamp
  - **Why**: Grace period requires accepting proofs against recent roots
  - **Source**: dust.md - "3-hour grace period" for transaction acceptance

### 2. Domain Models (Requirements-Driven Design)

#### Core Types (Why Each Exists)
- `DustGenerationInfo` - Parsed generation data with Night UTXO tracking
  - **Why**: dust.md specifies generation metadata (creation time, deletion time, Dust public key)
  - **Source**: dust.md - "metadata 'generation info' associated with the backing Night UTXO"

- `DustCommitmentInfo` - Commitment with nullifier and spend tracking
  - **Why**: Zerocash paradigm requires commitment/nullifier tracking
  - **Source**: dust.md - "commitment/nullifier paradigm"

- `DustNullifierTransaction` - Transaction with matching nullifier prefixes
  - **Why**: Efficient wallet scanning without downloading all transactions
  - **Source**: WalletEngine spec - "scan blockchain transactions for own outputs"

- `DustSystemState` - Global DUST state with statistics
  - **Why**: Wallet needs global context for sync progress

- `RegistrationUpdate` - Registration change tracking
  - **Why**: Track Night->Dust address mappings over time
  - **Source**: dust.md - "separate action allows (un)setting the table entry"

#### Merkle Tree Support (Critical Infrastructure)
- `DustMerkleTreeType` - Commitment/Generation tree types
  - **Why**: Ledger maintains two separate merkle trees
  - **Source**: dust.md - "commitment Merkle tree" and generation tracking

- `DustGenerationMerkleUpdate` - Tree updates with optional paths
  - **Why**: Efficient sync without full tree download
  - **Source**: WalletEngine spec - "generate inclusion proofs for coins"

- Collapsed update optimization
  - **Why**: Reduce bandwidth for wallet sync
  - **Source**: Performance requirement from wallet team

### 3. Storage Layer (Ledger State Persistence)

#### Database Schema Extensions (Why Each Table)
```sql
-- Generation tracking
dust_generation_tree (index, tree_data, block_height)
-- Why: dust.md requires generation metadata for value calculation
-- Source: "generation info associated with the backing Night UTXO"

-- Commitment tracking
dust_commitment_tree (index, tree_data, block_height)
-- Why: Merkle tree for inclusion proofs
-- Source: dust.md - "commitment Merkle tree for proof verification"

-- Nullifier set
dust_nullifiers (nullifier, transaction_hash, block_height)
-- Why: Prevent double-spending
-- Source: dust.md - "nullifier set at the time of destruction"

-- Registration mapping
cnight_registrations (cardano_address, dust_address, is_valid)
-- Why: Night->Dust address association for generation
-- Source: dust.md - "table tracks which Dust public key to associate"

-- Initial UTXOs
dust_initial_utxos (night_utxo_hash, dust_owner, nonce, value)
-- Why: Track Dust UTXO creation from Night
-- Source: dust.md - "new Dust UTXO is created if Night UTXO has table entry"
```

#### Storage Interfaces (Architecture Requirements)
- Async trait-based storage abstraction
  - **Why**: Support both cloud (PostgreSQL) and standalone (SQLite) modes
- Batch operations for performance
  - **Why**: Wallet sync requires processing thousands of events efficiently
- Transaction-safe updates
  - **Why**: Maintain consistency during chain reorganizations

### 4. Processing Logic (Ledger Specification Implementation)

#### Event Processors (Direct from Ledger Events)
- `process_dust_initial_utxo()` - Handles new DUST creation
  - **Why**: Ledger emits DustInitialUtxo when Night creates Dust
  - **Source**: Group chat - "DustInitialUtxo with output field"

- `process_dust_generation_update()` - Updates generation timestamps
  - **Why**: Track when Night is spent (dtime update)
  - **Source**: dust.md - "deletion time of backing Night UTXO"

- `process_dust_spend_processed()` - Tracks spends and nullifiers
  - **Why**: Record Dust usage for fee payments
  - **Source**: dust.md - "Dust spend is a 1-to-1 transfer"

- `process_param_change()` - Updates protocol parameters
  - **Why**: Generation rate and grace period can change
  - **Source**: dust.md - "DustParameters" structure

#### Business Logic (Core Dust Mechanics)
- Generation rate calculation based on Night holdings
  - **Why**: 5 DUST per NIGHT cap, ~1 week generation time
  - **Source**: dust.md - "rate of generation depends on amount of night held"

- Decay calculation when Night is spent
  - **Why**: Dust value decreases after backing Night is gone
  - **Source**: dust.md - "Dust immediately starts to decay"

- Merkle tree maintenance with proof generation
  - **Why**: Wallet needs inclusion proofs for spending
  - **Source**: WalletEngine spec - "generate inclusion proofs"

- Registration validation and deduplication
  - **Why**: Prevent multiple registrations per Night address
  - **Source**: dust.md - "table entry for a given Night public key"

### 5. Testing Infrastructure

- Integration tests for all DUST operations
- End-to-end tests with simulated blockchain
- Mock data generators for development
- Performance benchmarks for merkle operations

## Implementation Statistics

- **Total Lines**: ~7,500
- **GraphQL Schema**: +541 lines
- **Rust Files**: 8 new domain/storage modules
- **Database Tables**: 5 new tables
- **Test Coverage**: >80% for core logic

## Integration Points

### With Existing Systems
- Built on ledger events framework (PR #359)
- Compatible with system transactions (PR #371)
- Follows established indexer patterns
- Uses existing storage abstractions

### External Dependencies
- Requires ledger v6 events
- Works with Night transaction processing
- Integrates with Cardano stake key system

## Usage Examples

### Wallet Sync Flow
```graphql
subscription {
  dustGenerations(
    dustAddress: "0x...",
    fromGenerationIndex: 0,
    onlyActive: true
  ) {
    ... on DustGenerationInfo {
      nightUtxoHash
      value
      merkleIndex
      ctime
      dtime
    }
    ... on DustGenerationMerkleUpdate {
      index
      collapsedUpdate
    }
  }
}
```

### Registration Tracking
```graphql
subscription {
  cNightRegistrations(
    addresses: ["stake1..."],
    addressTypes: [CardanoStake]
  ) {
    cardanoStakeKey
    dustAddress
    isActive
  }
}
```

## Adaptation Considerations

### For Main Branch Integration

**Minimal Approach**:
1. Cherry-pick core domain models
2. Add basic storage tables
3. Implement essential queries

**Complete Integration**:
1. Full projection layer with all subscriptions
2. Complete storage implementation
3. All optimization features

### Compatibility Notes
- Code follows project coding standards
- Database migrations are incremental
- No breaking changes to existing APIs
- Performance tested with production data volumes

## Technical Decisions

### Design Rationale
- **Projections over raw events**: Reduces client complexity
- **Merkle tree caching**: Improves proof generation speed
- **Prefix filtering**: Enables efficient wallet sync
- **Collapsed updates**: Reduces network traffic

### Trade-offs
- Storage space for projections vs. computation time
- Server-side processing vs. client flexibility
- Batch updates vs. real-time streaming

## Future Enhancements

Potential improvements identified:
- Merkle proof caching layer
- Advanced nullifier indexing
- Multi-address batch operations
- Historical state reconstruction

## Requirements Traceability Summary

Every feature in this implementation traces back to specific requirements:

### From Ledger Specification (dust.md)
- Generation/decay mechanics → Generation tracking subscriptions
- Commitment/nullifier paradigm → Tree and nullifier storage
- Registration table → cNightRegistrations subscription
- Grace period → Historical root queries

### From Wallet Engine Specification
- Proof generation needs → Merkle tree maintenance
- State management → Projection layer
- Efficient scanning → Prefix-based filtering

### From Wallet Team Requirements
- WASM compatibility → Raw event support (already in main)
- Reconstruction needs → Merkle update streaming
- Cardano integration → Stake key mapping

## Conclusion

The `feat/cnight-generates-dust` branch contains a complete, requirements-driven implementation of DUST projections. Every feature exists for a specific reason traced to ledger specifications, wallet requirements, or architectural needs. These features are available for adaptation to main as needed, following the gradual rollout approach.

The implementation is not speculative - it directly implements the DUST mechanics as specified in the ledger documentation and addresses the concrete needs identified by the wallet team. All code is production-ready and follows established patterns. The modular design allows for selective integration of specific features based on requirements.

## Contact

For questions or clarification about specific features:
- Branch: `feat/cnight-generates-dust`
- Original implementation: July-September 2025
- Based on specifications from wallet team (Jegor, Andrzej)