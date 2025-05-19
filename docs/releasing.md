# Releasing a New Version

We use [cargo-release](https://github.com/crate-ci/cargo-release) and Git tags to manage releases.

## Prerequisites

- A valid GPG key for signed commits and tags.
- Classic GitHub PAT (as described in [development-setup.md](./development-setup.md)) for pushing tags if needed.
- `cargo release` plugin installed:
  ```bash
  cargo install cargo-release
  ```

## Steps

1. Update `CHANGELOG.md` with new features and fixes.
2. Run tests:
   ```bash
   just all
   ```
3. Create a release (e.g., `minor`):
   ```bash
   just release minor
   ```
   This runs `cargo release` under the hood, signing commits and tags, and updating `Cargo.toml`.

4. Push the tag:
   ```bash
   git push --tags
   ```

CI will build and publish Docker images for the new tag. Check [GitHub Releases](https://github.com/input-output-hk/midnight-indexer/releases) for details.
