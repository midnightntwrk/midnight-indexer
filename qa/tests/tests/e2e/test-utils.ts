import { BlockResponse } from '@utils/indexer/indexer-types';
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

/**
 * Simple retry mechanism: try every 500ms for up to 6 seconds
 * Retry getting a block by hash until the block is found or the maximum number of attempts is reached.
 * @param hash - The hash of the block to get.
 * @returns The block response.
 */
export function getBlockByHashWithRetry(hash: string): Promise<BlockResponse> {
  return retry(
    () => new IndexerHttpClient().getBlockByOffset({ hash }),
    (response) => response.data?.block != null,
    12,
    500,
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
