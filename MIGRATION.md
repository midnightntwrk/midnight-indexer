# SPO Services Migration Documentation

## Overview

This document details the migration of SPO (Stake Pool Operator) services from the `midnight-indexer-spo-extension` repository into the main `midnight-indexer` repository (v3.0.0-alpha.9).

**Migration Date**: November 17, 2025
**Source Repository**: midnight-indexer-spo-extension (based on midnight-indexer from months ago)
**Target Repository**: midnight-indexer v3.0.0-alpha.9
**Target Network**: Midnight Preview Network (`wss://rpc.preview.midnight.network`)

## Background

The SPO extension was originally developed on an older version of midnight-indexer. The main midnight-indexer repository has since been updated with 100+ commits, including critical changes for Preview network compatibility. The migration was necessary to:

1. Get latest improvements and bug fixes from upstream
2. Support the Midnight Preview network (previous version only supported older dev networks)
3. Resolve NetworkId compatibility issues that prevented connection to Preview network

### Key Version Changes

| Component | Old Version (spo-extension) | New Version (midnight-indexer) |
|-----------|----------------------------|--------------------------------|
| midnight-ledger | alpha.2 | alpha.5 |
| async-nats | 0.42 | 0.45 |
| NetworkId | Enum-based | String-based wrapper |

## Migration Strategy

**Approach Selected**: Integrate SPO services into midnight-indexer (Option A)

**Rationale**:
- Midnight-indexer is the canonical upstream repository
- Easier to maintain going forward
- Access to latest improvements and Preview network support
- NetworkId changes in midnight-indexer v3.0.0 required for Preview network

## Phase 1: Repository Setup

### 1.1 Branch Creation
```bash
git checkout -b feature/integrate-spo-services
```

### 1.2 Service Directories Copied
From `midnight-indexer-spo-extension` to `midnight-indexer`:
- `spo-indexer/` - Complete directory (38 files)
- `spo-api/` - Complete directory

## Phase 2: Workspace Configuration

### 2.1 Updated Root Cargo.toml

**Added Workspace Members**:
```toml
members = [
    # ... existing members
    "spo-indexer",
    "spo-api",
]
```

**Added Workspace Dependencies**:
```toml
[workspace.dependencies]
blake2 = { version = "0.10.6" }
blockfrost = { version = "1.1.0" }
hex = { version = "0.4.3" }
once_cell = { version = "1.19" }
paste = { version = "1.0" }
regex = { version = "1.11" }
```

### 2.2 Updated Service Dependencies

**Files Modified**:
- `spo-indexer/Cargo.toml`
- `spo-api/Cargo.toml`

**Changes**: Updated dependencies to use workspace versions:
```toml
blake2 = { workspace = true }
blockfrost = { workspace = true }
hex = { workspace = true }
once_cell = { workspace = true }
paste = { workspace = true }
regex = { workspace = true }
```

## Phase 3: Database Migrations

### 3.1 Migration Files Copied

Copied from `spo-extension/indexer-common/migrations/postgres/` to `midnight-indexer/indexer-common/migrations/postgres/`:

1. **002_spo_initial.sql** (84 lines)
   - Creates `epochs` table
   - Creates `pool_metadata_cache` table
   - Creates `spo_identity` table
   - Creates `committee_membership` table
   - Creates `spo_epoch_performance` table
   - Creates `spo_history` table

2. **003_drop_stg_committee.sql** (4 lines)
   - Drops staging table

3. **004_spo_stake.sql** (17 lines)
   - Creates `spo_stake_snapshot` table

4. **005_spo_stake_history.sql** (29 lines)
   - Creates `spo_stake_history` table
   - Creates `spo_stake_refresh_state` table

## Phase 4: Docker Configuration

### 4.1 Updated docker-compose.yaml

**Added Services**:

