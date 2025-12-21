# Version Compatibility Guide

## Overview

This document explains the version compatibility requirements for the Midnight Indexer components to help avoid configuration issues like those described in [Issue #597](https://github.com/midnightntwrk/midnight-indexer/issues/597).

## Critical Rule: Never Use `latest` Tag

**⚠️ IMPORTANT:** Never use the `latest` tag for any component in production or development environments. Always specify exact version numbers.

Using `latest` can lead to:
- Incompatible component versions
- API endpoint mismatches (404 errors on `/api/v3/graphql`)
- Unpredictable behavior
- Difficult-to-debug issues

## Version Requirements

### All Indexer Components Must Match

All three Indexer components **MUST** use the **exact same version**:

- `chain-indexer`
- `wallet-indexer`
- `indexer-api`

### Midnight Node Compatibility

Each Indexer version requires a specific Midnight Node version. Check the [`NODE_VERSION`](./NODE_VERSION) file in the repository root for the compatible Node version.

## Current Version Compatibility

| Indexer Version | Compatible Node Version |
|----------------|------------------------|
| 3.0.0 | 0.18.0 |
| 3.0.0-alpha.19 | 0.18.0-rc.7 |

## Example: Correct Configuration

### Docker Compose

```yaml
version: '3.8'

services:
  midnight-node:
    image: midnightntwrk/midnight-node:0.18.0  # ✅ Exact version from NODE_VERSION
    # ... other configuration

  chain-indexer:
    image: midnightntwrk/chain-indexer:3.0.0  # ✅ Exact version
    depends_on:
      - midnight-node
    # ... other configuration

  wallet-indexer:
    image: midnightntwrk/wallet-indexer:3.0.0  # ✅ Same version as chain-indexer
    depends_on:
      - midnight-node
    # ... other configuration

  indexer-api:
    image: midnightntwrk/indexer-api:3.0.0  # ✅ Same version as other indexers
    depends_on:
      - chain-indexer
      - wallet-indexer
    ports:
      - "3000:3000"
    # ... other configuration
```

### Kubernetes

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: midnight-node
spec:
  template:
    spec:
      containers:
      - name: midnight-node
        image: midnightntwrk/midnight-node:0.18.0  # ✅ Exact version
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: chain-indexer
spec:
  template:
    spec:
      containers:
      - name: chain-indexer
        image: midnightntwrk/chain-indexer:3.0.0  # ✅ Exact version
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: wallet-indexer
spec:
  template:
    spec:
      containers:
      - name: wallet-indexer
        image: midnightntwrk/wallet-indexer:3.0.0  # ✅ Same version
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: indexer-api
spec:
  template:
    spec:
      containers:
      - name: indexer-api
        image: midnightntwrk/indexer-api:3.0.0  # ✅ Same version
```

## Common Issues and Solutions

### Issue: 404 on `/api/v3/graphql`

**Symptom:**
```bash
curl -X POST http://localhost:3000/api/v3/graphql
# Returns: HTTP/1.1 404 Not Found
```

**Cause:** Version mismatch between components, often due to using `latest` tag.

**Solution:**
1. Check the `NODE_VERSION` file for the correct Midnight Node version
2. Ensure all Indexer components use the same exact version
3. Update your Docker Compose or Kubernetes configuration with exact versions
4. Restart all services

### Issue: Different Author Values in Blocks Table

**Symptom:** Two Indexers running on different servers produce different `author` values for the same blocks.

**Cause:** This was a bug in versions prior to the fix for [Issue #548](https://github.com/midnightntwrk/midnight-indexer/issues/548).

**Solution:**
1. Upgrade to the latest version that includes the fix
2. Ensure both Indexers use the exact same version
3. If needed, re-index from genesis to ensure consistency

## Verification

### Check Component Versions

When the Indexer API starts, it will log version information:

```
╔════════════════════════════════════════════════════════════════════════════╗
║                    Midnight Indexer API Version Info                      ║
╠════════════════════════════════════════════════════════════════════════════╣
║  Indexer API Version:        3.0.0                                        ║
║  Expected Node Version:      0.18.0                                       ║
╠════════════════════════════════════════════════════════════════════════════╣
║  IMPORTANT: All Indexer components must use the SAME version:             ║
║    - chain-indexer:3.0.0                                                  ║
║    - wallet-indexer:3.0.0                                                 ║
║    - indexer-api:3.0.0                                                    ║
║                                                                            ║
║  NEVER use 'latest' tag in production - always specify exact versions!    ║
╚════════════════════════════════════════════════════════════════════════════╝
```

### Verify API Endpoints

Test that the v3 API is available:

```bash
# Should return GraphQL schema, not 404
curl -X POST http://localhost:3000/api/v3/graphql \
  -H "Content-Type: application/json" \
  -d '{"query": "{ __schema { types { name } } }"}'
```

## Upgrading

When upgrading to a new version:

1. Check the `NODE_VERSION` file for the new compatible Node version
2. Update **all** components simultaneously to the same version
3. Update the Midnight Node to the compatible version
4. Test the upgrade in a non-production environment first
5. Verify all API endpoints are accessible

## Support

If you encounter version-related issues:

1. Check this document for common solutions
2. Verify all versions match the requirements
3. Check the [GitHub Issues](https://github.com/midnightntwrk/midnight-indexer/issues) for similar problems
4. Open a new issue with:
   - Exact versions of all components
   - Configuration files (docker-compose.yml, etc.)
   - Error messages and logs

## References

- [Issue #597: API v3 endpoint 404](https://github.com/midnightntwrk/midnight-indexer/issues/597)
- [Issue #548: Different author values](https://github.com/midnightntwrk/midnight-indexer/issues/548)
- [NODE_VERSION file](./NODE_VERSION)
