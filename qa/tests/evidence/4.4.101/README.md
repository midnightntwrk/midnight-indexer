---
title: Midnight Indexer 4.4.101 — QA test evidence
type: qa-test-report
status: published
summary: >-
  Smoke, integration and end-to-end results for indexer 4.4.101 on a local undeployed stack (toolkit and moth backends) and on qanet (moth backend).
---

## Summary

Result: **PASS.**

> **Image equivalence.** The undeployed stack ran the image tag `4.4.101-5b60b43a`. That tag is equivalent to `4.4.101`: both are built from commit `5b60b43a65a898933f25b9bbd60bcd5d855c8560`, which carries the tag `v4.4.101` and workspace version 4.4.101.

The two environments cover different things. qanet supplies chain data that a fresh local chain does not have: DUST generations, bridge data, contract fixtures and a custom unshielded token. The undeployed stack runs the tests that need a local chain: crafted failing segments, subscription polling, and toolkit key material.

| Field | Value |
| --- | --- |
| Item | Midnight Indexer 4.4.101 (undeployed image tag `4.4.101-5b60b43a`, equivalent to `4.4.101`) |
| Date | 5 November 2026 |
| Targets | qanet (deployed, ledger 9, after the hard fork); undeployed (local Docker stack, node 2.1.101) |
| Kinds of test | smoke, integration, end-to-end |
| Transaction backends | qanet: moth; undeployed: node toolkit (smoke, integration, end-to-end) and moth (end-to-end) |
| Tested by | Oleksandr Romanov (oleksandr.romanov@shielded.io) |

### Qanet

| Test kind | Cases (test files) | Tests | Passed | Failed | Skipped | Todo | Status |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| Smoke | 6 | 30 | 29 | 0 | 1 | 0 | PASS |
| Integration | 30 | 315 | 275 | 0 | 20 | 20 | PASS |
| End-to-end (moth) | 13 | 78 | 53 | 0 | 23 | 2 | PASS (1 test passed on retry) |
| **Total** | **49** | **423** | **357** | **0** | **44** | **22** | **PASS** |

### Undeployed

| Test kind | Backend | Cases (test files) | Tests | Passed | Failed | Skipped | Todo | Status |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| Smoke | toolkit | 6 | 30 | 29 | 0 | 1 | 0 | PASS |
| Integration | toolkit | 30 | 315 | 175 | 0 | 120 | 20 | PASS |
| End-to-end | toolkit | 13 | 78 | 61 | 0 | 15 | 2 | PASS |
| End-to-end | moth | 13 | 78 | 49 | 0 | 27 | 2 | PASS (1 test passed on retry) |
| **Total** | | **62** | **501** | **314** | **0** | **163** | **24** | **PASS** |

### Combined

| Environment | Tests | Passed | Failed | Skipped | Todo |
| --- | ---: | ---: | ---: | ---: | ---: |
| qanet | 423 | 357 | 0 | 44 | 22 |
| undeployed | 501 | 314 | 0 | 163 | 24 |
| **Total** | **924** | **671** | **0** | **207** | **46** |

Note: Unit tests are not part of this report. They run in the indexer's own CI (`just test`), not against a deployed environment.

## Environment

| Field | Value |
| --- | --- |
| Test repository | `https://github.com/midnightntwrk/midnight-indexer`, suite in `qa/tests` |
| Commit SHA | `24b350f06776261a3dd54b7188a09891de5b2029` |
| Branch | `release/4.4.101` |
| qanet target | `TARGET_ENV=qanet`. Indexer `https://indexer.qanet.shielded.tools`, node RPC `rpc.qanet.shielded.tools` |
| qanet chain at run time | tip about block 3,401,600; protocol version 2001100; first ledger-9 block 3,370,894 |
| Undeployed target | `TARGET_ENV=undeployed`. Local Docker stack from the repository's `docker-compose.yaml`, provisioned by the suite, indexer API on port 8088 |

### Component versions

| Component | qanet | Undeployed | Source |
| --- | --- | --- | --- |
| Indexer (system under test) | 4.4.101 | `ghcr.io/midnight-ntwrk/{chain-indexer,wallet-indexer,spo-indexer,indexer-api}:4.4.101` (arm64), equivalent to `:4.4.101` | qanet: the deployment. |
| Node | 2.1.101-913ec011 | `ghcr.io/midnight-ntwrk/midnight-node:2.1.101` | qanet: `system_version`. Undeployed: `NODE_TAG` |
| Protocol version / ledger | 2001100, ledger 9 | node 2.1.101 runtime | qanet: indexer `block.protocolVersion` |
| Node toolkit | not used (`TX_BACKEND=moth`) | `ghcr.io/midnight-ntwrk/midnight-node-toolkit:2.1.101` | `NODE_TOOLKIT_TAG`, the same tag as the node |
| Proof server | `midnightntwrk/proof-server:9.0.0-rc.10` | `midnightntwrk/proof-server:9.0.0-rc.10` (moth run) | local container started by the suite (`utils/moth/proof-server.ts`) |
| Transaction backend | moth-wallet `0.15.0-next.20261104215904-5ce169b` | the same, for the moth run | `qa/tests/package.json` |
| Wallet SDK | `@midnightntwrk/wallet-sdk` 2.0.0 | same | `qa/tests/package.json` |
| Midnight.js (contract path) | 5.0.0 | same | `qa/tests/package.json` |
| Ledger client | `@midnightntwrk/ledger-v9` 1.0.0-rc.7 | same | `qa/tests/package.json` |
| Compact runtime | 0.20.0 | same | `qa/tests/package.json` |
| Test runner | vitest 4.1.10 on Bun 1.3.5, Node.js 24.0.0 | same | local |

