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

// Integration tests for c2m-bridge GraphQL subscriptions: bridgeEvents and
// bridgeBalance (#942).
//
// Subscription mechanics: bridgeEvents backfills-then-live-tails — it replays
// matching historical events from the optional `from` cursor (an event id) and
// then streams new ones; there is no progress sentinel and no toBlock bound.
// bridgeBalance(address) emits the current balance immediately on connect, then
// re-emits on every relevant event. A claim is a BridgeClaimTransaction surfaced
// via the unshieldedTransactions query — there is no bridgeClaims subscription.
//
// Test-data reality (2026-07): bridge events are cross-chain and only
// BridgeUserTransfer has data, only on devnet. The immediate zero-balance frame
// needs only the surface; the historical replay needs a real UserTransfer and
// ctx.skips otherwise. Variant/recipient filters, reconnection continuity and
// balance-update-on-event stay test.todo until a data source can produce them.
//   test.todo → needs bridge data not yet producible in any test environment.
//   test.skip → blocked on a specific in-flight feature (noted inline).
//
// Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/942

import log from '@utils/logging/logger';
import { env } from 'environment/model';
import type { TestContext } from 'vitest';
import '@utils/logging/test-logging-hooks';
import {
  IndexerWsClient,
  BridgeEventSubscriptionResponse,
  BridgeBalanceSubscriptionResponse,
} from '@utils/indexer/websocket-client';
import type { BridgeUserTransfer } from '@utils/indexer/indexer-types';

const EMPTY_RECIPIENT = '0'.repeat(64);
const ZERO_U128 = '0'.repeat(32);

let surfacePresent = false;
let sampleUserTransfer: BridgeUserTransfer | null = null;

function safeUnsubscribe(unsubscribe: () => void): void {
  try {
    unsubscribe();
  } catch (error) {
    log.debug(`Ignoring unsubscribe error during teardown: ${String(error)}`);
  }
}

/**
 * Probes the bridge query surface over HTTP to decide surface presence and pick
 * a sample UserTransfer. Kept self-contained (a raw fetch, not the query client)
 * so this subscription PR does not depend on the #941 query plumbing.
 */
async function probeBridgeSurface(): Promise<{
  present: boolean;
  sample: BridgeUserTransfer | null;
}> {
  const apiVersion = process.env.INDEXER_API_VERSION?.trim() || 'v4';
  const url = `${env.getIndexerHttpBaseURL()}/api/${apiVersion}/graphql`;
  const query = `query {
    bridgeEvents(variant: USER_TRANSFER, limit: 1) {
      __typename
      ... on BridgeUserTransfer { id blockHeight midnightTxHash cardanoTxHash amount recipient }
    }
  }`;
  try {
    const response = await fetch(url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ query }),
      signal: AbortSignal.timeout(15_000),
    });
    const body = (await response.json()) as {
      data?: { bridgeEvents: BridgeUserTransfer[] };
      errors?: unknown[];
    };
    if (!response.ok || body.errors || !body.data) {
      return { present: false, sample: null };
    }
    const sample =
      body.data.bridgeEvents.find((e) => e.__typename === 'BridgeUserTransfer') ?? null;
    return { present: true, sample };
  } catch (error) {
    log.warn(`Bridge surface probe failed: ${String(error)}`);
    return { present: false, sample: null };
  }
}