```yaml
spo-indexer:
  profiles:
    - cloud
  depends_on:
    postgres:
      condition: "service_healthy"
    nats:
      condition: "service_started"
  build:
    context: .
    dockerfile: spo-indexer/Dockerfile
  image: "spo-indexer:local"
  restart: "no"
  environment:
    RUST_LOG: "spo_indexer=debug,indexer_common=debug,fastrace_opentelemetry=off,info"
    APP__APPLICATION__NETWORK_ID: "preview"
    APP__INFRA__NODE__URL: "wss://rpc.preview.midnight.network"
    APP__INFRA__NODE__BLOCKFROST_ID: $APP__INFRA__NODE__BLOCKFROST_ID
    APP__INFRA__STORAGE__HOST: "postgres"
    APP__INFRA__STORAGE__PASSWORD: $APP__INFRA__STORAGE__PASSWORD
    APP__INFRA__PUB_SUB__URL: "nats:4222"
    APP__INFRA__PUB_SUB__PASSWORD: $APP__INFRA__PUB_SUB__PASSWORD
  healthcheck:
    test: ["CMD-SHELL", "cat /var/run/spo-indexer/running || exit 0"]

spo-api:
  profiles:
    - cloud
  depends_on:
    postgres:
      condition: "service_healthy"
    nats:
      condition: "service_started"
  build:
    context: .
    dockerfile: spo-api/Dockerfile
  image: "spo-api:local"
  restart: "no"
  ports:
    - "8090:8090"
  environment:
    RUST_LOG: "spo_api=debug,indexer_common=debug,fastrace_opentelemetry=off,info"
    APP__APPLICATION__NETWORK_ID: "preview"
    APP__INFRA__STORAGE__HOST: "postgres"
    APP__INFRA__STORAGE__PASSWORD: $APP__INFRA__STORAGE__PASSWORD
    APP__INFRA__API__PORT: "8090"
    APP__INFRA__API__MAX_COMPLEXITY: "2000"
    APP__INFRA__API__MAX_DEPTH: "50"
  healthcheck:
    test: ["CMD-SHELL", "cat /var/run/spo-api/running || exit 0"]
```

**Key Changes from Original**:
- Changed from pulling pre-built images to local builds
- Updated network configuration to use Preview network
- Updated database user from "postgres" to "indexer" for consistency

### 4.2 Environment Variables

**Updated .envrc.local**:
```bash
export APP__INFRA__NODE__BLOCKFROST_ID="previewukkFxumNW31cXmsBtKI1JTnbxvcVCbCj"
export APP__INFRA__STORAGE__PASSWORD="indexer"
export APP__INFRA__PUB_SUB__PASSWORD="indexer"
```

## Phase 5: Configuration Updates

### 5.1 SPO Indexer Configuration

**File**: `spo-indexer/config.yaml`

**Changes**:
```yaml
application:
  network_id: "preview"  # Changed from "Undeployed"

infra:
  storage:
    user: "indexer"  # Changed from "postgres"

  node:
    url: "wss://rpc.preview.midnight.network"  # Changed from dev network
```

### 5.2 SPO API Configuration

**File**: `spo-api/config.yaml`

**Changes**:
```yaml
application:
  network_id: "preview"  # Changed from "Undeployed"
```

## Phase 6: Code Compatibility Fixes

### 6.1 NetworkId Type Change

**Issue**: NetworkId changed from `Copy` trait enum to String-based wrapper (non-Copy)

**Files Modified**:

1. **spo-api/src/application.rs:15**
   ```rust
   // BEFORE
   #[derive(Debug, Clone, Copy, Deserialize)]
   pub struct Config {
       pub network_id: NetworkId,
   }

   // AFTER
   #[derive(Debug, Clone, Deserialize)]  // Removed Copy
   pub struct Config {
       pub network_id: NetworkId,
   }
   ```

2. **spo-api/src/infra/api/mod.rs:191**
   ```rust
   // BEFORE
   fn get_network_id(&self) -> NetworkId {
       self.data::<NetworkId>()
           .copied()
           .expect("NetworkId is stored in Context")
   }

   // AFTER
   fn get_network_id(&self) -> NetworkId {
       self.data::<NetworkId>()
           .cloned()  // Changed from .copied()
           .expect("NetworkId is stored in Context")
   }
   ```

### 6.2 Preview Network API Compatibility

**Issue**: Midnight Preview network RPC API changed between alpha.2 and alpha.5

#### Change 1: `auraPubKey` Field Removed

**Files Modified**:

1. **spo-indexer/src/domain/rpc.rs:94-108**
   ```rust
   // BEFORE
   pub struct CandidateRegistration {
       pub sidechain_pub_key: String,
       pub sidechain_account_id: String,
       pub mainchain_pub_key: String,
       pub cross_chain_pub_key: String,
       pub aura_pub_key: String,
       pub grandpa_pub_key: String,
       // ... rest of fields
   }

   // AFTER
   pub struct CandidateRegistration {
       pub sidechain_pub_key: String,
       pub sidechain_account_id: String,
       pub mainchain_pub_key: String,
       pub cross_chain_pub_key: String,
       #[serde(default)]
       pub aura_pub_key: Option<String>,  // Made optional
       #[serde(default)]
       pub grandpa_pub_key: Option<String>,  // Made optional
       // ... rest of fields
   }
   ```

