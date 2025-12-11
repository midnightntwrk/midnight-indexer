import '@utils/logging/test-logging-hooks';
import { IndexerWsClient } from '@utils/indexer/websocket-client';
import { EventCoordinator } from '@utils/event-coordinator';
import { ZswapLedgerEventSchema } from '@utils/indexer/graphql/schema';
import {
  collectValidZswapEvents,
  collectZswapEventError,
} from '../../../shared/zswap-events-utils';

describe('zswap ledger event subscriptions', () => {
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

  describe('a subscription to zswap ledger events without offset (default replay)', () => {
    /**
     * Subscribing to ZswapLedger events without providing an offset should replay
     * historical events in the correct ledger order.
     *
     * @given no zswap event offset parameters are provided
     * @when we subscribe to zswap ledger events
     * @then events must be applied sequentially in order
     * @and the subscription must maintain strict event ordering via monotonic IDs
     */
    test('streams events in strictly increasing order', async () => {
      const received = await collectValidZswapEvents(indexerWsClient, eventCoordinator, 3);

      expect(received.length === 3, `Expected 3 events, got: ${received.length}`).toBe(true);

      const ids = received.map((e) => e.data!.zswapLedgerEvents.id);
      const isStrict = ids.every((id, i) => i === 0 || id > ids[i - 1]);

      expect(isStrict, `Zswap event IDs must be strictly increasing, got: ${ids.join(', ')}`).toBe(
        true,
      );
    });
  });

  describe('subscription with explicit offset', () => {
    /**
     * Subscribing to ZswapLedger events with an explicit offset should replay
     * historical events beginning from the provided event ID.
     *
     * @given a zswap event offset parameter is provided
     * @when we subscribe to zswap ledger events with that offset
     * @then events must be applied sequentially in order
     * @and the subscription must maintain strict event ordering via monotonic IDs
     */
    test('streams events starting from the specified ID', async () => {
      const firstEvent = await collectValidZswapEvents(indexerWsClient, eventCoordinator, 1);
      const latestId = firstEvent[0].data!.zswapLedgerEvents.maxId;
      const startId = Math.max(latestId - 5, 0);
      const received = await collectValidZswapEvents(indexerWsClient, eventCoordinator, 3, startId);
      expect(received.length).toBe(3);

      const ids = received.map((e) => e.data!.zswapLedgerEvents.id);

      expect(
        ids[0] >= startId,
        `Expected first event ID >= startId (${startId}), got: ${ids[0]}`,
      ).toBe(true);
      const isStrictlyIncreasing = ids.every((id, i) => i === 0 || id > ids[i - 1]);

      expect(
        isStrictlyIncreasing,
        `Zswap event IDs must be strictly increasing, got: ${ids.join(', ')}`,
      ).toBe(true);
    });

    /**
     * Validates that all replayed zswap ledger events conform to the expected schema.
     *
     * @given a zswap ledger subscription with an explicit offset ID
     * @when historical zswap events are streamed starting from that offset
     * @then each received event must match the ZswapLedgerEventsUnionSchema definition
     */
    test('validates historical zswap events against schema', async () => {
      const firstEvent = await collectValidZswapEvents(indexerWsClient, eventCoordinator, 1);
      const latestId = firstEvent[0].data!.zswapLedgerEvents.maxId;

      const fromId = Math.max(latestId - 3, 0);
      const received = await collectValidZswapEvents(indexerWsClient, eventCoordinator, 3, fromId);
      received
        .filter((msg) => msg.data?.zswapLedgerEvents)
        .forEach((msg) => {
          expect.soft(msg).toBeSuccess();

          const event = msg.data!.zswapLedgerEvents;
          const parsed = ZswapLedgerEventSchema.safeParse(event);
          expect(
            parsed.success,
            `Zswap ledger event schema validation failed:\n${JSON.stringify(parsed.error?.format(), null, 2)}`,
          ).toBe(true);
        });
    });
  });

  describe('subscription error handling', () => {
    /**
     * Subscribing with a query that references a nonexistent field should return
     * a GraphQL validation error instead of streaming zswap ledger events.
     *
     * @given a zswap ledger subscription whose selection set contains an unknown field
     * @when the subscription request is sent to the indexer GraphQL endpoint
     * @then the server must return a validation error indicating the field does not exist
     * @and no zswap ledger events should be streamed
     */
    test('should return an error for unknown field', async () => {
      const errorMessage = await collectZswapEventError(indexerWsClient, null, true);
      expect(errorMessage).toBe(`Unknown field "unknownField" on type "ZswapLedgerEvent".`);
    });

    /**
     * Providing a negative offset should result in an error response instead of
     *
     * @given a zswap ledger subscription with an explicit offset parameter
     * @when the offset value is negative
     * @then an error should be returned
     */
    test('rejects negative offset ID with an error', async () => {
      const errorMessage = await collectZswapEventError(indexerWsClient, { id: -50 });
      expect(errorMessage).toBe(`Failed to parse "Int": Invalid number`);
    });
  });
});
