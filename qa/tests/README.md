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

### 4a) Start Toolkit Postgres (required for E2E tests)

E2E tests use the Node Toolkit fetch cache backed by Postgres.  
Before running **any E2E tests** (local or deployed), start the Toolkit Postgres service in the root of the project:

```bash
bash qa/scripts/start-toolkit-postgres.sh
```

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

For full instructions on updating the Node version, see the [Updating Node Version Guide](../../docs/updating-node-version.md)

## Running Test Projects on undeployed/local environment 

### Integration tests on undeployed/local environment (with pre-existing data)

Running the tests on your local/undeployed environment has some prerequisites, depending on the type of tests you want to run. The integration tests require test data to be available for the tests to run, to do so you can use one of the scripts available in the QA folder that will help spin up a local environment with a Midnight chain with some pre-existing data:

> ⚠️ **Important**
>  
> Make sure to set the correct versions of **Node / Indexer / Toolkit** **before running the startup script**.  
> See **“Getting Started – Set versions”**.

```bash
# Startup a local environment with test data (transactions + contract actions)

# NOTE: Set Node / Indexer / Toolkit versions first (see “Getting Started – Set versions”
bash qa/scripts/startup-localenv-with-data.sh
cd qa/tests
TARGET_ENV=undeployed yarn test:integration
```


### E2E tests on undeployed/local environment (from genesis without pre-existing data)

The e2e tests don't require any pre-existing data to be executed, in fact they perform some
actions themselves so that they can assert on the outcome of those actions.

> ⚠️ **Important**
>  
> Make sure to set the correct versions of **Node / Indexer / Toolkit** **before running the startup script**.  
> See **“Getting Started – Set versions”**.


```bash
# Startup a local environment from genesis block, without test data
bash qa/scripts/startup-localenv-from-genesis.sh

# Start Toolkit Postgres before running E2E tests
bash qa/scripts/start-toolkit-postgres.sh
cd qa/tests
TARGET_ENV=undeployed yarn test:e2e
```

### Smoke tests on undeployed/local environment

Smoke tests don't require any pre-existing data so just use the following

```bash
bash qa/scripts/startup-localenv-from-genesis.sh
TARGET_ENV=undeployed yarn test:smoke
```

See the individual project README files for detailed information about each test suite.

---

Indexer can be executed locally (this is known as `undeployed` environment). You can start it in two ways, depending on whether you want a clean or pre-seeded environment:


## 🌐 Running Against Deployed Environments

There are a number of deployed environments that are used for testing components of the Midnight network. They are:

- devnet
- qanet
- preview
- testnet02

When running **E2E tests** against deployed environments (devnet, qanet, preview, etc.),
Toolkit Postgres must still be running locally:

```bash
bash qa/scripts/start-toolkit-postgres.sh
```

To execute the tests against these environments just change the TARGET_ENV variable accordingly (NOTE: use lower case for environment names)

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