2. **spo-indexer/src/domain/rpc.rs:120-126** (Display impl)
   ```rust
   // BEFORE
   writeln!(f, "      Aura Pub Key: {}", self.aura_pub_key)?;
   writeln!(f, "      Grandpa Pub Key: {}", self.grandpa_pub_key)?;

   // AFTER
   if let Some(aura_key) = &self.aura_pub_key {
       writeln!(f, "      Aura Pub Key: {}", aura_key)?;
   }
   if let Some(grandpa_key) = &self.grandpa_pub_key {
       writeln!(f, "      Grandpa Pub Key: {}", grandpa_key)?;
   }
   ```

3. **spo-indexer/src/infra/subxt_node.rs:191-192**
   ```rust
   // BEFORE
   aura_pub_key: remove_hex_prefix(reg.aura_pub_key),
   grandpa_pub_key: remove_hex_prefix(reg.grandpa_pub_key),

   // AFTER
   aura_pub_key: reg.aura_pub_key.map(remove_hex_prefix),
   grandpa_pub_key: reg.grandpa_pub_key.map(remove_hex_prefix),
   ```

4. **spo-indexer/src/application.rs:266**
   ```rust
   // BEFORE
   let aura_pk = remove_hex_prefix(raw_spo.aura_pub_key.to_string());

   // AFTER
   let aura_pk = raw_spo.aura_pub_key.as_ref()
       .map(|k| remove_hex_prefix(k.to_string()))
       .unwrap_or_default();
   ```

## Phase 7: Build and Testing

### 7.1 Compilation

**Command**:
```bash
source .envrc.local && docker compose --profile cloud build spo-indexer spo-api
```

**Results**:
- ✅ spo-indexer builds successfully
- ✅ spo-api builds successfully
- Build time: ~4 minutes (first build), ~3 minutes (incremental)

### 7.2 Container Startup

**Command**:
```bash
source .envrc.local && docker compose up -d postgres nats spo-indexer spo-api
```

**Results**:
- ✅ postgres: Started and healthy
- ✅ nats: Started
- ✅ spo-api: Started and healthy (port 8090)
- ✅ spo-indexer: Started successfully

### 7.3 Initial Testing Results

**Successful Operations**:
1. ✅ Connected to Preview network RPC (`wss://rpc.preview.midnight.network`)
2. ✅ Created database connection pool
3. ✅ Applied database migrations
4. ✅ Successfully processed epoch 979338
5. ✅ Started processing epoch 979339

**Sample Log Output**:
```json
{"timestamp":"2025-11-17T23:05:44.259759+00:00","level":"INFO","target":"spo_indexer","file":"spo-indexer/src/main.rs","line":52,"message":"starting"}
{"timestamp":"2025-11-17T23:05:48.341566+00:00","level":"DEBUG","target":"indexer_common::infra::pool::postgres","file":"/build/indexer-common/src/infra/pool/postgres.rs","line":60,"message":"created pool"}
processing epoch 979338
processed epoch 979338
processing epoch 979339
```

## Current Status

### ✅ Completed

1. Repository structure migrated
2. Workspace configuration updated
3. Database migrations integrated
4. Docker configuration updated
5. Configuration files updated for Preview network
6. NetworkId compatibility fixes applied
7. Preview network API compatibility fixes (aura_pub_key, grandpa_pub_key)
8. Successful compilation of both services
9. Successful connection to Preview network
10. Successfully processing SPO registration data from Preview network

### ⚠️ Known Issues

#### Issue #1: Missing RPC Method `sidechain_getEpochCommittee`

**Error**:
```json
{"timestamp":"2025-11-17T23:05:51.417863+00:00","level":"ERROR","target":"spo_indexer","file":"spo-indexer/src/main.rs","line":31,"message":"process exited with ERROR","kvs":{"backtrace":"disabled backtrace","error":"cannot make rpc call: sidechain_getEpochCommittee"}}
```

**Root Cause**: The `sidechain_getEpochCommittee` RPC method does not exist in the Midnight Preview network API (likely removed or renamed between alpha.2 and alpha.5).

