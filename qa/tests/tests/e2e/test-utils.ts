import { BlockResponse, TransactionResponse } from '@utils/indexer/indexer-types';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { IndexerWsClient, UnshieldedTxSubscriptionResponse } from '@utils/indexer/websocket-client';
import { ToolkitWrapper } from '@utils/toolkit/toolkit-wrapper';

export function retry<T>(
  fn: () => Promise<T>,
  condition: (result: T) => boolean,
  maxAttempts: number,
  delay: number,
): Promise<T> {
  return new Promise((resolve, reject) => {
    let attempts = 0;
    const execute = () => {
      attempts++;
      fn()
        .then((result) => {
          if (condition(result)) {
            resolve(result);
          } else if (attempts < maxAttempts) {
            setTimeout(execute, delay);
          } else {
            reject(new Error(`Condition not met after ${maxAttempts} attempts`));
          }
        })
        .catch((error) => {
          if (attempts < maxAttempts) {
            setTimeout(execute, delay);
          } else {
            reject(error);
          }
        });
    };
    execute();
  });
}

export function retrySimple<T>(
  fn: () => Promise<T | null>,
  maxAttempts = 10,
  delayMs = 3000,
): Promise<T> {
  return retry(fn, (result) => result !== null, maxAttempts, delayMs) as Promise<T>;
}

export function getBlockByHashWithRetry(hash: string): Promise<BlockResponse> {
  return retry(
    () => new IndexerHttpClient().getBlockByOffset({ hash: hash }),
    (response) => response.data?.block != null,
    30,
    2000,
  );
}

export function getTransactionByHashWithRetry(hash: string): Promise<TransactionResponse> {
  return retry(
    () => new IndexerHttpClient().getTransactionByOffset({ hash: hash }),
    (response) => response.data?.transactions != null && response.data.transactions.length > 0,
    30,
    2000,
  );
}

export function getContractDeploymentHashes(
  contractAddress: string,
): Promise<{ txHash: string; blockHash: string }> {
  return retry(
    async () => {
      const indexerClient = new IndexerHttpClient();
      const contractActionResponse = await indexerClient.getContractAction(contractAddress);

      if (contractActionResponse?.data?.contractAction?.__typename === 'ContractDeploy') {
        const contractAction = contractActionResponse.data.contractAction;
        const txHash = contractAction.transaction?.hash || '';
        const blockHash = contractAction.transaction?.block?.hash || '';

        if (!txHash || !blockHash) {
          throw new Error('Missing transaction hash or block hash in contract deployment');
        }

        return { txHash, blockHash };
      }

      throw new Error('Contract action is not a deployment or not found');
    },
    (result) => result.txHash !== '' && result.blockHash !== '',
    30,
    2000,
  );
}

/**
 * Wait until the provided events array stabilizes (its length stops changing between checks),
 * then drain and return its contents.
 *
 * Checks every `intervalMs` milliseconds and bails out after `maxWaitMs` to avoid infinite waits.
 */
export async function waitForEventsStabilization<T>(
  events: T[],
  intervalMs: number = 500,
  maxWaitMs: number = 60000,
): Promise<T[]> {
  let previousCount = -1;
  const start = Date.now();

  while (true) {
    await new Promise((resolve) => setTimeout(resolve, intervalMs));
    const currentCount = events.length;
    if (currentCount === previousCount) {
      return events.splice(0, events.length);
    }
    previousCount = currentCount;
    if (Date.now() - start > maxWaitMs) {
      return events.splice(0, events.length);
    }
  }
}

/**
 *  Prepares wallet event subscriptions for unshielded-transaction tests.
 *
 * Subscribes the source and a number of destination wallets to unshielded transaction events,
 *  waits for initial event streams to stabilise and returns all relevant context needed before performing any transactions.
 */
export async function setupWalletEventSubscriptions(
  toolkit: ToolkitWrapper,
  indexerWsClient: IndexerWsClient,
  sourceSeed: string,
  destinationSeeds: string[],
) {
  // Getting the addresses from their seeds
  const sourceAddress = (await toolkit.showAddress(sourceSeed)).unshielded;
  // Events from the indexer websocket for both the source addresses
  const sourceAddressEvents: UnshieldedTxSubscriptionResponse[] = [];

  // Subscribe the source wallet to unshielded transaction events
  const sourceAddrUnscribeFromEvents = indexerWsClient.subscribeToUnshieldedTransactionEvents(
    { next: (event) => sourceAddressEvents.push(event) },
    { address: sourceAddress },
  );

  // Historical events from the indexer websocket for both the source addresses
  let historicalSourceEvents: UnshieldedTxSubscriptionResponse[] = [];

  // Wait until source events count stabilizes, then snapshot to historical array
  historicalSourceEvents = await waitForEventsStabilization(sourceAddressEvents, 1000);

  // Derive and subscribe ALL destination wallets dynamically
  const destinationWallets = await Promise.all(
    destinationSeeds.map(async (seed) => {
      const destinationAddress = (await toolkit.showAddress(seed)).unshielded;

      const events: UnshieldedTxSubscriptionResponse[] = [];
      // We use the array to capture events before submitting the transaction
      let historicalDestinationEvents: UnshieldedTxSubscriptionResponse[] = [];

      // Subscribe the destination wallet to unshielded transaction events
      const unsubscribe = indexerWsClient.subscribeToUnshieldedTransactionEvents(
        { next: (event) => events.push(event) },
        { address: destinationAddress },
      );
      // Wait until destination events count stabilizes, then snapshot to historical array
      historicalDestinationEvents = await waitForEventsStabilization(events, 1000);

      return {
        seed,
        destinationAddress,
        events,
        unsubscribe,
        historicalDestinationEvents,
      };
    }),
  );
  return {
    source: {
      seed: sourceSeed,
      address: sourceAddress,
      events: sourceAddressEvents,
      unsubscribe: sourceAddrUnscribeFromEvents,
      historicalEvents: historicalSourceEvents,
    },
    destinations: destinationWallets,
  };
}

/**
 * Extracts all unshielded transaction events of a specific GraphQL `__typename`
 * from a wallet’s subscription event stream.
 */
export function getEventsOfType<T extends string>(
  events: UnshieldedTxSubscriptionResponse[],
  type: T,
) {
  return events
    .map((e) => e.data?.unshieldedTransactions)
    .filter((tx): tx is Extract<typeof tx, { __typename: T }> => tx?.__typename === type);
}
