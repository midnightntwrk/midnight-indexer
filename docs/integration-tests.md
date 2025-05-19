# Integration Tests

Integration tests run against actual services, including a Node, PostgreSQL, and NATS. They verify the correctness of GraphQL queries and subscriptions, as well as data indexing logic.

## Requirements

- Docker: For running containers.
- The `testcontainers` and `testcontainers-modules` crates handle spinning up services.
- `just`: For uniform test execution.

## Running Tests

```bash
just test
```

This will:
- Build necessary executables.
- Run unit and integration tests using `cargo nextest`.
- Start necessary containers (e.g., NATS, PostgreSQL, Node).

If tests pass, coverage and reports are generated. See also `just coverage-report` for generating coverage reports.

## Test Structure

- `indexer-tests/`: Contains integration tests:
    - `api_cloud.rs`: Tests the GraphQL API in a cloud environment.
    - `e2e_cloud.rs`: End-to-end tests simulating real node interactions.
    - Additional helpers and queries are stored under `indexer-tests/tests/`.

## Debugging

Run tests with `RUST_LOG=debug` for more details. You can also set `DEBUG=testcontainers*` to see container logs.
