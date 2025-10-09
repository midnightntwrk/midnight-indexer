# Toolkit TypeScript Wrapper (Prototype)

**Purpose**
- Provide a clean TypeScript API to call Midnight Toolkit from tests/services.
- Be environment-aware so calls use the correct RPC URLs and network IDs.


**Show-address**
- Wraps: `midnight-node-toolkit show-address`
- Params: chain: `undeployed` | `nodedev01` | `devnet` | `qanet` | `testnet02`, addressType: `shielded` | `unshielded`, seed: 64-hex string
- Does not require a running node; uses network ID from `env-registry.ts`.
- Runs via Testcontainers (ephemeral).

