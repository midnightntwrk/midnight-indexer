// This file is part of midnightntwrk/midnight-indexer
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// An idle websocket to a hosted indexer is closed roughly 60 seconds after the
// last message the server sent, without a close frame — the client sees a bare
// 1006. That alone would be unremarkable, except the progress subscription
// deliberately backs off while the chain is idle (since indexer 4.4.0: 30s
// doubling to 240s, ±20% jitter), so the server can stay quiet for longer than
// the idle limit and the subscription it is throttling gets dropped.
//
// `IndexerWsClient` works around this by sending a graphql-transport-ws `ping`
// every 25 seconds; the server's `pong` resets the idle timer.
//
// THESE ARE THE COUNTER TESTS. They pin down both halves of that claim:
//
//   1. with the keepalive OFF, an idle socket really is closed;
//   2. with the keepalive ON, the same idle socket survives well past the limit.
//
// Without (1) the workaround could quietly become unnecessary — or quietly
// insufficient — and nothing would notice. Without (2) we would not know the
// ping actually prevents the close rather than merely looking plausible.
//
// They are slow by nature: proving something does NOT happen for two and a half
// minutes takes two and a half minutes. That is why they live in the integration
// suite and not in a path that runs on every change.

import '@utils/logging/test-logging-hooks';
import log from '@utils/logging/logger';
import { randomBytes } from 'crypto';
import { env } from 'environment/model';
import type { TestContext } from 'vitest';
import { IndexerWsClient } from '@utils/indexer/websocket-client';

/** The observed idle limit. Measured against preview: 60.0s, three times over. */
const SERVER_IDLE_CLOSE_MS = 60_000;
/**
 * How long to sit idle before judging the socket.
 *
 * The subscription used below never emits, so the socket is idle from the
 * `connection_ack` onwards and the close is due one idle limit later. Two and a
 * half times the measured 60.0s leaves room for jitter and for a slower
 * environment without making the file any more expensive than it has to be.
 */
const OBSERVATION_WINDOW_MS = 150_000;
/** The whole point is to outlast the limit, so the test timeout must exceed it. */
const TEST_TIMEOUT_MS = OBSERVATION_WINDOW_MS + 90_000;

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * Subscribe to something that will never emit, sit idle, and report whether the
 * socket is still open.
 *
 * Openness is the whole question here, so it is read straight off the socket.
 * `assertSocketAlive()` must NOT be used: it also fails a socket that has merely
 * been silent too long, which is true of BOTH cases below by construction, so
 * either test would report "did not survive" whatever the server did.
 *
 * A contract-action subscription for a random address is used because it needs
 * no wallet, no seed, no funded balance and no key derivation: the address is
 * 32 random bytes, so nothing on chain will ever match it and the indexer has
 * nothing to send. That is exactly the condition under test.
 *
 * A BLOCK subscription must NOT be used here. Blocks arrive continuously on a
 * live chain, so the connection never goes idle and both tests below pass while
 * measuring nothing — which is what an earlier version of this file did.
 */
async function surviveIdle(client: IndexerWsClient, windowMs: number): Promise<boolean> {
  client.subscribeToContractActionEvents({ next: () => {} }, randomBytes(32).toString('hex'));
  await sleep(windowMs);

  const open = client.isSocketOpen();
  log.debug(`Socket after ${windowMs / 1000}s idle: ${open ? 'open' : 'closed'}`);
  return open;
}

describe('websocket keepalive', () => {
  afterEach(() => {
    // Restores INDEXER_WS_KEEPALIVE even when vitest times a test out while its
    // body is still suspended on the idle window: a `delete process.env...` in
    // that body's `finally` would not have run yet, and the next test would
    // build its client with the keepalive still disabled.
    vi.unstubAllEnvs();
  });

  // The idle close is not the indexer's own doing — its configuration has no
  // websocket idle timeout — it comes from the ingress in front of the hosted
  // environments. The undeployed stack has no such ingress, so there is no
  // behaviour there for this test to pin down.
  describe.skipIf(env.isUndeployedEnv())('an idle socket without the client keepalive', () => {
    /**
     * The close this keepalive exists to work around.
     *
     * @given a websocket with the client keepalive disabled
     * @when it holds a subscription that emits nothing and stays idle past the
     *   60s idle limit
     * @then the socket is found closed
     */
    test(
      'should be closed while idle past the idle limit',
      async (ctx: TestContext) => {
        ctx.task!.meta.custom = { labels: ['Subscription', 'WebSocket', 'Keepalive'] };

        vi.stubEnv('INDEXER_WS_KEEPALIVE', 'off');
        const client = new IndexerWsClient();
        try {
          await client.connectionInit();
          const survived = await surviveIdle(client, OBSERVATION_WINDOW_MS);

          // If this ever fails, idle sockets are no longer being closed. That is
          // good news, but it means the keepalive is no longer load-bearing and
          // this test — not the keepalive — is what should be reconsidered first.
          expect(
            survived,
            `The socket was still open after ${OBSERVATION_WINDOW_MS / 1000}s idle, though an ` +
              `unpinged socket was measured closing after ${SERVER_IDLE_CLOSE_MS / 1000}s idle. ` +
              'If the environment changed, revisit the keepalive in IndexerWsClient.',
          ).toBe(false);
        } finally {
          await client.connectionClose();
        }
      },
      TEST_TIMEOUT_MS,
    );
  });

  describe('an idle socket with the client keepalive', () => {
    /**
     * The workaround itself: the server's pong resets the idle timer.
     *
     * @given a websocket with the client keepalive enabled (the default)
     * @when it holds a subscription that emits nothing and stays idle past the
     *   60s idle limit
     * @then the socket is still open
     */
    test(
      'should stay open while idle past the idle limit',
      async (ctx: TestContext) => {
        ctx.task!.meta.custom = { labels: ['Subscription', 'WebSocket', 'Keepalive'] };

        const client = new IndexerWsClient();
        try {
          await client.connectionInit();
          const survived = await surviveIdle(client, OBSERVATION_WINDOW_MS);

          expect(
            survived,
            `The socket closed within ${OBSERVATION_WINDOW_MS / 1000}s despite the keepalive. ` +
              'Either the ping is not being sent, or the server no longer answers it — ' +
              'check for a `pong` in the websocket debug log.',
          ).toBe(true);
        } finally {
          await client.connectionClose();
        }
      },
      TEST_TIMEOUT_MS,
    );
  });
});
