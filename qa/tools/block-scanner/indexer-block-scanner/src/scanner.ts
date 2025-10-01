import os from 'os';
import fs from 'fs';
import path from 'path';
// import { Worker } from 'worker_threads';
import { TARGET_ENV, INDEXER_WS_URL } from './env.js';
import { Block, RegularTransaction, Transaction } from './indexer-types.js';

// Read the GraphQL subscription query from file
const BLOCK_SUBSCRIPTION_QUERY_WITH_OFFSET = fs
  .readFileSync(
    path.join(path.dirname(new URL(import.meta.url).pathname), 'block-subscription.graphql'),
    'utf-8',
  )
  .trim();

let MAX_HEIGHT = 0;

const TMP_DIR = 'tmp_scan';
const NUM_WORKERS = os.cpus().length;
const BLOCKS_PER_WORKER = 10000;

// Parse command line arguments
const args = process.argv.slice(2);
const testDataFolder = args[0]; // First argument is the test data folder path

const handlersMap: Map<string, SubscriptionHandlers<any>> = new Map<
  string,
  SubscriptionHandlers<any>
>();

function parseTimestampToMs(value: unknown): number | undefined {
  if (value === null || value === undefined) return undefined;
  // If it's a number or numeric string, interpret as epoch (sec or ms)
  if (typeof value === 'number' || (typeof value === 'string' && /^\d+(\.\d+)?$/.test(value))) {
    const asNumber = Number(value);
    if (!Number.isFinite(asNumber)) return undefined;
    // Heuristic: < 1e12 => seconds; else milliseconds
    const ms = asNumber < 1e12 ? asNumber * 1000 : asNumber;
    return Number.isFinite(ms) ? ms : undefined;
  }
  // Otherwise try Date.parse on strings like ISO
  if (typeof value === 'string') {
    const parsed = Date.parse(value);
    return Number.isFinite(parsed) ? parsed : undefined;
  }
  return undefined;
}

/**
 * Maps the Graphql response payload
 */
export interface SubscriptionPayload<T> {
  data?: T;
  errors?: any[];
}

interface BlockOffset {
  hash?: string;
  height?: number;
}

/**
 * Handlers used to respond to incoming GraphQL subscription messages.
 */
export interface SubscriptionHandlers<T> {
  /** Called when a new payload is received */
  next: (value: T) => void;

  /** Called when an error is received (note this is actually obsolete) */
  error?: (err: any) => void;

  /** Called when the subscription completes */
  complete?: () => void;
}

if (!fs.existsSync(TMP_DIR)) fs.mkdirSync(TMP_DIR);

/**
 * Updates test data files in the specified folder
 * @param folderPath - Path to the test data folder
 */
function updateTestDataFiles(folderPath: string, dataFile: string): void {
  console.log(`updateTestDataFiles called with folder: ${folderPath}`);

  // List the files in the folder
  const targetFolder = `${folderPath}/${TARGET_ENV}`;

  updateContracDataFile(folderPath, dataFile);
}

function updateContracDataFile(folderPath: string, dataFile: string): void {
  // Read the data file containing all the relevant blocks
  const data = fs.readFileSync(dataFile, 'utf8');

  // Parse the data making sure the line is not empty and only filter the blocks that contain contract actions
  const dataArray = data
    .split('\n')
    .map((line) => {
      if (line.trim() !== '') {
        return JSON.parse(line);
      }
    })
    .filter((block) => {
      if (block !== undefined) {
        // Not all the transactions are RegularTransaction, so we need to filter them out
        // SystemTeransactions don't have contractActions
        return block.transactions.some((transaction: Transaction | any) => {
          if (transaction.__typename === 'RegularTransaction') {
            return transaction.contractActions.length > 0;
          }
          return false;
        });
      }
      return false;
    });

  // The contract actions data structure will hold the address and the contract actions
  // with the height of the block where the contract action was executed
  // something like this:
  // {
  //   "address": {
  //     "ContractDeploy": [1, 2, 3],
  //     "ContractCall": [4, 5, 6],
  //     "ContractUpdate": [7, 8, 9]
  //   }
  // }
  const contractActionsMap: { [key: string]: { [key: string]: number[] } } = {};

  // Iterate over the dataArray and count the contract actions per address
  for (const block of dataArray) {
    for (const transaction of block.transactions) {
      if (transaction.__typename === 'RegularTransaction') {
        for (const contractAction of transaction.contractActions) {
          const address: string = contractAction.address;
          const contractActionType: string = contractAction.__typename;
          if (!contractActionsMap[address]) {
            contractActionsMap[address] = {
              ContractDeploy: [],
              ContractCall: [],
              ContractUpdate: [],
            };
            console.log(`address: ${address}, contractActionType: ${contractActionType.trim()}`);
          }
          const current = contractActionsMap[address];
          current[contractActionType.trim() as keyof typeof current].push(block.height);
        }
      }
    }
  }

  // Print the contract actions map
  console.log(`contractActionsMap: ${JSON.stringify(contractActionsMap, null, 2)}`);

  // Write the data to the target folder
  fs.writeFileSync(
    path.join(folderPath, `${TARGET_ENV}`, `contracts-actions.json`),
    JSON.stringify(contractActionsMap, null, 2),
  );
}

