# SPO API

GraphQL API exposing SPO identity, pool metadata, and per-epoch performance.

- HTTP (GraphQL + GraphiQL UI): /api/v1/graphql
- WebSocket (GraphQL WS): /api/v1/graphql/ws
- Readiness: /ready

Open GraphiQL at <http://localhost:8090/api/v1/graphql>

## Quick start

Option A — cargo (local)

```bash
# Build
cargo build -p spo-api --features cloud

# Run (requires Postgres and password)
export APP__INFRA__STORAGE__PASSWORD=indexer
CONFIG_FILE=spo-api/config.yaml cargo run -p spo-api --features cloud
```

Option B — Docker Compose

```bash
# Ensure .env contains APP__INFRA__STORAGE__PASSWORD
echo APP__INFRA__STORAGE__PASSWORD=indexer >> .env

# Bring up DB and API
docker compose up -d postgres spo-api

# Health
curl -f http://localhost:8090/ready
```

## Handy queries

```graphql
query ServiceInfo {
  serviceInfo { name version network }
}

query LatestPerformance {
  spoPerformanceLatest(limit: 10, offset: 0) {
    epochNo
    spoSkHex
    produced
    expected
    poolIdHex
  }
}

query PerformanceBySPO($spoSk: String!) {
  spoPerformanceBySpoSk(spoSkHex: $spoSk, limit: 5, offset: 0) {
    epochNo
    produced
    expected
    identityLabel
  }
}

query EpochPerformance($epoch: Int!) {
  epochPerformance(epoch: $epoch, limit: 20, offset: 0) {
    spoSkHex
    produced
    expected
    poolIdHex
  }
}

query SpoByPoolId($poolId: String!) {
  spoByPoolId(poolIdHex: $poolId) {
    poolIdHex
    sidechainPubkeyHex
    name
    ticker
  }
}

query SpoList {
  spoList(limit: 10, offset: 0) {
    poolIdHex
    sidechainPubkeyHex
    name
    ticker
    homepageUrl
    logoUrl
  }
}

query CurrentEpochInfo {
  currentEpochInfo {
    epochNo
    durationSeconds
    elapsedSeconds
  }
}

query EpochUtilization($epoch: Int!) {
  epochUtilization(epoch: $epoch)
}

query SpoCount {
  spoCount
}
```

## Operation reference (v1)

- serviceInfo: ServiceInfo!
- spoIdentities(limit: Int = 50, offset: Int = 0): [SpoIdentity!]!
- spoIdentityByPoolId(poolIdHex: String!): SpoIdentity
- poolMetadata(poolIdHex: String!): PoolMetadata
- poolMetadataList(limit: Int = 50, offset: Int = 0, withNameOnly: Boolean = false): [PoolMetadata!]!
- spoList(limit: Int = 20, offset: Int = 0): [Spo!]!
- spoByPoolId(poolIdHex: String!): Spo
- spoCompositeByPoolId(poolIdHex: String!): SpoComposite
- stakePoolOperators(limit: Int = 20): [String!]!
- spoPerformanceLatest(limit: Int = 20, offset: Int = 0): [EpochPerf!]!
- spoPerformanceBySpoSk(spoSkHex: String!, limit: Int = 100, offset: Int = 0): [EpochPerf!]!
- epochPerformance(epoch: Int!, limit: Int = 100, offset: Int = 0): [EpochPerf!]!
- currentEpochInfo: EpochInfo
- epochUtilization(epoch: Int!): Float
- spoCount: BigInt

Key return types (selected fields):

- SpoIdentity: poolIdHex, mainchainPubkeyHex, sidechainPubkeyHex, auraPubkeyHex
- PoolMetadata: poolIdHex, hexId, name, ticker, homepageUrl, logoUrl
- Spo: poolIdHex, sidechainPubkeyHex, auraPubkeyHex, name, ticker, homepageUrl, logoUrl
- EpochPerf: epochNo, spoSkHex, produced, expected, identityLabel, poolIdHex
- EpochInfo: epochNo, durationSeconds, elapsedSeconds

Notes

- Identifiers are stored as plain strings (hex text), not BYTEA. Supply lowercase hex without 0x where possible.
- Performance joins use spo_sk (sidechain key) as the canonical identity.
- Subscriptions will be added later (NATS integration).

## Configuration

Excerpt (see spo-api/config.yaml):

```yaml
infra:
  storage:
    host: localhost
    port: 5432
    dbname: indexer
    user: indexer
    sslmode: prefer
    max_connections: 10
    idle_timeout: 1m
    max_lifetime: 5m
  api:
    address: 0.0.0.0
    port: 8090
    max_complexity: 2000
    max_depth: 50
```

Provide the password via env: APP__INFRA__STORAGE__PASSWORD.
