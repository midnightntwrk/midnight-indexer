# Running the indexer against a forked Midnight network

[midnight-node](https://github.com/midnightntwrk/midnight-node) ships fork
tooling in its `local-environment/` directory: it restores a snapshot of a
well-known network (e.g. mainnet), rewrites the authority set to local mock
validators, and brings the fork up as a docker compose project. This lets the
indexer be exercised against **real network state** — including runtime-upgrade
rehearsals — instead of a fresh dev chain.

The integration contract is deliberately small:

1. The fork's compose project uses a **stable docker network name**
   (mainnet: `midnight-fork-mainnet`).
2. Every successful bring-up writes a **manifest env file** at
   `<midnight-node>/local-environment/artifacts/<network>.manifest.env`
   containing `MIDNIGHT_FORK_NETWORK` (docker network name),
   `MIDNIGHT_FORK_NETWORK_ID`, `MIDNIGHT_FORK_NODE_TAG`,
   `MIDNIGHT_FORK_NODE_WS` (in-network node RPC, e.g. `ws://node1:9944`), and
   `MIDNIGHT_FORK_NODE_WS_HOST` (host-published, e.g. `ws://localhost:9950`).

`docker-compose.midnight-fork.yaml` runs the cloud-mode stack as its own
compose project (`midnight-indexer-fork`), sourcing its configuration from the
manifest. Only `chain-indexer` joins the fork's network; postgres, NATS,
wallet-indexer, and indexer-api stay on the project's private network. Because
the two compose projects are independent, the indexer stack can be restarted
or rebuilt without touching the fork (and vice versa).

## Quick start

```bash
# First bring-up: restores the snapshot (an http(s) URL)
NODE_IMAGE=ghcr.io/midnightntwrk/midnight-node:2.1.101 \
  just fork-up preview --num-validators 3 \
  --from-snapshot https://snapshots.midnight.network/preview/<path>/preview-epoch-<epoch>-block-<block>-v2.1.101-<hash>.tar.gz

# Subsequent runs reuse the restored state:
NODE_IMAGE=ghcr.io/midnightntwrk/midnight-node:2.1.101 just fork-up preview --num-validators 3

# Tear everything down (indexer overlay first, then the fork):
just fork-down preview
```

`just fork-up`:

1. Sparse-clones midnight-node (only `local-environment/`) into
   `.midnight-node/` and checks out `main`: no node release yet has the
   local-environment that converts the boot node along with the validators
   (midnight-node#2308), without which the fork's validators never peer. Set
   `MIDNIGHT_NODE_REF` to a branch, tag or commit SHA to pin something else.
   The ref is fetched on every run, so changing it or moving a branch takes
   effect, and `npm ci` reruns whenever `package-lock.json` changes. Set
   `MIDNIGHT_NODE_DIR` to use your own checkout as-is instead.
2. Brings up the fork via `npm run run:<network>`, with mock-authorities
   `ghcr.io/shieldedtech/mock-authorities:14cee4b`. Earlier images leave the
   fork unfinalized and write a fork block that chain-indexer cannot decode.
   Override with `MOCK_AUTHORITIES_IMAGE`.
3. Checks that the indexer images being run support the fork's node version
   exactly, using the `NODE_VERSIONS` of the indexer release named by
   `INDEXER_TAG`, or this checkout's when `INDEXER_TAG` is not a release.
   chain-indexer rejects a node version it has no metadata for (see
   [updating-node-version.md](updating-node-version.md)). A `NODE_IMAGE`
   without a tag, or pinned by digest, cannot be checked. Set
   `FORK_SKIP_VERSION_CHECK=1` to proceed anyway.
4. Starts this overlay with `docker compose --env-file <manifest>`.

The GraphQL API is then available at `http://localhost:8088` (override with
`INDEXER_API_PORT`). Indexer images default to the released
`midnightntwrk/*:4.4.101`; override with `INDEXER_TAG` / `IMAGE_REGISTRY` to test
locally built images (`just build-docker-image <package>`).

## Iterating on the indexer only

Restarting the overlay does not touch the fork:

```bash
manifest=.midnight-node/local-environment/artifacts/mainnet.manifest.env
docker compose --env-file $manifest -f docker-compose.midnight-fork.yaml down --volumes
INDEXER_TAG=dev docker compose --env-file $manifest -f docker-compose.midnight-fork.yaml up -d
```

Note that `down --volumes` drops the indexed state: chain-indexer re-indexes
from genesis on the next start (roughly 19 blocks/s — reaching a mainnet-height
tip takes many hours; verifying that blocks are flowing takes seconds).

For host-side (non-docker) workflows, source the manifest and use the
host-published endpoint directly:

```bash
just run-chain-indexer node=ws://localhost:9950 network_id=mainnet
```

## Caveats

- Always tear down the overlay before stopping the fork (this is what
  `just fork-down` does): the fork cannot remove its docker network while
  overlay containers are attached.
- Don't combine this overlay with the fork's in-tree `withindexer` compose
  profile — that runs a second copy of this stack with fixed container names.
- The default `APP__INFRA__SECRET`/passwords in the overlay are for local fork
  testing only. If you override the secret, it must be ≥ 64 hex chars and
  contain letters (an all-digit value is type-inferred as an integer and
  rejected).
- `spo-indexer` is not part of the overlay: it follows Cardano stake-pool data
  via Blockfrost, which a local fork does not reproduce.
