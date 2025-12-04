// This file is part of midnightntwrk/midnight-indexer
// Copyright (C) 2025 Midnight Foundation
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
import {
  IndexerWsClient,
  SubscriptionHandlers,
  DustLedgerEventSubscriptionResponse,
} from '@utils/indexer/websocket-client';
import { EventCoordinator } from '@utils/event-coordinator';
import { DustLedgerEventsUnionSchema } from '@utils/indexer/graphql/schema';

describe('dust ledger event subscriptions', () => {
  let indexerWsClient: IndexerWsClient;
  let eventCoordinator: EventCoordinator;

  beforeEach(async () => {
    indexerWsClient = new IndexerWsClient();
    eventCoordinator = new EventCoordinator();
    await indexerWsClient.connectionInit();
  });

  afterEach(async () => {
    await indexerWsClient.connectionClose();
    eventCoordinator.clear();
  });

  /**
   * Helper to subscribe to dust ledger events and collect a specific number of valid responses.
   * Supports optional ID-based historical replay via `fromId`.
   */
  async function collectValidDustEvents(
    expectedCount: number,
    fromId?: number,
  ): Promise<DustLedgerEventSubscriptionResponse[]> {
    const received: DustLedgerEventSubscriptionResponse[] = [];
    const eventName = `${expectedCount} DustLedger Events`;

    let unsubscribe: (() => void) | null = null;
    let finished = false;

    const handler: SubscriptionHandlers<DustLedgerEventSubscriptionResponse> = {
      next: (payload) => {
        if (finished) return;
        received.push(payload);

        if (received.length >= expectedCount) {
          finished = true;
          unsubscribe?.();
          eventCoordinator.notify(eventName);
        }
      },
    };

    const offset = fromId !== undefined ? { id: fromId } : undefined;
    unsubscribe = indexerWsClient.subscribeToDustLedgerEvents(handler, offset);
    await eventCoordinator.waitForAll([eventName], 10000);
    return received;
  }

  /**
   * Helper to subscribe to dust ledger events and capture GraphQL error responses.
   * Used for testing invalid variables (e.g. negative offsets) or invalid fields.
   */
  async function collectDustEventError(
    variables: Record<string, unknown> | null,
    unknownField: boolean = false,
  ): Promise<string> {
    return new Promise((resolve) => {
      const validQuery = `
      subscription DustLedgerEvents($id: Int) {
        dustLedgerEvents(id: $id) {
          id
        }
      }
    `;

      const invalidFieldQuery = `
      subscription DustLedgerEvents {
        dustLedgerEvents {
          unknownField
        }
      }
    `;

      const query = unknownField ? invalidFieldQuery : validQuery;
      let unsubscribe: (() => void) | null = null;
      const handler: SubscriptionHandlers<unknown> = {
        next: (payload) => {
          if (typeof payload === 'object' && payload !== null && 'errors' in payload) {
            const p = payload as { errors: { message: string }[] };
            resolve(p.errors[0].message);
            unsubscribe?.();
          }
        },
        error: (err) => {
          resolve(String(err));
          unsubscribe?.();
        },
      };

      unsubscribe = indexerWsClient.subscribeToDustLedgerEvents(handler, undefined, query);

      if (variables) {
        indexerWsClient.send({
          id: '0',
          type: 'start',
          payload: { query, variables },
        });
      }
    });
  }

  describe('a subscription to dust ledger events without offset (default replay)', () => {
    /**
     * Subscribing to DustLedger events without providing an offset should replay
     * historical events in the correct ledger order.
     *
     * @given no dust event offset parameters are provided
     * @when we subscribe to dust ledger events
     * @then events must be applied sequentially in order
     * @and the subscription must maintain strict event ordering via monotonic IDs
     */
    test('streams events in strictly increasing order', async () => {
      const received = await collectValidDustEvents(3);

      expect(received.length === 3, `Expected 3 events, got: ${received.length}`).toBe(true);

      const ids = received.map((e) => e.data!.dustLedgerEvents.id);
      const isStrict = ids.every((id, i) => i === 0 || id > ids[i - 1]);

      expect(isStrict, `Dust event IDs must be strictly increasing, got: ${ids.join(', ')}`).toBe(
        true,
      );
    });
  });

  describe('subscription with explicit offset', () => {
    /**
     * Subscribing to DustLedger events with an explicit offset should replay
     * historical events beginning from the provided event ID.
     *
     * @given a dust event offset parameter is provided
     * @when we subscribe to dust ledger events with that offset
     * @then events must be applied sequentially in order
     * @and the subscription must maintain strict event ordering via monotonic IDs
     */
    test('streams events starting from the specified ID', async () => {
      const firstEvent = await collectValidDustEvents(1);
      const latestId = firstEvent[0].data!.dustLedgerEvents.maxId;

      const startId = Math.max(latestId - 5, 0);
      const received = await collectValidDustEvents(3, startId);
      expect(received.length).toBe(3);

      const ids = received.map((e) => e.data!.dustLedgerEvents.id);

      expect(
        ids[0] >= startId,
        `Expected first event ID >= startId (${startId}), got: ${ids[0]}`,
      ).toBe(true);
      const isStrictlyIncreasing = ids.every((id, i) => i === 0 || id > ids[i - 1]);

      expect(
        isStrictlyIncreasing,
        `Dust event IDs must be strictly increasing, got: ${ids.join(', ')}`,
      ).toBe(true);
    });

    /**
     * Validates that all replayed dust ledger events conform to the expected schema.
     *
     * @given a dust ledger subscription with an explicit offset ID
     * @when historical dust events are streamed starting from that offset
     * @then each received event must match the DustLedgerEventsUnionSchema definition
     */
    test('validates historical dust events against schema', async () => {
      const firstEvent = await collectValidDustEvents(1);
      const latestId = firstEvent[0].data!.dustLedgerEvents.maxId;

      const fromId = Math.max(latestId - 5, 0);
      const received = await collectValidDustEvents(5, fromId);
      received
        .filter((msg) => msg.data?.dustLedgerEvents)
        .forEach((msg) => {
          expect.soft(msg).toBeSuccess();

          const event = msg.data!.dustLedgerEvents;
          const parsed = DustLedgerEventsUnionSchema.safeParse(event);
          expect(
            parsed.success,
            `Dust ledger event schema validation failed:\n${JSON.stringify(parsed.error?.format(), null, 2)}`,
          ).toBe(true);
        });
    });
  });

  describe('subscription error handlin', () => {
    /**
     * Subscribing with a query that references a nonexistent field should return
     * a GraphQL validation error instead of streaming dust ledger events.
     *
     * @given a dust ledger subscription whose selection set contains an unknown field
     * @when the subscription request is sent to the indexer GraphQL endpoint
     * @then the server must return a validation error indicating the field does not exist
     * @and no dust ledger events should be streamed
     */
    test('should return an error for unknown field', async () => {
      const errorMessage = await collectDustEventError(null, true);

      expect(errorMessage).toBe(`Unknown field "unknownField" on type "DustLedgerEvent".`);
    });

    /**
     * Providing a negative offset should result in an error response instead of
     *
     * @given a dust ledger subscription with an explicit offset parameter
     * @when the offset value is negative
     * @then an error should be returned
     */
    test('rejects negative offset ID with an error', async () => {
      const errorMessage = await collectDustEventError({ id: -50 });

      expect(errorMessage).toBe(`Failed to parse "Int": Invalid number`);
    });
  });
});
