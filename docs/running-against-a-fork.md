# Running the indexer against a forked Midnight network

The indexer's usual local setup (`just run-node`, `docker-compose.yaml`) runs
against a fresh dev chain. This guide runs it against a **local fork of a real
network** instead: a snapshot of Preview (or another well-known network)
restored onto local nodes, with the real validators swapped for local ones. The
indexer then sees real chain history and real ledger state, which a dev chain
cannot reproduce.

Use it to:

- index real network history end to end, e.g. to check a decoding change or a
  new node version against everything a network has produced so far;
- rehearse a runtime or node upgrade on real state before it reaches the
  network;
- reproduce a bug that only shows up with a real network's data.

For everyday work on a single component, the dev chain is faster.

## How it works

The fork itself comes from
[midnight-node](https://github.com/midnightntwrk/midnight-node)'s
`local-environment/` tooling. It restores the snapshot into each node's data
directory, rewrites the authority set to local mock validators with the
mock-authorities tool, adds one fork block on top of the snapshot's finalized tip, and starts the
nodes as a docker compose project. The fork then produces and finalizes new
blocks of its own.

This repo adds `docker-compose.midnight-fork.yaml`, which runs the cloud-mode
indexer stack as a separate compose project (`midnight-indexer-fork`) next to
the fork. The two meet through a small contract:

1. The fork's compose project uses a **stable docker network name**,
   `midnight-fork-<network>`.
2. Every successful bring-up writes a **manifest env file** at
   `<midnight-node>/local-environment/artifacts/<network>.manifest.env` with
   `MIDNIGHT_FORK_NETWORK` (the docker network name),
   `MIDNIGHT_FORK_NETWORK_ID`, `MIDNIGHT_FORK_NODE_TAG`,
   `MIDNIGHT_FORK_NODE_WS` (in-network node RPC, e.g. `ws://node1:9944`) and
   `MIDNIGHT_FORK_NODE_WS_HOST` (host-published, e.g. `ws://localhost:9950`).

Only `chain-indexer` joins the fork's network. Postgres, NATS,
`wallet-indexer` and `indexer-api` stay on the overlay's private network.
Because the two compose projects are independent, the indexer stack can be
restarted or rebuilt without touching the fork, and vice versa.

`just fork-up` and `just fork-down` drive both halves.

## Prerequisites

- Docker with Compose v2, `just`, `git`, and Node.js 20 or newer with `npm`
  (for midnight-node's `local-environment`).
- Disk space for one copy of the snapshot per node. A Preview snapshot is about
  300 MB; a mainnet snapshot is about 119 GB per node.
- Pull access to `ghcr.io/shieldedtech/mock-authorities`. The image is not
  public yet, so run `docker login ghcr.io` with an account that can read it.
- A node image (`NODE_IMAGE`) whose version the indexer supports: see
  `NODE_VERSIONS`.
- A snapshot URL. Published snapshots are listed at
  <https://snapshots.midnight.network>; pick one whose node version (in the
  file name, e.g. `-v2.1.101-`) matches `NODE_IMAGE`. local-environment only
  accepts `http(s)://` URLs, not local paths.

## Quick start

A Preview fork with three validators on node 2.1.101:

```bash
# First bring-up: downloads and restores the snapshot.
NODE_IMAGE=ghcr.io/midnightntwrk/midnight-node:2.1.101 \
  just fork-up preview --num-validators 3 \
  --from-snapshot https://snapshots.midnight.network/preview/<path>/preview-epoch-<epoch>-block-<block>-v2.1.101-<hash>.tar.gz

# Later runs reuse the restored state:
NODE_IMAGE=ghcr.io/midnightntwrk/midnight-node:2.1.101 just fork-up preview --num-validators 3

# Tear everything down (indexer overlay first, then the fork):
just fork-down preview
```

Arguments after the network name are passed to local-environment's
`npm run run:<network>`.

`preview` is the network this guide is tested with. Other well-known networks
work the same way, but a mainnet fork needs far more disk and takes hours to
index.

## Checking that it works

`indexer-api` serves GraphQL at `http://localhost:8088/api/v4/graphql`.
chain-indexer first indexes the snapshot's history, at about 135 blocks/s in
our Preview test, so a Preview fork catches up in a few minutes. It then
follows the fork's new blocks.

```bash
# Latest indexed block; it keeps rising once chain-indexer has caught up.
curl -s localhost:8088/api/v4/graphql -H 'content-type: application/json' \
  -d '{"query":"{ block { height hash } }"}'

# chain-indexer logs "caught_up":true once it reaches the fork's head.
docker logs -f midnight-indexer-fork-chain-indexer-1 | grep caught_up
```

The fork's nodes are also reachable from the host at
`MIDNIGHT_FORK_NODE_WS_HOST` from the manifest, e.g. to check finality with
`chain_getFinalizedHead`.

## What `just fork-up` does

1. Sparse-clones midnight-node (only `local-environment/`) into
   `.midnight-node/` and checks out `main`: no node release yet has the
   local-environment that converts the boot node along with the validators
   (midnight-node#2308), without which the fork's validators never peer. The
   ref is fetched on every run, so changing it or moving a branch takes effect,
   and `npm ci` reruns whenever `package-lock.json` changes.
2. Brings up the fork via `npm run run:<network>`, with mock-authorities
   `ghcr.io/shieldedtech/mock-authorities:14cee4b`. Earlier images leave the
   fork unfinalized and write a fork block that chain-indexer cannot decode.
3. Checks that the indexer images being run support the fork's node version
   exactly, using the `NODE_VERSIONS` of the indexer release named by
   `INDEXER_TAG`, or this checkout's when `INDEXER_TAG` is not a release.
   chain-indexer rejects a node version it has no metadata for (see
   [updating-node-version.md](updating-node-version.md)). A `NODE_IMAGE`
   without a tag, or pinned by digest, cannot be checked.
4. Starts the overlay with `docker compose --env-file <manifest>`.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `NODE_IMAGE` | (required) | midnight-node image for the fork's nodes. |
| `MIDNIGHT_NODE_REF` | `main` | midnight-node branch, tag or commit SHA to clone. |
| `MIDNIGHT_NODE_DIR` | `.midnight-node` | Use your own midnight-node checkout instead; it is used as-is, never fetched or switched. |
| `MIDNIGHT_NODE_REPO` | `https://github.com/midnightntwrk/midnight-node` | Repository to clone. |
| `MOCK_AUTHORITIES_IMAGE` | `ghcr.io/shieldedtech/mock-authorities:14cee4b` | Image that converts the snapshot. |
| `INDEXER_TAG` | `4.4.101` | Indexer image tag, e.g. a locally built one. |
| `IMAGE_REGISTRY` | `midnightntwrk` | Registry or namespace for the indexer images. |
| `INDEXER_API_PORT` | `8088` | Host port for `indexer-api`. |
| `FORK_SKIP_VERSION_CHECK` | unset | Set to `1` to start the overlay even if the version check fails. |

## Iterating on the indexer

Restarting the overlay does not touch the fork. To test a local build, build
the images (`just build-docker-image <package>`) and point `INDEXER_TAG` at
them:

```bash
manifest=.midnight-node/local-environment/artifacts/preview.manifest.env
docker compose --env-file $manifest -f docker-compose.midnight-fork.yaml down --volumes
INDEXER_TAG=dev docker compose --env-file $manifest -f docker-compose.midnight-fork.yaml up -d
```

`down --volumes` drops the indexed state, so chain-indexer re-indexes from
genesis on the next start. Leave out `--volumes` to keep it.

To run an indexer component on the host instead of in docker, use the
host-published node endpoint and the network ID from the manifest:

```bash
just run-chain-indexer node=ws://localhost:9950 network_id=preview
```

## Troubleshooting

- **`node <version> is not supported by indexer <tag>`**: the indexer release
  has no metadata for that node version. Use an indexer release that lists it
  in `NODE_VERSIONS`, or a node image and snapshot of a supported version.
- **chain-indexer stops at the fork block** with `protocol version header
  missing from block` or `Cannot decode extrinsic at index 0`: the fork was
  converted by a mock-authorities image older than `14cee4b`. Check
  `MOCK_AUTHORITIES_IMAGE`, tear down and bring the fork up again from the
  snapshot.
- **Finality stays at the fork block** while the nodes keep authoring: an old
  mock-authorities image again, or validators that cannot see each other.
- **Nodes log `Idle (0 peers)`**, each on its own chain: the midnight-node
  checkout's local-environment predates midnight-node#2308 and left the boot
  node on the real chain. Use `main` (the default) or a later release.
- **`No .network.yaml file found for namespace '<network>'`**: a
  local-environment bug on midnight-node `main` that midnight-node#2416 fixes.
  Until that merges, set `MIDNIGHT_NODE_REF=fix/local-env-glob-minimatch`.

## Caveats

- Always tear down the overlay before stopping the fork (this is what
  `just fork-down` does): the fork cannot remove its docker network while
  overlay containers are attached.
- Don't combine this overlay with the fork's in-tree `withindexer` compose
  profile: that runs a second copy of this stack with fixed container names.
- The default `APP__INFRA__SECRET`/passwords in the overlay are for local fork
  testing only. If you override the secret, it must be ≥ 64 hex chars and
  contain letters (an all-digit value is type-inferred as an integer and
  rejected).
- `spo-indexer` is not part of the overlay: it follows Cardano stake-pool data
  via Blockfrost, which a local fork does not reproduce.
