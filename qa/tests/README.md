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
This project provides a structured environment for running  smoke and integration tests, covering both GraphQL queries and subscriptions, against various target environments (including local/undeployed), supporting rapid development and testing for the Midnight Indexer component.

---

## 📦 Prerequisites

- **Node.js**: v22 or higher
- **Yarn**: v3.6.x (already included in .yarn/releases)
- **Midnight Indexer**: 3.x and above
- **Docker**: latest stable (required for local/udeployed runs)

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

### Running Test Projects

```bash
# Run all test projects together
TARGET_ENV=undeployed yarn test

# Run individual projects
TARGET_ENV=undeployed yarn test:smoke        # Smoke tests only
TARGET_ENV=undeployed yarn test:integration  # Integration tests only
TARGET_ENV=undeployed yarn test:e2e          # E2E tests only (with cache warmup)
```

See the individual project README files for detailed information about each test suite.

---
## 🚀 Getting Started (Local Undeployed Environment)

Indexer can be executed locally (this is known as `undeployed` environment). You can start it in two ways, depending on whether you want a clean or pre-seeded environment:

#### **Option 1 — Using the compose file directly**

Brings up all core services (Node, Indexer, NATS, Postgres) but starts the blockchain from genesis, meaning there will be no pre-existing blocks or transactions until you create them. 

See **Step 5** below for how to start it.

#### **Option 2 — Using the helper startup script (recommended for testing)**

This method wraps the compose command and additionally seeds the environment with sample data (blocks with transactions).

See **Step 5** below for how to use the script.

Once you’ve chosen your preferred setup, follow the steps below to install dependencies and run the tests.

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

By default, the node and indexer version to use will be determined based on the value in `NODE_VERSION` file and the SHA-1 of the commit where that file was updated (which indicates when a working indexer/node pair has been identified)
Alternavely, you can override indexer and node image, and pin them to specific tags:

```bash
export NODE_TAG=0.17.0-rc.4
export INDEXER_TAG=3.0.0-alpha.5
```

For full instructions on updating the Node version, see the [Updating Node Version Guide](../../docs/updating-node-version.md)

### 5) Start the local environment 

Choose one of the following options:

#### **Option 1 — Compose directly (clean chain):**

```bash
docker compose --profile cloud up -d
```
> Starts all containers, but the chain begins from genesis (no existing blocks or transactions).

#### **Option 2 — Helper startup script (pre-seeded data):**

```bash
bash qa/scripts/startup-localenv-with-data.sh
```
That script will:
- run `docker compose --profile cloud up -d` 
- wait for all containers to become healthy
- seed sample data for GraphQL testing

### 6) Run the tests from the QA folder:
```bash
cd qa/tests
TARGET_ENV=undeployed yarn test 
```

Run individual test projects:
```bash
TARGET_ENV=undeployed yarn test:smoke        # Quick health checks
TARGET_ENV=undeployed yarn test:integration  # GraphQL API tests
TARGET_ENV=undeployed yarn test:e2e          # End-to-end with toolkit
```

---

## 🌐 Running Against Deployed Environments

There are a number of deployed environments that are used for testing components of the Midnight network. They are:
  - devnet
  - qanet
  - preview
  - testnet02

To execute the tests against these environments just change the TARGET_ENV variable accordingly (NOTE: use lower case for environment names)
```bash
TARGET_ENV=devnet yarn test       # devnet
TARGET_ENV=testnet02 yarn test    # testnet02
```
NOTE: Although all the known environments are supported, right now, it only makes sense to target `undeployed` or `devnet` environments. 
This is because we are using the latest Indexer 3.x API which has incompatible changes with respect to Indexer 2.x deployed.


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
