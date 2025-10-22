import { BlockResponse, TransactionResponse } from '@utils/indexer/indexer-types';
import { IndexerHttpClient } from '@utils/indexer/http-client';

function retry<T>(
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
  // eslint-disable-next-line no-constant-condition
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