async function connectionInit(ws: WebSocket) {
  const timeoutMs = 2000;
  console.debug(`Ready state = ${ws.readyState}`);

  const maxTime = Date.now() + timeoutMs;
  while (ws.readyState !== WebSocket.OPEN) {
    if (Date.now() > maxTime) {
      throw new Error('WebSocket connection timeout');
    }
    await new Promise((res) => setTimeout(res, 50));
    console.debug(`Ready state = ${ws.readyState}`);
  }

  const response: Promise<{ type: string }> = new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      ws.removeEventListener('message', onMessage);
      reject(new Error('Timed out waiting for connection_ack'));
    }, timeoutMs);

    const onMessage = (event: MessageEvent) => {
      const message = JSON.parse(event.data);
      if (message.type === 'connection_ack') {
        clearTimeout(timeout);
        ws.removeEventListener('message', onMessage);
        resolve(message);
      }
    };

    ws.addEventListener('message', onMessage);
    ws.send(
      JSON.stringify({
        type: 'connection_init', // Payload is optional and can be used for negotiation
      }),
    );
  });

  if ((await response).type !== 'connection_ack') {
    throw new Error("connection_ack message wasn't received");
  }
}

function generateCustomId(): string {
  const chars = 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789';
  const segment = () =>
    Array.from({ length: 7 }, () => chars.charAt(Math.floor(Math.random() * chars.length))).join(
      '',
    );
  return `${segment()}-${segment()}-${segment()}`;
}

function handleMessage(event: MessageEvent) {
  const message = JSON.parse(event.data);
  const { id, payload, type } = message;

  if (type === 'connection_ack') {
    return;
  }

  const handlers = handlersMap.get(id);
  if (!handlers) return;

  switch (type) {
    case 'next':
      handlers.next?.(payload);
      break;
    case 'error':
      handlers.error?.(payload);
      break;
    case 'complete':
      handlers.complete?.();
      handlersMap.delete(id);
      break;
  }
}

function subscribeToBlockEvents(
  ws: WebSocket,
  blockOffset: BlockOffset | undefined,
  handlers: SubscriptionHandlers<SubscriptionPayload<{ blocks: Block }>>,
  queryOverride?: string,
  variablesOverride?: Record<string, unknown>,
): () => void {
  const id = '1';

  let query = BLOCK_SUBSCRIPTION_QUERY_WITH_OFFSET;

  let variables: Record<string, unknown> | undefined = blockOffset
    ? {
        OFFSET: blockOffset,
      }
    : undefined;

  if (queryOverride) query = queryOverride;

  console.debug(query);
  console.debug(JSON.stringify(variables));

  const payload = {
    id,
    type: 'start',
    payload: {
      query,
      variables,
    },
  };

  console.debug(JSON.stringify(payload));

  handlersMap.set(id, handlers);
  ws.send(JSON.stringify(payload));

  return () => {
    const stopMessage = { id, type: 'stop' };
    ws.send(JSON.stringify(stopMessage));
    handlersMap.delete(id);
  };
}

