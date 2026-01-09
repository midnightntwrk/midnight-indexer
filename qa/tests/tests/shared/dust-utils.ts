import {
  SubscriptionHandlers,
  DustLedgerEventSubscriptionResponse,
  IndexerWsClient,
} from '@utils/indexer/websocket-client';
import { EventCoordinator } from '@utils/event-coordinator';

/**
 * Helper to subscribe to dust ledger events and collect a specific number of valid responses.
 * Supports optional ID-based historical replay via `fromId`.
 */
export async function collectValidDustEvents(
  indexerWsClient: IndexerWsClient,
  eventCoordinator: EventCoordinator,
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
export async function collectDustEventError(
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