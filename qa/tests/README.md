# Midnight Indexer QA tests

## Table of Contents

- [📦 Prerequisites](#-prerequisites)
- [🧰 Install Dependencies](#-install-dependencies)
- [🔐 Environmental Setup](#-environment-setup)
- [🏢 Organization Access](#-organization-access)
- [🧪 Test Framework Organization](#-test-framework-organization)
- [🚀 Getting Started (Local Undeployed Environment)](#-getting-started-local-undeployed-environment)
- [🌐 Running Against Deployed Environments](#-running-against-deployed-environments)
- [✨ Features](#-features)
- [🛠️ Future Developments & Test Ideas](#-future-developments-improvements--test-ideas)

A test suite for validating and experimenting with the Midnight Indexer component through its GraphQL API.
This project provides a structured environment for running smoke and integration tests, covering both GraphQL queries and subscriptions, against various target environments (including local/undeployed), supporting rapid development and testing for the Midnight Indexer component.

---

## 📦 Prerequisites

- **Node.js**: v22 or higher
- **Yarn**: v3.6.x (already included in .yarn/releases)
- **Midnight Indexer**: 3.x and above
- **Docker**: latest stable (required for local/undeployed runs)

---

## 🧰 Install Dependencies

From the **QA tests folder**, install all required dependencies:

```bash
cd qa/tests
yarn install --immutable
```

---

## 🔐 Environment Setup

### Organization Access

> Your GitHub account must be a member of the midnight-ntwrk organization to read private repositories and pull images: https://github.com/midnight-ntwrk/

Before running the QA tests, make sure your local environment is configured according to the setup steps described in the main project README.

#### Step 1 — [Environment Variables](../../README.md#environment-variables)

#### Step 2 — [Required Configuration for Private Repositories](../../README.md#required-configuration-for-private-repositories)

#### Step 3 — [GitHub Personal Access Token (PAT)](../../README.md#github-personal-access-token-pat)

#### Step 4 — [~/.netrc Setup](../../README.md#netrc-setup)

#### Step 5 — [Docker Authentication](../../README.md#docker-authentication)

#### Step 6 — [GPG Setup (Signed Git Commits)](../../README.md#gpg-setup-signed-git-commits)

> This is required to push signed commits to Midnight repositories

---

## 🧪 Test Framework Organization

The test suite is organized using **Vitest projects**, which allows running different test types independently:

- **[Smoke Tests](tests/smoke/README.md)** - Quick health checks and API validation (~1 second runtime)
- **[Integration Tests](tests/integration/README.md)** - Comprehensive GraphQL API testing with pre-seeded data
- **[E2E Tests](tests/e2e/README.md)** - End-to-end validation using the Node Toolkit (includes cache warmup)

Each project can be run independently or together. E2E tests include a cache warmup phase for the Node Toolkit, while smoke and integration tests start immediately.

## 🚀 Getting Started

### 1) From **qa/tests**, ensure dependencies are installed:

```bash
cd qa/tests
yarn install --immutable
```

### 2) Move to the **repo root**:

```bash
cd ../..   # move up to the repo root
```

### 3) Load env

```bash
source .envrc
```

### 4) Set versions

By default, the node and indexer version to use will be determined based on the value in `NODE_VERSION` file and the SHA-1 of the commit where that file was updated (which indicates when a working indexer/node pair has been identified).
Alternatively, you can override versions before running tests, depending on the target environment.

### 4a) Toolkit fetch cache (Postgres)

E2E and integration tests that use the Node Toolkit fetch a Postgres-backed
cache (`MN_FETCH_CACHE`). The test harness brings the `toolkit-postgres`
container up automatically on a dynamically chosen host port — no manual
step is required. Cache data persists between runs under
`qa/tests/.tmp/toolkit-postgres-data/`. To start fresh, stop and remove the
container: `docker rm -f toolkit-postgres`.

#### Undeployed / local environment

When running against undeployed (local) environments, you may override Node, Indexer, and Toolkit versions before running the startup scripts:

```bash
# Set desired versions of Indexer + Node + Toolkit (must be done BEFORE running the startup scripts)
export NODE_TAG=0.17.0-rc.4
export INDEXER_TAG=3.0.0-alpha.5
export NODE_TOOLKIT_TAG=latest-main
```

Note: if you need to match a particular toolkit version:
```bash
export NODE_TOOLKIT_TAG=0.18.0-rc.7
```

#### Deployed environment (devnet, qanet, preview, etc)

When running against deployed environments, the Node and Indexer versions are fixed by the target environment and must not be overridden.

In this case, you may only override the Toolkit version used by the tests:

```bash
export NODE_TOOLKIT_TAG=latest-main
```

Note: if you need to match a particular toolkit version:
```bash
export NODE_TOOLKIT_TAG=0.17.0-rc.4
```

#### Indexer API Version

The GraphQL API version used by the HTTP and WebSocket clients defaults to `v4`. If the target environment uses a different API version, you can override it with the `INDEXER_API_VERSION` environment variable:

```bash
export INDEXER_API_VERSION=v3
```

This controls the version segment in the API endpoint paths (e.g. `/api/v3/graphql` and `/api/v3/graphql/ws`). If not set, the clients will use `/api/v4/graphql` and `/api/v4/graphql/ws`.

#### Vitest Worker Pool Cap

By default Vitest sizes its worker pool to all available parallelism (≈ `os.cpus().length`), so on a typical CI runner each test run drives 4–8 forked workers concurrently against the indexer. To cap that — for example when characterising load-induced flakiness against a shared environment — set the `VITEST_MAX_WORKERS` environment variable:

```bash
# Cap to a single worker (serial file execution)
VITEST_MAX_WORKERS=1 TARGET_ENV=qanet yarn test:integration

# Or a percentage of available CPUs
VITEST_MAX_WORKERS=50% TARGET_ENV=qanet yarn test:integration
```

Accepted values: a positive integer (`1`, `2`, …) or a `"<n>%"` percentage. Invalid values fail fast at config load with a clear error rather than crashing inside the worker pool. When the variable is unset, Vitest falls back to its default (all available parallelism), so local and unconstrained CI runs are unaffected.

For full instructions on updating the Node version, see the [Updating Node Version Guide](../../docs/updating-node-version.md)

## Running Test Projects on undeployed/local environment

When `TARGET_ENV=undeployed`, the test framework provisions the local Docker
stack automatically as a vitest `globalSetup` step and tears it down when the
suite finishes. **No manual script invocation is required.**

> ⚠️ **Required env vars**
>
> `NODE_TAG` and `INDEXER_TAG` must be set explicitly when
> `TARGET_ENV=undeployed`. There is no auto-derivation. `NODE_TOOLKIT_TAG`
> defaults to `latest-main` if unset.

Stack flavour by suite:

| Suite | Provisioning script invoked | Chain state |
|-------|----------------------------|-------------|
| `smoke` | `qa/scripts/startup-localenv-with-data.sh` | pre-seeded from `.node/<NODE_TAG>/` |
| `integration` | `qa/scripts/startup-localenv-with-data.sh` | pre-seeded from `.node/<NODE_TAG>/` |
| `e2e` | `qa/scripts/startup-localenv-from-genesis.sh` | fresh (toolkit generates data dynamically) |

> ℹ️ **`.node/<NODE_TAG>/` must exist** for the with-data flavour. Generate it
> via `./generate_node_data.sh <NODE_TAG>` from the repo root if it isn't there.

### Smoke and integration

```bash
cd qa/tests
NODE_TAG=1.0.0-rc.8 INDEXER_TAG=4.3.2-rc.1 TARGET_ENV=undeployed yarn test:smoke
NODE_TAG=1.0.0-rc.8 INDEXER_TAG=4.3.2-rc.1 TARGET_ENV=undeployed yarn test:integration
```

Smoke uses the same with-data stack as integration, so a smoke pass is a
meaningful precursor to integration.

### E2E

E2E still requires the Toolkit Postgres container (used for the toolkit fetch
cache). Start it once before running:

```bash
bash qa/scripts/start-toolkit-postgres.sh

cd qa/tests
NODE_TAG=1.0.0-rc.8 INDEXER_TAG=4.3.2-rc.1 TARGET_ENV=undeployed yarn test:e2e
```

### Clash safety

If the indexer is already reachable on `http://localhost:8088/ready` when the
framework starts, it treats this as a manually-managed stack: it **skips
provisioning** and **skips teardown**. You can keep a stack running between
runs by spinning it up yourself first.

### Manual stack management (optional)

The provisioning scripts remain available for direct invocation if you prefer
to manage the stack yourself (e.g. to keep it up across many `yarn test:*`
runs, or for debugging):

```bash
# pre-seeded data flavour
NODE_TAG=1.0.0-rc.8 INDEXER_TAG=4.3.2-rc.1 bash qa/scripts/startup-localenv-with-data.sh

# fresh-from-genesis flavour
NODE_TAG=1.0.0-rc.8 INDEXER_TAG=4.3.2-rc.1 bash qa/scripts/startup-localenv-from-genesis.sh

# teardown
docker compose --profile cloud down
```

See the individual project README files for detailed information about each test suite.

### Runtime Upgrade Test

Tests indexer behaviour during a node runtime upgrade (e.g. 0.21 → 0.22). Uses the same approach as the node CI hardfork test: starts the newer node binary with an older chain-spec (embedding the old runtime), then applies the new runtime via governance.

#### Prerequisites

- All standard prerequisites above
- The `FROM_NODE_TAG` node image must be available (e.g. `midnightntwrk/midnight-node:0.21.0`)
- The `TO_NODE_TAG` node image must be available (e.g. `midnightntwrk/midnight-node:0.22.2`)
- The runtime WASM must differ between the two versions (patch versions like 0.22.1 → 0.22.2 may have identical runtimes)

#### How it works

1. Generates a chain-spec from the old node — this embeds the old runtime
2. Extracts the new runtime WASM from the new node image
3. Starts the new node binary with the old chain-spec (the node executes the old runtime via WASM)
4. Starts the indexer and waits for it to be ready
5. Pauses for pre-upgrade test execution
6. Performs a runtime upgrade via federated governance using the node-toolkit
7. Verifies the `specVersion` changed
8. Pauses for post-upgrade test execution

#### Running the test

```bash
# From the repo root
source .envrc

FROM_NODE_TAG=0.22.2 \
  TO_NODE_TAG=1.0.0-rc.1 \
  INDEXER_TAG=4.1.0-ff417ad1 \
  IMAGE_REGISTRY=ghcr.io/midnight-ntwrk \
  bash qa/scripts/test-runtime-upgrade.sh
```

The script pauses after environment startup so you can run pre-upgrade tests:

```bash
cd qa/tests
TARGET_ENV=undeployed yarn test:integration
```

You can verify the node version before and after the upgrade on https://polkadot.js.org/apps/?rpc=ws%3A%2F%2F127.0.0.1%3A9944#/explorer — the runtime version should change after the upgrade.

Press Enter in the script to trigger the runtime upgrade. After the upgrade completes, run post-upgrade tests:

```bash
TARGET_ENV=undeployed yarn test:integration
```

#### Environment variables

| Variable | Required | Default | Description |
|---|---|---|---|
| `FROM_NODE_TAG` | Yes | — | Old node version (e.g. `0.21.0`) |
| `TO_NODE_TAG` | Yes | — | New node version (e.g. `0.22.2`) |
| `INDEXER_TAG` | Yes | — | Indexer image tag to test |
| `NODE_TOOLKIT_TAG` | No | `latest-main` | Node toolkit version for the governance upgrade |
| `IMAGE_REGISTRY` | No | `midnightntwrk` | Docker image registry (use `ghcr.io/midnight-ntwrk` for GHCR images) |

#### Notes

- A direct node binary swap (stopping the old node, starting the new one on existing chain data) does **not** work for hard forks — the new binary panics on state produced by the old runtime. The chain-spec approach avoids this.
- The council URIs (`//Eve`, `//Ferdie`) and technical committee URIs (`//Alice`, `//Bob`) are hardcoded for the `dev` preset. If your chain-spec uses different well-known accounts, update them in the script.
- The compose override file `docker-compose.runtime-upgrade.yaml` is used to mount the chain-spec into the node container.

---

## 🌐 Running Against Deployed Environments

There are a number of deployed environments that are used for testing components of the Midnight network. They are:

- devnet
- qanet
- preview

When running **E2E tests** against deployed environments (devnet, qanet, preview, etc.),
the test harness auto-starts the toolkit fetch cache locally (see “Toolkit
fetch cache (Postgres)” above). Just change the `TARGET_ENV` variable accordingly
(NOTE: use lower case for environment names):

```bash
TARGET_ENV=devnet yarn test       # devnet
TARGET_ENV=qanet yarn test    # qanet
```

If the target environment uses a different indexer API version than the default (`v4`), set `INDEXER_API_VERSION` accordingly:

```bash
TARGET_ENV=preprod INDEXER_API_VERSION=v3 yarn test:integration
```

## ✨ Features

- **Based on Vitest**: Uses Vitest as a modern, TypeScript-based test framework with project organization

- **Project-Based Organization**: Tests are organized into three independent projects (smoke, integration, e2e) that can run separately or together

- **[Smoke Tests](tests/smoke/README.md)**: Health checks and schema validation for GraphQL endpoints

- **[Integration Tests](tests/integration/README.md)**: Fine-grained GraphQL query and subscription tests for blocks, transactions, and contract actions

- **[E2E Tests](tests/e2e/README.md)**: Tests that use the Node Toolkit to perform actions on the blockchain and validate indexer results

- **Smart Cache Management**: E2E tests include toolkit cache warmup; integration and smoke tests start immediately

- **Custom Reporters**: JUnit-compatible output for CI integration and XRay custom test reporting

- **Improved Logging**: Configurable logging for debugging and test traceability

---

## 🛠️ Future Developments, Improvements & Test Ideas

- **Contract actions**: Expand test coverage to include missing contract actions.

- **Advanced Integration tests**: Expand test coverage with the usage of Node Toolkit.

- **Test containers support**: Add support for Test Container to add better fine-grained control over the indexer sub-components

- **Add Tooling for Test Data Scraping**: Tools for generating synthetic blocks, transactions, and keys.

- **GraphQL Schema Fuzzing**: Randomized query/subscription request schema with corresponding validation

- **Dynamic Data Fetching**: Use the block scraper to fetch recent block data to execute the test against (potentially) different test data every run

- **Log file per test**: Right now the test execution is per test file, having log files per test will allow concurrent test execution.