**Impact**:
- spo-indexer processes a few epochs successfully
- Crashes when it tries to fetch committee information
- Prevents continuous operation

**Location**: `spo-indexer/src/infra/subxt_node.rs:220`

**Current Implementation**:
```rust
pub async fn get_committee(&self, epoch_number: u32) -> Result<Vec<Validator>, SPOClientError> {
    let rpc_params = RawValue::from_string(format!("[{}]", epoch_number))?;

    loop {
        let raw_response = self
            .rpc_client
            .request(
                "sidechain_getEpochCommittee".to_string(),  // This method doesn't exist
                Some(rpc_params.clone()),
            )
            .await
            .map_err(|_| SPOClientError::RpcCall("sidechain_getEpochCommittee".to_string()))?;
        // ...
    }
}
```

**Potential Solutions**:

1. **Option A - Find Alternative RPC Method**:
   - Research Midnight Preview network API documentation
   - Find the new method name for fetching epoch committee
   - Update the RPC call

2. **Option B - Derive from Alternative Data**:
   - Check if committee information is available through `sidechain_getAriadneParameters` response
   - Extract committee from candidate registrations if possible

3. **Option C - Make Committee Optional**:
   - Modify application logic to handle missing committee data
   - Skip committee-related operations if API unavailable
   - **Note**: This may impact functionality that depends on committee data

**Recommended Next Steps**:
1. Research Midnight Preview network RPC API documentation
2. Check if there's an alternative method to get committee data
3. Test if the application can function without committee data
4. If committee data is optional, implement graceful degradation

## Testing Checklist

### Completed Tests
- [x] Docker build succeeds for spo-indexer
- [x] Docker build succeeds for spo-api
- [x] Containers start without errors
- [x] Database connection established
- [x] Database migrations apply successfully
- [x] Connection to Preview network RPC successful
- [x] SPO registration data fetching works
- [x] Epoch processing works (at least partially)

### Pending Tests
- [ ] Full epoch processing without errors
- [ ] Committee data retrieval (blocked by missing RPC)
- [ ] Pool metadata fetching from Blockfrost
- [ ] spo-api GraphQL queries
- [ ] Stake refresh functionality
- [ ] End-to-end data flow from indexer to API

## Environment Setup

### Required Environment Variables

```bash
# Database
export APP__INFRA__STORAGE__PASSWORD="indexer"

# NATS
export APP__INFRA__PUB_SUB__PASSWORD="indexer"
export APP__INFRA__LEDGER_STATE_STORAGE__PASSWORD="indexer"

# Blockfrost (for Cardano pool metadata)
export APP__INFRA__NODE__BLOCKFROST_ID="previewukkFxumNW31cXmsBtKI1JTnbxvcVCbCj"

# Optional: Encryption secret for wallet indexer
export APP__INFRA__SECRET="303132333435363738393031323334353637383930313233343536373839303132"
```

### Running Services

**Start all services**:
```bash
source .envrc.local && docker compose --profile cloud up -d
```

**Start only SPO services**:
```bash
source .envrc.local && docker compose up -d postgres nats spo-indexer spo-api
```

**View logs**:
```bash
# All logs
docker compose logs -f

# SPO Indexer only
docker compose logs -f spo-indexer

# SPO API only
docker compose logs -f spo-api
```

**Rebuild after code changes**:
```bash
source .envrc.local && docker compose build spo-indexer spo-api
source .envrc.local && docker compose up -d spo-indexer spo-api
```

## API Endpoints

### SPO API
- **GraphQL Endpoint**: http://localhost:8090/api/v1/graphql
- **GraphQL Playground**: http://localhost:8090/api/v1/playground
- **Health Check**: http://localhost:8090/ready

### Indexer API (if running full stack)
- **GraphQL Endpoint**: http://localhost:8088/api/v1/graphql
- **Health Check**: http://localhost:8088/ready

## Files Modified Summary

### New Files
- None (all files copied from spo-extension)

### Modified Files

