import {
  SubscriptionHandlers,
  DustLedgerEventSubscriptionResponse,
  IndexerWsClient,
  GraphQLCompleteMessage,
} from '@utils/indexer/websocket-client';
import { EventCoordinator } from '@utils/event-coordinator';
import log from '@utils/logging/logger';

/**
 * Helper to subscribe to dust ledger events and collect a specific number of valid responses.
 * Supports optional ID-based historical replay via `fromId`.
 */
export async function collectValidDustLedgerEvents(
  indexerWsClient: IndexerWsClient,
  eventCoordinator: EventCoordinator,
  expectedCount: number,
  fromId?: number,
): Promise<DustLedgerEventSubscriptionResponse[]> {
  const received: DustLedgerEventSubscriptionResponse[] = [];
  const eventName = `${expectedCount} DustLedger Events`;

  const handler = {
    next: (payload: DustLedgerEventSubscriptionResponse) => {
      if (received.length >= expectedCount) return;

      received.push(payload);
      log.debug(
        `Received event ${received.length}/${expectedCount}:\n${JSON.stringify(payload, null, 2)}`,
      );
      if (received.length == expectedCount) {
        eventCoordinator.notify(eventName);
        log.debug(`${expectedCount} Dust Ledger events received`);
        indexerWsClient.send<GraphQLCompleteMessage>({
          id: subscription.id,
          type: 'complete',
        });
      }
    },
  };

  const offset = fromId ? { id: fromId } : undefined;
  const maxTimeBetweenIds = fromId ? 2_000 : 8_000;
  const subscription = indexerWsClient.subscribeToDustLedgerEvents(handler, offset);

  await eventCoordinator.waitForAll([eventName], maxTimeBetweenIds);
  subscription.unsubscribe();
  return received;
}

/**
 * Helper to subscribe to dust ledger events and capture GraphQL error responses.
 * Used for testing invalid variables (e.g. negative offsets) or invalid fields.
 */
export async function collectDustLedgerEventError(
  indexerWsClient: IndexerWsClient,
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

    let resolved = false;

    const handler: SubscriptionHandlers<unknown> = {
      next: (payload) => {
        if (resolved) return;
        if (typeof payload === 'object' && payload !== null && 'errors' in payload) {
          const p = payload as { errors: { message: string }[] };
          resolved = true;
          subscription.unsubscribe();
          clearTimeout(timeout);
          resolve(p.errors[0].message);
        }
      },
      error: (err) => {
        if (resolved) return;
        resolved = true;
        subscription.unsubscribe();
        clearTimeout(timeout);
        resolve(String(err));
      },
    };

    let offset: { id: number } | undefined;
    if (variables?.id) {
      offset = { id: variables.id as number };
    }

    const subscription = indexerWsClient.subscribeToDustLedgerEvents(
      handler as SubscriptionHandlers<DustLedgerEventSubscriptionResponse>,
      offset,
      query,
    );

    const timeout = setTimeout(() => {
      if (resolved) return;
      resolved = true;
      subscription.unsubscribe();
      resolve('Timeout: No error received');
    }, 3000);
  });
}
