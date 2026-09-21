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

// The indexer closes a websocket roughly 60 seconds after the last message IT
// sent, without a close frame — the client sees a bare 1006. That alone would be
// unremarkable, except the progress subscription deliberately backs off while the
// chain is idle (since indexer 4.4.0: 30s doubling to 240s, ±20% jitter), so the
// server can stay quiet for longer than its own limit and drop the very
// subscription it is throttling.
//
// `IndexerWsClient` works around this by sending a graphql-transport-ws `ping`
// every 25 seconds; the server's `pong` resets its idle timer.
//
// THESE ARE THE COUNTER TESTS. They pin down both halves of that claim:
//
//   1. with the keepalive OFF, an idle socket really is closed by the server;
//   2. with the keepalive ON, the same idle socket survives well past the limit.
//
// Without (1) the workaround could quietly become unnecessary — or quietly
// insufficient — and nothing would notice. Without (2) we would not know the
// ping actually prevents the close rather than merely looking plausible.
//
// They are slow by nature: proving something does NOT happen for two minutes
// takes two minutes. That is why they live in the integration suite and not in
// an e2e path that runs on every change.

import '@utils/logging/test-logging-hooks';
import log from '@utils/logging/logger';
import { randomBytes } from 'crypto';
import { IndexerWsClient } from '@utils/indexer/websocket-client';

/** The server's observed idle limit. Measured against preview: 60.0s, three times over. */
const SERVER_IDLE_CLOSE_MS = 60_000;
/**
 * How long to sit idle before judging the socket.
 *
 * It must outlast a progress gap long enough to trip the idle limit. The
 * progress backoff runs 30s, 60s, 120s, 240s, so the first gap that exceeds 60s
 * is the second one — reached a little over 90s in. Measured closes landed at
 * 94s, 147s and 153s, so the window has to clear the worst of those with room
 * for the ±20% jitter.
 */
const OBSERVATION_WINDOW_MS = 240_000;
/** The whole point is to outlast the limit, so the test timeout must exceed it. */
const TEST_TIMEOUT_MS = OBSERVATION_WINDOW_MS + 90_000;

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * Subscribe to something that will never emit, sit idle, and report whether the
 * socket survived.
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
  try {
    client.assertSocketAlive();
    return true;
  } catch (error) {
    log.debug(`Socket did not survive the idle window: ${String(error)}`);
    return false;
  }
}

describe('websocket keepalive', () => {
  /**
   * @given a websocket with the client keepalive disabled
   * @when it holds a subscription and stays idle past the server's idle limit
   * @then the server closes it, which is the behaviour the keepalive works around
   */
  test(
    'an unpinged idle socket is closed by the indexer',
    async () => {
      process.env.INDEXER_WS_KEEPALIVE = 'off';
      const client = new IndexerWsClient();
      try {
        await client.connectionInit();
        const survived = await surviveIdle(client, OBSERVATION_WINDOW_MS);

        // If this ever fails, the server stopped closing idle sockets. That is
        // good news, but it means the keepalive is no longer load-bearing and
        // this test — not the keepalive — is what should be reconsidered first.
        expect(
          survived,
          `The socket was still alive after ${OBSERVATION_WINDOW_MS / 1000}s idle, though the ` +
            `indexer was measured closing idle sockets after ${SERVER_IDLE_CLOSE_MS / 1000}s. ` +
            'If the server changed, revisit the keepalive in IndexerWsClient.',
        ).toBe(false);
      } finally {
        delete process.env.INDEXER_WS_KEEPALIVE;
        await client.connectionClose();
      }
    },
    TEST_TIMEOUT_MS,
  );

  /**
   * @given a websocket with the client keepalive enabled (the default)
   * @when it holds a subscription and stays idle past the server's idle limit
   * @then the socket is still usable, because the server's pong resets its timer
   */
  test(
    'a pinged idle socket survives past the indexer idle limit',
    async () => {
      const client = new IndexerWsClient();
      try {
        await client.connectionInit();
        const survived = await surviveIdle(client, OBSERVATION_WINDOW_MS);

        expect(
          survived,
          `The socket died within ${OBSERVATION_WINDOW_MS / 1000}s despite the keepalive. ` +
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
