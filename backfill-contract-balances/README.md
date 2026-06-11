# backfill-contract-balances

One-off, idempotent backfill recomputing missing `contract_balances` rows from the
per-action contract state already stored in `contract_actions.state` (issue #1245).

Every release from 3.0.0 to 4.3.3 silently extracted empty contract balances, so
`contract_balances` is empty for all history indexed by those versions. The fix (#1246)
corrects extraction for newly indexed actions; this tool repairs the already-indexed
history. It decodes each stored state with the fixed `ContractState::balances()` (using
the `protocol_version` of the action's transaction) and inserts the resulting rows with
`ON CONFLICT DO NOTHING`.

Safety properties (full argument in #1245): each row is a pure projection of per-action
frozen inputs, with no linkage to neighbouring blocks; no component reads
`contract_balances` during indexing; `UNIQUE (contract_action_id, token_type)` makes
re-runs no-ops; only existing action ids are touched while the live chain-indexer writes
only new ones; written rows are byte-identical to fixed-code rows (same
`save_contract_balances` encodings). The tool exits non-zero if any state fails to
decode.

**Precondition:** the target deployment must run an indexer with the #1246 fix
(4.3.4+), otherwise newly indexed actions keep getting no balances and the gap reopens
at the head.

## Configuration

Environment variables, matching the deployed components:

| Variable | Default | Notes |
|---|---|---|
| `APP__INFRA__STORAGE__HOST` / `PORT` / `DBNAME` / `USER` | localhost / 5432 / indexer / indexer | cloud (Postgres) |
| `APP__INFRA__STORAGE__PASSWORD` | required | cloud |
| `APP__INFRA__STORAGE__SSLMODE` | prefer | cloud: disable, prefer, require |
| `APP__INFRA__STORAGE__CNN_URL` | target/data/indexer.sqlite | standalone (SQLite) |
| `APPLY` | 0 | 0 = dry-run (read-only, prints what it would insert), 1 = insert |
| `BATCH` | 500 | actions per scan batch |

## Running locally

Against the local cloud stack (docker compose Postgres) or the standalone SQLite
database:

```bash
just run-backfill-contract-balances              # dry-run, cloud
just run-backfill-contract-balances apply="1"    # insert, cloud
just feature=standalone run-backfill-contract-balances    # dry-run, SQLite
```

The tool never creates schema (it is a repair, not a component): the target database must
already be migrated, i.e. the indexer components must have run against it at least once.
On an unmigrated database it fails with "relation contract_balances does not exist".

## Running on an environment

The tool has its own image, `ghcr.io/midnight-ntwrk/backfill-contract-balances`, built
ON DEMAND only: run the `build-backfill-image` workflow from the GitHub Actions UI on
the ref whose code should ship (e.g. the release tag). It is deliberately not part of
the regular `build-indexer-images` pipeline since this is a one-off repair tool.

Run it in-cluster as a Job (or via the chart's gated Job template once available), with
DB credentials from the indexer's connection secret:

```yaml
apiVersion: batch/v1
kind: Job
metadata:
  name: backfill-contract-balances
spec:
  backoffLimit: 0
  ttlSecondsAfterFinished: 86400
  template:
    metadata:
      labels:
        app.kubernetes.io/name: backfill-contract-balances
    spec:
      restartPolicy: Never
      containers:
        - name: backfill-contract-balances
          image: ghcr.io/midnight-ntwrk/backfill-contract-balances:<tag>
          env:
            - name: APP__INFRA__STORAGE__HOST
              valueFrom:
                secretKeyRef:
                  name: rds-connection-details-indexer
                  key: endpoint
            - name: APP__INFRA__STORAGE__PASSWORD
              valueFrom:
                secretKeyRef:
                  name: rds-connection-details-indexer
                  key: password
            - name: APP__INFRA__STORAGE__SSLMODE
              value: require
            - name: APPLY
              value: "0" # dry-run first; rerun with "1" to insert
          resources:
            requests: { cpu: 250m, memory: 256Mi }
            limits: { cpu: "1", memory: 512Mi }
          securityContext:
            runAsNonRoot: true
            allowPrivilegeEscalation: false
            capabilities: { drop: [ALL] }
            seccompProfile: { type: RuntimeDefault }
```

Dry-run first (`APPLY=0`), check the log summary, then rerun with `APPLY=1`. Re-running
is always safe.

## Tests

`cargo nextest run -p backfill-contract-balances --features cloud` (unit tests plus an
end-to-end Postgres test: testcontainer, production migrations, seeded real preview
contract state, dry-run/apply/idempotency with byte-exact row assertions) and
`--features standalone` (the same end-to-end against SQLite).
