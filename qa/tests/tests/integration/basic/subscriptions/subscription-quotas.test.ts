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

import '@utils/logging/test-logging-hooks';
import { IndexerWsClient } from '@utils/indexer/websocket-client';

const PER_CONNECTION_CAP = 20;
const REGISTRATION_WAIT_MS = 1_000;
const RESPONSE_TIMEOUT_MS = 5_000;
const WS_OPEN = 1;

const blockSubscriptionQuery = `
  subscription {
    blocks {
      hash
      height
    }
  }
`;

describe('subscription quotas (HAL-03 / SSE-196)', () => {
  let client: IndexerWsClient;

  beforeEach(async () => {
    client = new IndexerWsClient();
    await client.connectionInit();
  }, 30_000);

  afterEach(async () => {
    await client.connectionClose();
  });

  describe('per-connection concurrent subscription cap', () => {
    /**
     * The indexer enforces a per-connection concurrent active subscription cap
     * via the SubscriptionGuard acquired at each resolver entry. Default cap is
     * 20 (configurable via `infra.api.quota.max_concurrent_per_connection`).
     * The 21st concurrent subscription on a single WebSocket connection must
     * be rejected with a client error, while the WebSocket connection itself
     * stays open and the existing 20 subscriptions continue unaffected.
     *
     * @given a single WebSocket connection with 20 active block subscriptions
     * @when a 21st block subscription is opened on the same connection
     * @then the 21st subscription returns a client error mentioning the limit
     * @and the WebSocket connection stays open
     */
    test(`rejects the ${PER_CONNECTION_CAP + 1}th concurrent subscription on one connection`, async () => {
      const cleanups: Array<() => void> = [];

      for (let i = 0; i < PER_CONNECTION_CAP; i++) {
        const idx = i + 1;
        const cleanup = client.subscribe(blockSubscriptionQuery, {
          next: () => {
            /* drain quietly */
          },
          error: (err) => {
            throw new Error(
              `Subscription #${idx} of ${PER_CONNECTION_CAP} unexpectedly errored: ${err.message}`,
            );
          },
        });
        cleanups.push(cleanup);
      }

      await new Promise((resolve) => setTimeout(resolve, REGISTRATION_WAIT_MS));

      const rejected = await new Promise<Error | null>((resolve) => {
        const timeout = setTimeout(() => resolve(null), RESPONSE_TIMEOUT_MS);
        const cleanupExtra = client.subscribe(blockSubscriptionQuery, {
          next: () => {
            clearTimeout(timeout);
            cleanupExtra();
            resolve(null);
          },
          error: (err) => {
            clearTimeout(timeout);
            resolve(err as Error);
          },
        });
      });

      expect(
        rejected,
        `Expected the ${PER_CONNECTION_CAP + 1}th subscription to be rejected, but no error was returned within ${RESPONSE_TIMEOUT_MS}ms`,
      ).not.toBeNull();
      expect(rejected!.message.toLowerCase()).toContain('subscription limit exceeded');
      expect(
        client.getState(),
        `WebSocket connection should stay open after a single subscription is rejected, got ${IndexerWsClient.getStateName(client.getState())}`,
      ).toBe(WS_OPEN);

      cleanups.forEach((fn) => fn());
    }, 30_000);

    /**
     * The per-connection counter is decremented when a subscription ends,
     * via the `SubscriptionGuard` Drop. After 20 active subscriptions are
     * established and one is closed, the freed slot must allow a new
     * subscription to start on the same connection.
     *
     * @given 20 active block subscriptions on a single connection
     * @when one of the active subscriptions is closed
     * @then a new subscription opened on the same connection succeeds
     */
    test('frees a slot when an active subscription is closed', async () => {
      const cleanups: Array<() => void> = [];

      for (let i = 0; i < PER_CONNECTION_CAP; i++) {
        const cleanup = client.subscribe(blockSubscriptionQuery, {
          next: () => {
            /* drain quietly */
          },
        });
        cleanups.push(cleanup);
      }

      await new Promise((resolve) => setTimeout(resolve, REGISTRATION_WAIT_MS));

      const closed = cleanups.shift();
      closed!();

      await new Promise((resolve) => setTimeout(resolve, REGISTRATION_WAIT_MS));

      const succeeded = await new Promise<boolean>((resolve) => {
        const timeout = setTimeout(() => resolve(false), RESPONSE_TIMEOUT_MS);
        const cleanupExtra = client.subscribe(blockSubscriptionQuery, {
          next: () => {
            clearTimeout(timeout);
            cleanupExtra();
            resolve(true);
          },
          error: () => {
            clearTimeout(timeout);
            resolve(false);
          },
        });
      });

      expect(
        succeeded,
        `Expected a new subscription to succeed after one of the ${PER_CONNECTION_CAP} active subscriptions was closed (slot should be freed via Drop)`,
      ).toBe(true);

      cleanups.forEach((fn) => fn());
    }, 30_000);
  });
});