| File | Lines Changed | Purpose |
|------|---------------|---------|
| `Cargo.toml` | +8 | Added workspace members and dependencies |
| `docker-compose.yaml` | +60 | Added spo-indexer and spo-api services |
| `.envrc.local` | +3 | Added Blockfrost ID and credentials |
| `spo-indexer/config.yaml` | 3 | Updated network_id, user, RPC URL |
| `spo-api/config.yaml` | 1 | Updated network_id |
| `spo-indexer/Cargo.toml` | 3 | Updated to use workspace dependencies |
| `spo-api/Cargo.toml` | 1 | Updated to use workspace dependencies |
| `spo-api/src/application.rs` | 1 | Removed Copy trait from Config |
| `spo-api/src/infra/api/mod.rs` | 1 | Changed .copied() to .cloned() |
| `spo-indexer/src/domain/rpc.rs` | 8 | Made aura_pub_key and grandpa_pub_key optional |
| `spo-indexer/src/infra/subxt_node.rs` | 2 | Handle optional aura/grandpa keys |
| `spo-indexer/src/application.rs` | 1 | Handle optional aura_pub_key |

**Total Files Modified**: 13
**Total New Files**: 4 (migration SQL files)

## Dependency Changes

### Updated Dependencies

| Dependency | Old Version | New Version | Reason |
|------------|-------------|-------------|--------|
| midnight-ledger | alpha.2 | alpha.5 | Preview network support |
| async-nats | 0.42 | 0.45 | Compatibility with midnight-ledger |
| blake2 | - | 0.10.6 | Added to workspace |
| blockfrost | - | 1.1.0 | Added to workspace |
| hex | - | 0.4.3 | Added to workspace |
| once_cell | - | 1.19 | Added to workspace |
| paste | - | 1.0 | Added to workspace |
| regex | - | 1.11 | Added to workspace |

## Breaking Changes from alpha.2 to alpha.5

### 1. NetworkId Type System

**Before (alpha.2)**:
```rust
#[derive(Copy, Clone)]
pub enum NetworkId {
    Undeployed,
    DevNet,
    TestNet,
    MainNet,
}
```

**After (alpha.5)**:
```rust
pub struct NetworkId(pub String);
```

**Impact**:
- NetworkId no longer implements Copy trait
- Supports arbitrary network names ("preview", "qanet", etc.)
- Configuration changed from enum variant to string value

### 2. Midnight RPC API Changes

**Removed Fields in `CandidateRegistration`**:
- `auraPubKey` - No longer returned by `sidechain_getAriadneParameters`
- `grandpaPubKey` - No longer returned by `sidechain_getAriadneParameters`

**Missing RPC Methods**:
- `sidechain_getEpochCommittee` - Method not available in Preview network

**Impact**:
- Code must handle optional consensus keys
- Committee data retrieval needs alternative approach

## Recommendations for Future Work

### Immediate Priority

1. **Resolve Committee Data Issue**:
   - Contact Midnight team for Preview network RPC documentation
   - Identify correct method to fetch committee information
   - Or implement graceful handling if committee data is not critical

2. **End-to-End Testing**:
   - Test full epoch processing cycle
   - Verify data persists correctly to database
   - Test GraphQL queries through spo-api

### Medium Priority

3. **Documentation Updates**:
   - Update README with SPO services documentation
   - Document GraphQL schema
   - Add examples for common queries

4. **Monitoring**:
   - Add health metrics for SPO services
   - Monitor epoch processing performance
   - Track Blockfrost API usage

### Low Priority

5. **Optimization**:
   - Review and optimize database queries
   - Consider caching strategies for pool metadata
   - Optimize Docker build times with better layer caching

6. **Code Cleanup**:
   - Remove dead code if any
   - Consolidate duplicate logic
   - Update comments to reflect Preview network specifics

## Appendix

### A. Network Configuration Comparison

| Config Item | Old (Dev Network) | New (Preview Network) |
|-------------|-------------------|----------------------|
| network_id | "Undeployed" | "preview" |
| RPC URL | ws://node:9944 | wss://rpc.preview.midnight.network |
| Database User | postgres | indexer |
| Blockfrost Network | mainnet | preview |

### B. Database Schema

See migration files in `indexer-common/migrations/postgres/`:
- `002_spo_initial.sql` - Core SPO tables
- `003_drop_stg_committee.sql` - Cleanup
- `004_spo_stake.sql` - Stake tracking
- `005_spo_stake_history.sql` - Historical stake data

### C. References

- **Midnight Documentation**: https://docs.midnight.network/
- **midnight-indexer Repository**: https://github.com/midnightntwrk/midnight-indexer
- **midnight-ledger Repository**: https://github.com/midnightntwrk/midnight-ledger
- **Blockfrost API**: https://docs.blockfrost.io/

---

**Document Version**: 1.0
**Last Updated**: November 17, 2025
**Authors**: Migration performed with assistance from Claude (Anthropic)
