# Development Setup

## Requirements

- **Rust**: The latest stable version (see `rust-toolchain.toml`).
- **Cargo**: Comes with Rust.
- **Just**: A command-runner used for tasks. Install from [GitHub](https://github.com/casey/just).
- **Docker**: For integration tests and running services locally.
- **direnv**: For a reproducible development environment.
- **earthly**: [Earthly](https://earthly.dev/) for reproducible builds (if desired).
- **cargo-release (optional)**: For automated versioning and releases.

### Required Configuration for Private Repositories

You may need access to private Midnight repositories and containers. To achieve this:

1. **GitHub Personal Access Token (PAT)**:  
   Create a classic PAT with:
   - `repo` (all)
   - `write:packages`
   - `org:read`

2. **~/.netrc Setup**:
   ```bash
   machine github.com
   login <YOUR_GITHUB_ID>
   password <YOUR_GITHUB_PAT>
   ```

3. **Docker Authentication**:
   ```bash
   echo $GITHUB_TOKEN | docker login ghcr.io -u <YOUR_GITHUB_ID> --password-stdin
   ```

4. **Signed Commits**:
   Configure GPG signing for commits if required by the repository.

## Steps

1. Clone the repository:
   ```bash
   git clone https://github.com/input-output-hk/midnight-indexer.git
   cd midnight-indexer
   ```

2. direnv:
   ```bash
   # Before running `direnv allow`, you may need to run `just init`.
   # See the 'Initialisation' section below.
   direnv allow
   ```

3. Install Just:
   ```bash
   cargo install just
   ```

4. Initialisation:
   The first time you build the project, you might need to run:
   ```bash
   just init
   ```
   This command downloads necessary ledger binaries and sets up the proof server locally.
   Without this step, building may fail because required artifacts are missing.

5. Build:
   ```bash
   just all-features
   ```
   This runs checks, lints, tests, and doc generation.

## Working with Features

By default, `just` commands run with the `cloud` feature set. To run a `just` command with standalone features, specify it as:
```bash
just feature=standalone test
```
This applies to other commands like `just feature=standalone run-indexer`.

## Generating the GraphQL Schema

To regenerate the GraphQL schema from the code, run:
```bash
just generate-indexer-api-schema
```
This outputs the schema to `indexer-api/graphql/schema-v1.graphql` so it stays in sync with code changes.

## Building Docker Images Locally

For standalone development, you can build Docker images for each component:

- Chain Indexer:
  ```bash
  just docker-chain-indexer
  ```
- Wallet Indexer:
  ```bash
  just docker-wallet-indexer
  ```
- Indexer API:
  ```bash
  just docker-indexer-api
  ```
- Unified Indexer:
  ```bash
  just docker-indexer
  ```

These commands produce Docker images tagged as `ghcr.io/midnight-ntwrk/<component>:latest`.  
You can then run the images locally or as part of a Compose setup.

## Editor and Tooling

- Use `rust-analyzer` for IDE features.
- Run `just +nightly fmt` before pushing code.
- `just lint` for Clippy lint checks.

## Additional Tools

- **cargo release**: For managing versions, tags, and changelogs, see [Releasing](./releasing.md).

## Further Reading

- [Integration Tests](./integration-tests.md)
- [API Documentation](./api/v1/api-documentation.md)
- [Releasing](./releasing.md)
- [Contributing](./contributing.md)
