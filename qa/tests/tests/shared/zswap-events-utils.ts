import {
  ZswapLedgerEventSubscriptionResponse,
  IndexerWsClient,
  SubscriptionHandlers,
} from '@utils/indexer/websocket-client';
import { EventCoordinator } from '@utils/event-coordinator';
import log from '@utils/logging/logger';

export async function collectValidZswapEvents(
  indexerWsClient: IndexerWsClient,
  eventCoordinator: EventCoordinator,
  expectedCount: number,
  fromId?: number,
): Promise<ZswapLedgerEventSubscriptionResponse[]> {
  const received: ZswapLedgerEventSubscriptionResponse[] = [];
  const eventName = `${expectedCount} ZswapLedger Events`;

  const handler = {
    next: (payload: ZswapLedgerEventSubscriptionResponse) => {
      if (received.length >= expectedCount) return;

      received.push(payload);
      log.debug(
        `Received Zswap event ${received.length}/${expectedCount}:\n${JSON.stringify(payload, null, 2)}`,
      );
      if (received.length == expectedCount) {
        eventCoordinator.notify(eventName);
      }
    },
  };

  const offset = fromId ? { id: fromId } : undefined;
  const maxTimeBetweenIds = fromId ? 4_000 : 10_000;
  const subscription = indexerWsClient.subscribeToZswapLedgerEvents(handler, offset);

  await eventCoordinator.waitForAll([eventName], maxTimeBetweenIds);
  subscription.unsubscribe();
  return received;
}

export async function collectZswapEventError(
  indexerWsClient: IndexerWsClient,
  variables: Record<string, unknown> | null,
  unknownField: boolean = false,
): Promise<string> {
  return new Promise((resolve) => {
    const validQuery = `
      subscription ZswapEvents($id: Int) {
        zswapLedgerEvents(id: $id) {
          id
        }
      }
    `;

    const invalidFieldQuery = `
      subscription ZswapEvents {
        zswapLedgerEvents {
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

    const subscription = indexerWsClient.subscribeToZswapLedgerEvents(
      handler as SubscriptionHandlers<ZswapLedgerEventSubscriptionResponse>,
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