describe.skipIf(env.isUndeployedEnv())('bridge subscriptions', () => {
  let wsClient: IndexerWsClient;

  beforeAll(async () => {
    const probe = await probeBridgeSurface();
    if (!probe.present) {
      log.warn(`Bridge surface not present on ${env.getCurrentEnvironmentName()}; skipping`);
      return;
    }
    surfacePresent = true;
    sampleUserTransfer = probe.sample;
  }, 30_000);

  beforeEach(async () => {
    wsClient = new IndexerWsClient();
    await wsClient.connectionInit();
  }, 30_000);

  afterEach(async () => {
    await wsClient.connectionClose();
  });

  describe('bridgeEvents', () => {
    /**
     * @given an environment with at least one indexed BridgeUserTransfer
     * @when a bridgeEvents subscription is opened with from = <sampleId - 1>
     * @then the known UserTransfer is replayed with id >= sampleId in ascending order
     */
    test('should replay historical events from the given cursor id', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Subscription', 'Bridge', 'UserTransfer'] };
      if (!surfacePresent) return ctx.skip();
      if (!sampleUserTransfer) return ctx.skip(true, 'no BridgeUserTransfer data on this env');

      const fromCursor = Math.max(0, sampleUserTransfer.id - 1);
      const received: BridgeEventSubscriptionResponse[] = [];

      await new Promise<void>((resolve, reject) => {
        let settled = false;
        let idleTimer: ReturnType<typeof setTimeout> | undefined;
        let unsubscribe = () => {};
        const settle = (handler: () => void) => {
          if (settled) return;
          settled = true;
          clearTimeout(idleTimer);
          handler();
        };
        const hardTimeout = setTimeout(() => {
          safeUnsubscribe(unsubscribe);
          if (received.length > 0) {
            settle(resolve);
          } else {
            settle(() => reject(new Error('no bridge events replayed within timeout')));
          }
        }, 30_000);
        const resetIdle = () => {
          clearTimeout(idleTimer);
          idleTimer = setTimeout(() => {
            clearTimeout(hardTimeout);
            safeUnsubscribe(unsubscribe);
            settle(resolve);
          }, 5_000);
        };

        const subscription = wsClient.subscribeToBridgeEvents(
          {
            next: (payload) => {
              received.push(payload);
              resetIdle();
            },
            error: (error) => {
              clearTimeout(hardTimeout);
              safeUnsubscribe(unsubscribe);
              settle(() => reject(new Error(`Subscription error: ${JSON.stringify(error)}`)));
            },
          },
          { from: fromCursor },
        );
        unsubscribe = subscription.unsubscribe;
      });

      const events = received.map((r) => r.data!.bridgeEvents);
      expect(events.length).toBeGreaterThan(0);
      const ids = events.map((e) => e.id!).filter((id) => id !== undefined);
      expect(ids).toContain(sampleUserTransfer.id);
      expect([...ids]).toEqual([...ids].sort((a, b) => a - b));
    }, 60_000);

    /**
     * @given a chain with events for multiple recipients
     * @when a bridgeEvents subscription is opened with recipient = <knownAddress>
     * @then only events with the matching recipient are delivered
     */
    test.todo('should only deliver events matching the recipient filter');

    /**
     * @given a chain with multiple event variants
     * @when a bridgeEvents subscription is opened with variant = USER_TRANSFER
     * @then only BridgeUserTransfer events are delivered
     */
    test.todo('should only deliver events matching the variant filter');

    /**
     * @given the id of the last event received in a previous subscription
     * @when a subscription reconnects with from = <lastId>
     * @then events with id > lastId arrive with no gap or duplication
     */
    test.todo('should resume from cursor without gap or duplication on reconnection');

    // Blocked: UnapprovedTransfer is unreachable until the approval governance
    // logic lands on the node (ApprovedTransactions storage + governance
    // extrinsic).
    // Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/940
    test.skip('should deliver BridgeUnapprovedTransfer events via subscription', () => {});
  });

  describe('claims via unshieldedTransactions', () => {
    // There is no bridgeClaims subscription. A bridge claim is a
    // BridgeClaimTransaction surfaced via the unshieldedTransactions query, so
    // claim coverage belongs with the unshielded-transaction tests.
    /**
     * @given a chain containing a CardanoBridge claim for a known recipient
     * @when unshieldedTransactions is observed for that recipient
     * @then the claim appears as a BridgeClaimTransaction
     */
    test.todo(
      'should observe claims as BridgeClaimTransaction via the unshieldedTransactions query',
    );
  });

  describe('bridgeBalance', () => {
    /**
     * @given a 32-byte all-zeros address with no bridge activity
     * @when a bridgeBalance subscription is opened for that address
     * @then the first emitted frame has deposited, claimed and balance all zero
     */
    test('should emit zero balance immediately for an address with no bridge activity', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Subscription', 'Bridge', 'Balance', 'Negative'] };
      if (!surfacePresent) return ctx.skip();

      const firstFrame = await new Promise<BridgeBalanceSubscriptionResponse>((resolve, reject) => {
        let unsubscribe = () => {};
        const timeout = setTimeout(() => {
          safeUnsubscribe(unsubscribe);
          reject(new Error('no bridgeBalance frame received within timeout'));
        }, 15_000);

        const subscription = wsClient.subscribeToBridgeBalance(
          {
            next: (payload) => {
              clearTimeout(timeout);
              safeUnsubscribe(unsubscribe);
              resolve(payload);
            },
            error: (error) => {
              clearTimeout(timeout);
              safeUnsubscribe(unsubscribe);
              reject(new Error(`Subscription error: ${JSON.stringify(error)}`));
            },
          },
          EMPTY_RECIPIENT,
        );
        unsubscribe = subscription.unsubscribe;
      });

      expect(firstFrame).toBeSuccess();
      expect(firstFrame.data?.bridgeBalance).toEqual({
        deposited: ZERO_U128,
        claimed: ZERO_U128,
        balance: ZERO_U128,
      });
    }, 30_000);

    /**
     * @given a subscription to bridgeBalance(address: <knownAddress>)
     * @when a UserTransfer for that address is indexed
     * @then a new BridgeBalance with deposited > 0 is pushed
     */
    test.todo('should push an updated BridgeBalance when a relevant UserTransfer is indexed');

    /**
     * @given a subscribed address with a prior UserTransfer balance
     * @when a CardanoBridge claim for that address is indexed
     * @then balance reflects the net remaining-claimable, reaching zero once fully claimed
     */
    test.todo('should push an updated BridgeBalance when a claim reduces the balance');
  });
});