async function main(): Promise<void> {
  //Create a temporary file to store the blocks
  const blocksFile = fs.createWriteStream(path.join(TMP_DIR, `${TARGET_ENV}_blocks.jsonl`));

  console.log(`Connecting to ${TARGET_ENV} with url ${INDEXER_WS_URL[TARGET_ENV]}`);
  const indexerWs = new WebSocket(INDEXER_WS_URL[TARGET_ENV], 'graphql-transport-ws');
  indexerWs.onmessage = handleMessage.bind(indexerWs);
  await connectionInit(indexerWs);

  // One-shot query to get the latest block height at start time
  async function getLatestBlockHeight(ws: WebSocket, timeoutMs = 10_000): Promise<number> {
    const id = generateCustomId();
    const query = `query GetLatestBlock { block { height } }`;

    return await new Promise<number>((resolve, reject) => {
      let resolved = false;

      const timeout = setTimeout(() => {
        handlersMap.delete(id);
        if (!resolved) reject(new Error('Timed out fetching latest block height'));
      }, timeoutMs);

      handlersMap.set(id, {
        next: (payload: { data?: { block?: { height?: number } } }) => {
          try {
            const height = payload?.data?.block?.height ?? 0;
            const stopMessage = { id, type: 'stop' };
            ws.send(JSON.stringify(stopMessage));
            clearTimeout(timeout);
            handlersMap.delete(id);
            if (!resolved) {
              resolved = true;
              resolve(Number(height) || 0);
            }
          } catch (e) {
            clearTimeout(timeout);
            handlersMap.delete(id);
            if (!resolved) reject(e);
          }
        },
        complete: () => {
          // no-op; we resolve on next
        },
        error: (err) => {
          clearTimeout(timeout);
          handlersMap.delete(id);
          if (!resolved) reject(err);
        },
      });

      const payload = {
        id,
        type: 'start',
        payload: { query },
      };
      ws.send(JSON.stringify(payload));
    });
  }

  const receivedBlocks: Block[] = [];

  const blockSubscriptionHandler: SubscriptionHandlers<SubscriptionPayload<{ blocks: Block }>> = {
    next: (payload) => {
      if (payload.errors) {
        console.debug('Errors found');
        console.debug(JSON.stringify(payload.errors, null, 2));
        throw new Error('Indexer subscription failed');
      }

      if (payload.data !== undefined) {
        receivedBlocks.push(payload.data.blocks);
        if (payload.data?.blocks.transactions.length > 0) {
          console.debug('Transaction found');

          // Write the block to file as a json line
          blocksFile.write(JSON.stringify(payload.data.blocks) + '\n');
        }
        if (
          payload.data?.blocks.transactions.some(
            (transaction) =>
              transaction.__typename === 'RegularTransaction' &&
              (transaction as RegularTransaction).contractActions!.length > 0,
          )
        ) {
          console.debug(`Contract action found in block ${payload.data?.blocks.height}`);
        }
      }

      if (receivedBlocks.length % 1000 === 0) {
        console.debug(`Received ${receivedBlocks.length}`);
      }
    },
    complete: () => {
      console.debug('Completed sent from Indexer');
    },
  };

  // Determine target height, then subscribe and wait until we reach it or timeout
  const targetHeight = await getLatestBlockHeight(indexerWs).catch(() => 0);
  console.debug(`Target height: ${targetHeight}`);
  const TIMEOUT_MS = 600_000;

  let reachedTarget = false;
  const reachedPromise = new Promise<void>((resolve) => {
    const originalNext = blockSubscriptionHandler.next;
    blockSubscriptionHandler.next = (payload) => {
      originalNext(payload);
      const currentHeightVal = Number(payload?.data?.blocks?.height ?? NaN);
      if (!reachedTarget && Number.isFinite(currentHeightVal) && currentHeightVal >= targetHeight) {
        reachedTarget = true;
        resolve();
      }
    };
  });

  const unscribe = subscribeToBlockEvents(indexerWs, { height: 0 }, blockSubscriptionHandler);

  console.log('Subscribed to block updates!');
  console.log(`Streaming until height >= ${targetHeight} (or ${TIMEOUT_MS / 1000}s timeout) ...`);

  await Promise.race([reachedPromise, new Promise((res) => setTimeout(res, TIMEOUT_MS))]);

  console.log('Unscribing from block updates');
  unscribe();

  console.log('Closing websocket connection');

  // Clean up all handlers before closing
  handlersMap.clear();

  indexerWs.close();
  await new Promise<void>((resolve) => {
    indexerWs.addEventListener('close', () => resolve());
  });

  console.log(`Received ${receivedBlocks.length} blocks`);

  console.log('Block fetching completed!');

  // Update test data files if folder path was provided
  if (testDataFolder) {
    console.log(`Updating test data files in: ${testDataFolder}`);
    updateTestDataFiles(testDataFolder, `${TMP_DIR}/${TARGET_ENV}_blocks.jsonl`);
  }

  return;
}

await main()
  .then(() => {
    console.log('Process completed successfully');
    process.exit(0);
  })
  .catch((error) => {
    console.error('Process failed:', error);
    process.exit(1);
  });
