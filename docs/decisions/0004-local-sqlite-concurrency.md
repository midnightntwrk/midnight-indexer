# Local SQLite Concurrency Issue

## Context and Problem Statement

Our Rust-based indexer supports two primary modes:
- **Local mode**, using a single-file SQLite database for simplicity and standalone demos,
- **Cloud mode**, using Postgres and other services for higher concurrency and production use.

While SQLite is convenient in standalone mode (no external DB needed, quick setup), we encountered persistent `(code: 5) database is locked` errors under higher concurrency writes. These lock issues arise because SQLite typically locks entire tables or the file during write transactions, causing collisions when multiple tasks attempt concurrent writes.

Relevant bug ticket: [PM-15038: "Database is locked" error](https://shielded.atlassian.net/browse/PM-15038)

## Decision Drivers

- **Ease of standalone development:** We want a simple, all-in-one solution with minimal setup.
- **Mitigate file-locking errors:** Rapid, concurrent writes in SQLite lead to lock contention and errors.
- **Maintain existing SQLx codebase:** Switching libraries (rusqlite, libSQL) would be a large rework with no guaranteed concurrency benefits.
- **Acceptable compromise:** Local mode is not intended for high-scale concurrency; performance is less critical than reliability and simplicity.

## Considered Options

1. **Limit concurrency in the wallet indexer application**
    - Example: `wallet_indexer_application.parallelism = 1`
    - Pros: Straightforward to implement, no changes to the DB layer.
    - Cons: Only solves concurrency from wallet indexer tasks; other components might still write concurrently (e.g., chain indexer, indexer API mutations).

2. **Use SQLite “in-memory” or advanced PRAGMAs (WAL, busy_timeout, etc.)**
    - Pros: Faster I/O if purely in-memory; WAL can allow better read-write concurrency.
    - Cons: In testing, heavy concurrency still triggered locks/deadlocks. No guaranteed fix for frequent concurrent writes.

3. **Switch away from SQLx + SQLite**
    - Pros: Potentially find an alternative embedded file DB with better concurrency semantics (not known).
    - Cons: Substantial rework of the entire codebase, no guarantee that other file-based DBs avoid these same locking issues.

4. **Set `SqlitePoolOptions::max_connections(1)`**
    - Pros: Ensures a single active connection, effectively serializing writes to avoid lock collisions.
    - Cons: Loses any concurrent DB operation benefits—only one transaction at a time.

## Decision Outcome

We will **set `max_connections(1)`** in the SQLite pool for standalone mode. This single-connection strategy serializes all writes and prevents `(code: 5) database is locked` errors during concurrent tasks. This is acceptable because:

- **Local mode** is inherently for single-user usage (or demos, small-scale tests).
- We do not require high-performance writes in standalone setups.
- Our **cloud mode** with Postgres remains the solution for real-world concurrency needs for multiple users.

Also, we document log-level recommendations for spinning hard disk users (e.g., WARN or TRACE for better performance). INFO/DEBUG can slow down the app.

### Consequences

- **Good**: No more persistent lock errors or concurrency collisions in standalone mode.
- **Acceptable**: Parallel reads might still queue behind any write transaction, but standalone usage typically won’t demand heavy concurrency.
- **Acknowledged**: We remain on SQLx + SQLite for standalone mode without codebase churn. Other file-based or in-process DBs would face similar concurrency constraints.

## Additional Notes

- **Why Not Another DB?**
    - Rusqlite (previous version is already used by sqlx) or libSQL do not inherently solve file-level locking constraints. They would still require architectural changes or concurrency throttling.
    - We also rely on SQLx for migrations, type checks, and a unified approach across both SQLite (standalone) and Postgres (cloud).
- **If Higher Local Concurrency is Needed**: Users should switch to the **cloud mode** (Postgres) or accept that multiple write tasks might block each other in standalone mode.

## References

- **Bug Ticket**: [PM-15038: “Database is locked” error](https://shielded.atlassian.net/browse/PM-15038)
- **Existing Local vs. Cloud Indexer**: [Docs on Local / Cloud Architecture](../running.md)
- **SQLite Locking Model**: [SQLite Lock Documentation](https://sqlite.org/lockingv3.html)
- **Scala Indexer's approach (similar to us)**: [Slack conversation with the Scala indexer team](https://shielded.slack.com/archives/C080ARCQ8LS/p1737369844909979?thread_ts=1737366347.635009&cid=C080ARCQ8LS)
- **Test report posted in the team Slack channel**: [Test Report On Slack](https://shielded.slack.com/archives/C080ARCQ8LS/p1737398028062129)
