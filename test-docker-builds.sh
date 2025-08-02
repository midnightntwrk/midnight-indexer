#!/bin/bash
set -e

echo "=== Testing Docker builds with mock data ==="

# Get Rust version from rust-toolchain.toml
RUST_VERSION=$(grep channel rust-toolchain.toml | sed -r 's/channel = "(.*)"/\1/')
echo "Using Rust version: $RUST_VERSION"

# Ensure the Rust version is correctly extracted
if [ -z "$RUST_VERSION" ]; then
    echo "Failed to extract Rust version, using default 1.88.0"
    RUST_VERSION="1.88.0"
fi

# Build indexer-api
echo ""
echo "=== Building indexer-api Docker image ==="
docker build \
  --build-arg RUST_VERSION=$RUST_VERSION \
  --build-arg PROFILE=dev \
  --secret id=netrc,src=$HOME/.netrc \
  -f indexer-api/Dockerfile \
  -t midnight-indexer-api-test:local \
  .

# Build indexer-standalone  
echo ""
echo "=== Building indexer-standalone Docker image ==="
docker build \
  --build-arg RUST_VERSION=$RUST_VERSION \
  --build-arg PROFILE=dev \
  --secret id=netrc,src=$HOME/.netrc \
  -f indexer-standalone/Dockerfile \
  -t midnight-indexer-standalone-test:local \
  .

echo ""
echo "=== Build completed successfully ==="
echo "Images created:"
echo "  - midnight-indexer-api-test:local"
echo "  - midnight-indexer-standalone-test:local"