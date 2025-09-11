# Toolkit TypeScript Wrapper (Prototype)

**Purpose**
- Provide a clean TypeScript API to call Midnight Toolkit from tests/services.
- Be environment-aware so calls use the correct RPC URLs and network IDs.

**Terminology & Environments**
- We standardize “chain / env / network” via a single registry (`src/env-registry.ts`).
- Supported environments (initial): `undeployed`, `nodedev01`, `devnet`, `qanet`, `testnet02`.
- Each env defines:
  - Default RPC URLs (src/dest)
  - Network ID used for wallet/address ops
  - A per-env toolkit container name (if reusing a long-lived container)

**Design Goals**
- Start simple: expose `generateBatches`, `generateSingleTransaction`, `sendFromFile` later.
- Keep the adapter implementation swappable (initially Testcontainers) behind a stable TS interface.
- Use correct URLs per env (note: for `undeployed`, prefer container hostname, not localhost).