**Governance action.** qanet's hard fork from ledger 8 to ledger 9 was enacted before this testing, at block 3,370,894 on 3 November 2026. These runs did not trigger or simulate the fork. They tested the indexer on the post-fork chain, with the pre-fork history still in place.

## Detailed reports

Each environment has its own report with run details, run times, the per-case results and every individual test result, generated from the evidence files in that folder.

| Report | Runs | Evidence files |
| --- | --- | --- |
| [qanet](qanet/indexer-test-report.md) | smoke, integration, end-to-end (moth) | `qanet/{smoke,integration,e2e}/junit-test-results.xml` (JUnit) |
| [undeployed](undeployed/indexer-test-report.md) | smoke, integration and end-to-end (toolkit); end-to-end (moth) | `undeployed/smoke/smoke.json`, `undeployed/integration/integration.json`, `undeployed/e2e/e2e-toolkit.json`, `undeployed/moth/e2e.json` (vitest JSON) |

## How to rerun it

These steps reproduce the seven runs above. Run all commands from the repository root unless a step says otherwise.

### 1. Prerequisites

- Docker, Bun 1.3.x and Node.js 22 or later (the runs used Bun 1.3.5 and Node.js 24.0.0).
- Logged in to GHCR (`docker login ghcr.io`) with read access to `ghcr.io/midnight-ntwrk`. The indexer, node and toolkit images are pulled from there.
- Port 8088 free for the undeployed indexer API.
- For qanet only: a funding wallet seed that has made both shielded and unshielded transactions, with DUST registered after the ledger-9 fork.

### 2. Check out the tested code

```
cd qa/tests
bun install --frozen-lockfile
cd ../..
source .envrc
```

### 3. Undeployed runs

```bash
cd qa/tests
export TARGET_ENV=undeployed
export INDEXER_TAG=4.4.101
export NODE_TAG=2.1.101
export NODE_TOOLKIT_TAG=2.1.101
export IMAGE_REGISTRY=ghcr.io/midnight-ntwrk
EV=evidence/4.4.101/undeployed

# toolkit backend (default; TX_BACKEND unset)
bun run test:smoke       && cp reports/json/test-results.json $EV/smoke/smoke.json
bun run test:integration && cp reports/json/test-results.json $EV/integration/integration.json
bun run test:e2e         && cp reports/json/test-results.json $EV/e2e/e2e-toolkit.json

# moth backend
TX_BACKEND=moth PROOF_SERVER_TAG=9.0.0-rc.10 bun run test:e2e \
  && cp reports/json/test-results.json $EV/moth/e2e.json
```

### 4. qanet runs

Do not set `INDEXER_TAG`, `NODE_TAG` or `NODE_TOOLKIT_TAG`: qanet's deployment sets the versions. First refresh the qanet fixtures. The scanner rewrites `qa/tests/data/static/qanet/token-holding-contracts.jsonc` and `unshielded-token-types.jsonc`. In the recorded run it scanned to block 3,401,589.

```bash
cd qa/tools/block-scanner
bun install --frozen-lockfile
TARGET_ENV=qanet bun src/scanner.ts
TARGET_ENV=qanet bun run generate:data
cd ../../tests
```

Then run the three suites with the moth backend:

```bash
export TARGET_ENV=qanet
export FUNDING_SEED_QANET=<funding wallet seed>
export TX_BACKEND=moth
export PROOF_SERVER_TAG=9.0.0-rc.10
EV=evidence/4.4.101/qanet

bun run test:smoke       && cp reports/junit/test-results.xml $EV/smoke/junit-test-results.xml
bun run test:integration && cp reports/junit/test-results.xml $EV/integration/junit-test-results.xml
bun run test:e2e         && cp reports/junit/test-results.xml $EV/e2e/junit-test-results.xml
```

### Notes

- **Proof server version.** `PROOF_SERVER_TAG` is required for moth runs. Without it the suite starts `midnightntwrk/proof-server:8.1.0`, the code default, which is not the version these runs used. The first start downloads the ZK parameters into `qa/tests/.tmp/proof-server-zk-params`, which can take several minutes.
- **Moth wallet state.** Moth keeps one wallet state per seed (override the name with `TX_BACKEND_MOTH_WALLET`). If a qanet moth run reports a DUST balance of 0 for a funded wallet, the cached wallet state is stale. Move it aside so moth rebuilds it.
- **Optional.** `VITEST_MAX_WORKERS` caps vitest's worker pool.

## Failures and known issues

No test failed in any run.

| # | Test | Result | What happens | Ticket |
| --- | --- | --- | --- | --- |
| 1 | unshielded NIGHT transactions › a confirmed unshielded transfer streamed to address subscriptions › should emit UnshieldedTransaction only for the target wallet (A > B1) (`e2e/night-transactions.test.ts`), qanet and undeployed moth | PASS on retry | In both moth runs, the first attempt failed and the automatic retry (`retry: 1`) passed. That pattern points at the test or the moth backend's timing, not at the indexer. | No ticket yet. |
