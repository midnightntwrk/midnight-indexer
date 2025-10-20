# Midnight Indexer QA tests

## Table of Contents

- [📦 Prerequisites](#-prerequisites)
- [🧰 Install Dependencies](#-install-dependencies)
- [🔐 Environment Setup](#-environment-setup)
- [🔑 Required Access to Private Midnight Repositories](#-required-access-to-private-midnight-repositories)
  - [🏢 Organization Access](#-organization-access)
  - [🪪 GitHub Personal Access Token (Classic)](#-github-personal-access-token-classic)
  - [🐳 Docker Authentication to GitHub Container Registry](#docker-authentication-to-github-container-registry)
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

As we allow zero secrets in the git repository, you need to define a couple of environment variables for build (tests) and runtime (tests). Notice that the values are just used locally for testing and can be chosen arbitrarily; `APP__INFRA__SECRET` must be a hex-encoded 32-byte value.

It is recommended to provide these environment variables via a `~/.midnight-indexer.envrc` or `./.envrc.local` file which is sourced by the `.envrc` file:

```bash
export APP__INFRA__STORAGE__PASSWORD=indexer
export APP__INFRA__PUB_SUB__PASSWORD=indexer
export APP__INFRA__LEDGER_STATE_STORAGE__PASSWORD=indexer
export APP__INFRA__SECRET=303132333435363738393031323334353637383930313233343536373839303132
```
Then in your shell, enable them (with [direnv](https://direnv.net/) or manual sourcing):

```bash
source .envrc
```

---

## 🔑 Required Access to Private Midnight Repositories
### Organization Access 
Your GitHub account must be a member of the midnight-ntwrk organization to read private repositories and pull images:

Org: https://github.com/midnight-ntwrk/

### GitHub Personal Access Token (Classic)
Create a **classic** PAT:

**1. Go to**  https://github.com/settings/tokens  
Then click: **“Generate new token” → “Generate new token (classic)”**  

**2.Name your token**  

**3. Set expiration**  
Pick a sufficiently long period (e.g., 90 days) or No expiration

**4. Select scopes**  
Check the following boxes:
- `repo` (all)
- `read:packages`
- `read:org`

**5. Generate & copy**  
Click **Generate token**, then **copy** the token value. You won’t see it again.

**6. Save it securely**  
You can store it in your keychain/1Password and also reference it via env var `GITHUB_TOKEN` when needed:
```bash
export GITHUB_TOKEN=<YOUR_GITHUB_PAT>
```

**7. `~/.netrc` setup**  

**Create or update the file:**
```bash
nano ~/.netrc
```

**Add the following lines:**
```netrc
machine github.com
  login <YOUR_GITHUB_ID>
  password <YOUR_GITHUB_PAT>
```

**Save & secure the file:**
```bash
chmod 600 ~/.netrc
```

### Docker authentication to GitHub Container Registry
You must authenticate Docker with GitHub’s Container Registry (**ghcr.io**) before pulling private Midnight images (e.g., Indexer, Node, NATS, Postgres).

```bash
echo $GITHUB_TOKEN | docker login ghcr.io -u <YOUR_GITHUB_ID> --password-stdin
```
Expected output: `Login Succeeded`

> Replace `<YOUR_GITHUB_ID>` and tokens appropriately. Keep tokens out of version control.

---
## 🚀 Getting Started (Local Undeployed Environment)

Indexer can be executed locally (this is known as `undeployed` environment). The easiest way is through the compose file at the root of the repo. Note that the indexer can be executed as a single `standalone` docker container or using the `cloud` configuration, which is made up by a number of containers (including nats and postgres). Both docker compose profiles also spin up a Midnight node container, used as a main component dependency to feed data required by the indexer.

1) From **qa/tests**, ensure dependencies are installed:
```bash
cd qa/tests
yarn install --immutable
```

2) Move to the **repo root**:
```bash
cd ../..   # move up to the repo root
```

3) Load env 

```bash
source .envrc
```

4) Set versions
```bash
export NODE_TAG=${NODE_TAG:-latest}
export INDEXER_TAG=${INDEXER_TAG:-latest}
```
If you want to pin specific versions, you can override them:

```bash
export NODE_TAG=0.17.0-rc.4
export INDEXER_TAG=3.0.0-alpha.5
```

5) Start the local environment with seeded data using the helper script:
```bash
bash qa/scripts/startup-localenv-with-data.sh
```
That script will:
- run `docker compose --profile cloud up -d` 
- wait for all containers to become healthy
- seed sample data for GraphQL testing

6) Run the tests from the QA folder:
```bash
cd qa/tests
TARGET_ENV=undeployed yarn test 
```
> To run only the end-to-end test suite, use: `TARGET_ENV=undeployed yarn test e2e`

---

## 🌐 Running Against Deployed Environments

There are a number of deployed environments that are used for testing components of the Midnight network. The are:
  - Devnet
  - QANet
  - Testnet02

To execute the tests against these environments just change the TARGET_ENV variable accordingly (NOTE: use lower case for environment names)
```bash
TARGET_ENV=devnet yarn test       # devnet
TARGET_ENV=testnet02 yarn test    # testnet02
```
NOTE: Although all the known environments are supported, right now, it only makes sense to target `undeployed` or `devnet` environments. 
This is because we are using the latest Indexer 3.x API which has incompatible changes with respect to Indexer 2.x deployed.


## ✨ Features

- **Based on Vitest**: Uses Vitest as a modern, Typescript based, test framework core
- **Smoke Tests**: Health checks and schema validation for GraphQL endpoints.
- **Basic Integration Tests**: Fine-grained GraphQL query and subscription tests for blocks, transactions, and contract actions.
- **Custom Reporters**: JUnit-compatible output for CI integration.

- **Improved Logging**: Configurable logging for debugging and test traceability.

---

## 🛠️ Future Developments, Improvements & Test Ideas

- **Contract actions**: Expand test coverage to include missing contract actions.
- **Advanced Integration tests**: Expand test coverage with the usage of Node Toolkit.
- **Test containers support**: Add support for Test Container to add better fine-grained control over the indexer sub-components
- **Add Tooling for Test Data Scraping**: Tools for generating synthetic blocks, transactions, and keys.
- **GraphQL Schema Fuzzing**: Randomized query/subscription request schema with corresponding validation 
- **Dynamic Data Fetching**: Use the block scraper to fetch recent block data to execute the test against (potentially) different test data every run 
- **Log file per test**: Right now the test execution is per test file, having log files per test will allow concurrent test execution.
