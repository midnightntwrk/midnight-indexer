# Unit Tests

## Overview

Unit tests exercise the pure parts of the test harness itself, under `utils/`, with no
indexer, no Docker and no `TARGET_ENV`. They are the only project that runs on every pull
request touching `qa/`, so anything the other projects rely on and that can be checked
in isolation belongs here.

## Test Scope

- **Retry helper** (`utils/retry-helper.ts`): the retry budget, and the `NonRetryableError`
  bypass that makes a poll fail after one attempt instead of running out its budget
- **WebSocket liveness** (`utils/indexer/websocket-client.ts`): `assertSocketAlive()` on a
  closed socket, on 90s of silence with the keepalive running, and the deliberate absence of
  the silence check when the keepalive is off (`INDEXER_WS_KEEPALIVE=off`)

The slow counterpart, which proves against a real deployed indexer that an idle socket is
dropped without the keepalive and survives with it, lives in the integration project:
`tests/integration/basic/subscriptions/websocket-keepalive.test.ts`.

## Conventions

- Modules that touch the environment at import time (`environment/model`) and the logger
  (which creates a log directory on import) are replaced with `vi.mock` in the test file.
- The global `WebSocket` is replaced with `vi.stubGlobal`; the clock with `vi.setSystemTime`
  on real timers, since the code under test reads `Date.now()` rather than waiting on timers.
- A test that needs the environment is in the wrong project.

## Execution

```bash
bun run test:unit
```

The whole project runs in well under a second.
