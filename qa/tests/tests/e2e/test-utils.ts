import { BlockResponse, TransactionResponse } from '@utils/indexer/indexer-types';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import log from '@utils/logging/logger';
import { IndexerWsClient, UnshieldedTxSubscriptionResponse } from '@utils/indexer/websocket-client';
import dataProvider from '@utils/testdata-provider';
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
  maxAttempts = 5,
  delayMs = 1500,
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
 * Prepares a two-wallet (A > B) subscription setup for unshielded transaction tests.
 *
 * Subscribes both the source and destination wallets to unshielded transaction events, waits for their initial event streams to stabilize,
 * performs a single unshielded transfer of a configurable amount, and returns all relevant context for downstream assertions.
 */
export async function setupWalletSubscriptions(
  toolkit: ToolkitWrapper,
  indexerWsClient: IndexerWsClient,
  options?: { includeSecondDestination?: boolean },
) {
  const sourceSeed = dataProvider.getFundingSeed();
  const destinationSeed = '0000000000000000000000000000000000000000000000000000000987654321';
  const secondDestinationSeed = '0000000000000000000000000000000000000000000000000000000123456789';

  // Getting the addresses from their seeds
  const sourceAddress = (await toolkit.showAddress(sourceSeed)).unshielded;
  const destinationAddress = (await toolkit.showAddress(destinationSeed)).unshielded;

  // Events from the indexer websocket for both the source and destination addresses
  const sourceAddressEvents: UnshieldedTxSubscriptionResponse[] = [];
  const destinationAddressEvents: UnshieldedTxSubscriptionResponse[] = [];

  // Historical events from the indexer websocket for both the source and destination addresses
  // We use these two arrays to capture events before submitting the transaction
  let historicalSourceEvents: UnshieldedTxSubscriptionResponse[] = [];
  let historicalDestinationEvents: UnshieldedTxSubscriptionResponse[] = [];

  // Subscribe the source wallet to unshielded transaction events
  const sourceAddrUnscribeFromEvents = indexerWsClient.subscribeToUnshieldedTransactionEvents(
    { next: (event) => sourceAddressEvents.push(event) },
    { address: sourceAddress },
  );

  // Subscribe the destination wallet to unshielded transaction events
  const destAddrUnscribeFromEvents = indexerWsClient.subscribeToUnshieldedTransactionEvents(
    { next: (event) => destinationAddressEvents.push(event) },
    { address: destinationAddress },
  );
  // Wait until source events count stabilizes, then snapshot to historical array
  historicalSourceEvents = await waitForEventsStabilization(sourceAddressEvents, 1000);
  log.info(`Source events stabilized: ${historicalSourceEvents.length}`);

  // Wait until destination events count stabilizes, then snapshot to historical array
  historicalDestinationEvents = await waitForEventsStabilization(destinationAddressEvents, 1000);

  // Optional second destination

  let secondHistoricalDestinationEvents: UnshieldedTxSubscriptionResponse[] = [];
  const secondDestinationAddressEvents: UnshieldedTxSubscriptionResponse[] = [];

  let secondDestinationAddress: string | undefined;
  let secondDestAddrUnscribeFromEvents: (() => void) | undefined;

  if (options?.includeSecondDestination) {
    secondDestinationAddress = (await toolkit.showAddress(secondDestinationSeed)).unshielded;

    // Subscribe to second destination wallet events
    secondDestAddrUnscribeFromEvents = indexerWsClient.subscribeToUnshieldedTransactionEvents(
      { next: (event) => secondDestinationAddressEvents.push(event) },
      { address: secondDestinationAddress },
    );
    // Wait until second destination events stabilize
    secondHistoricalDestinationEvents = await waitForEventsStabilization(
      secondDestinationAddressEvents,
      1000,
    );
    log.info(`Second destination events stabilized: ${secondHistoricalDestinationEvents.length}`);
  }

  return {
    sourceSeed,
    destinationSeed,
    secondDestinationSeed,
    sourceAddress,
    destinationAddress,
    secondDestinationAddress,
    sourceAddressEvents,
    destinationAddressEvents,
    secondDestinationAddressEvents,
    sourceAddrUnscribeFromEvents,
    destAddrUnscribeFromEvents,
    secondDestAddrUnscribeFromEvents,
    historicalSourceEvents,
    historicalDestinationEvents,
    secondHistoricalDestinationEvents,
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
