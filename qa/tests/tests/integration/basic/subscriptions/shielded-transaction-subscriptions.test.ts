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

import { TestContext } from 'vitest';
import { randomBytes } from 'crypto';
import log from '@utils/logging/logger';
import { LedgerNetworkId, env } from 'environment/model';
import '@utils/logging/test-logging-hooks';
import {
  IndexerWsClient,
  SubscriptionHandlers,
  GraphQLCloseSessionMessage,
  ShieldedTxSubscriptionResponse,
} from '@utils/indexer/websocket-client';
import { generateSyntheticViewingKey } from '@utils/bech32-codec';
import { ToolkitWrapper } from '@utils/toolkit/toolkit-wrapper';

describe('shielded transaction subscriptions', () => {
  let randomSeed: string;
  let toolkit: ToolkitWrapper;
  let indexerWsClient: IndexerWsClient;

  beforeAll(async () => {
    // Initialise the toolkit wrapper
    log.debug('Creating the wrapper');
    toolkit = new ToolkitWrapper({});
    log.debug('Starting the wrapper');
    await toolkit.start();
    log.debug('Wrapper started');
  });

  afterAll(async () => {
    await toolkit.stop();
  });

  beforeEach(async () => {
    // Initialise a random seed used for the viewing key operations
    randomSeed = randomBytes(32).toString('hex');

    // Initialise the indexer websocket client and connect to it
    indexerWsClient = new IndexerWsClient();
    await indexerWsClient.connectionInit();
  });

  afterEach(async () => {
    // Close the indexer websocket client
    await indexerWsClient.connectionClose();
  });

  describe('opening a session with viewing key', async () => {
    /**
     * Opening a session with a valid viewing key returns a session ID
     *
     * Note: The only requirement is the viewing key is valid and matches the
     * target network is meant for. In essence, it might be a viewing key for
     * a wallet that doesn't exist, but that is ok because that is enough to open
     * a session. Then if the wallet doesn't exist (i.e. no relevant transactions),
     * the subscription will not stream any transaction data, that's all!
     *
     * @given a valid viewing key
     * @when we open a session with that viewing key
     * @then Indexer should return a session ID
     */
    test('should return a session ID, given a valid viewing key', async () => {
      log.info(`randomSeed = ${randomSeed}`);
      const viewingKey = await toolkit.showViewingKey(randomSeed);
      log.debug(`viewingKey = ${viewingKey}`);

      return indexerWsClient
        .openWalletSession(viewingKey)
        .then((sessionId) => {
          log.debug(`Received session id = ${sessionId}`);
          expect(sessionId).toMatch(/^[a-f0-9]+$/);
        })
        .catch((error) => {
          log.error(error);
          throw new Error(error);
        });
    });

    /**
     * Opening a session with unsupported hex format viewing key returns an error
     *
     * @given an unsupported hex format viewing key
     * @when we open a session with that viewing key
     * @then Indexer should return an error
     */
    test('should return an error, given an unsupported hex format viewing key', async () => {
      // Hex viewing key are no longer supported and should be rejected by indexer
      const hexViewingKey = 'AB34567890FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF';
      log.debug(`hexViewingKey = ${hexViewingKey}`);

      // Expect the promise to reject with an error
      await expect(indexerWsClient.openWalletSession(hexViewingKey)).rejects.toThrow();
    });

    /**
     * Opening a session with an invalid viewing key returns an error
     *
     * @given an invalid viewing key
     * @when we open a session with that viewing key
     * @then Indexer should return an error
     */
    test('should return an error, given an invalid viewing key', async () => {
      const generatedViewingKey = generateSyntheticViewingKey('dev1');
      log.debug(`generatedViewingKey = ${generatedViewingKey}`);

      // Expect the promise to reject with an error
      await expect(indexerWsClient.openWalletSession(generatedViewingKey)).rejects.toThrow();
    });

    /**
     * Opening a session with a valid viewing key meant for a different network returns an error
     *
     * @given a valid viewing key meant for a different network
     * @when we open a session with that viewing key
     * @then Indexer should return an error
     */
    test('should return an error, given a valid viewing key meant for a different network', async (ctx: TestContext) => {
      log.info(`Seed for viewing key = ${randomSeed}`);

      // Get all the ledger network ids
      const networkIds = Object.values(LedgerNetworkId);
      for (const networkId of networkIds) {
        log.debug(`networkId = ${networkId}`);
        const viewingKey = await toolkit.showViewingKey(randomSeed, networkId);
        log.debug(`viewingKey = ${viewingKey}`);
        if (networkId === env.getNetworkId().toLowerCase()) {
          continue;
        }
        await expect
          .soft(indexerWsClient.openWalletSession(viewingKey))
          .rejects.toThrow(/expected HRP.*but was/);
      }
    });
  });

  describe('closing a session with session ID', async () => {
    /**
     * Closing a session with a valid session ID terminates the session successfully
     *
     * @given a valid session ID obtained from opening a wallet session
     * @when we close the session with that session ID
     * @then Indexer should terminate the session successfully
     */
    test('should terminate the session successfully, given a valid session ID', async () => {
      // Gets the viewing key for the random seed using toolkit
      const viewingKey = await toolkit.showViewingKey(randomSeed);
      log.debug(`viewingKey = ${viewingKey}`);

      const sessionId = await indexerWsClient.openWalletSession(viewingKey);

      return indexerWsClient
        .closeWalletSession(sessionId)
        .then((message: GraphQLCloseSessionMessage) => {
          log.debug(`Received message = ${JSON.stringify(message, null, 2)}`);
          expect(message.payload.data.disconnect).toBeDefined();
        })
        .catch((error) => {
          log.error(error);
          throw new Error(error);
        });
    });

    /**
     * Closing a session with an invalid session ID returns an error
     *
     * @given an invalid session ID
     * @when we attempt to close a session with that session ID
     * @then Indexer should return an error
     */
    test('should return an error, given an invalid session ID', async () => {
      const sessionId = 'invalid-session-id';
      log.debug(`sessionId = ${sessionId}`);

      await expect
        .soft(indexerWsClient.closeWalletSession(sessionId))
        .rejects.toThrow(/Unexpected payload in disconnect response/);
    });
  });

  describe('a subscription to wallet updates providing viewing key only', async () => {
    /**
     * Subscribing to wallet updates with a valid viewing key streams wallet events
     *
     * @given a valid viewing key and an open wallet session
     * @when we subscribe to shielded transaction events for that session
     * @then Indexer should stream wallet events starting from the beginning
     * @and we should receive at least one event
     */
    test('should stream wallet events starting from the beginning, given there are relevant transactions', async () => {
      // Seed with transaction from which we get viewing key
      const seedWithTransactions = '0'.repeat(63) + '1';
      const viewingKey = await toolkit.showViewingKey(seedWithTransactions);
      log.debug(`viewingKey = ${viewingKey}`);

      const sessionId: string = await indexerWsClient.openWalletSession(viewingKey);

      const receivedEvents: ShieldedTxSubscriptionResponse[] = [];
      const shieldedTxSubscriptionHandler: SubscriptionHandlers<ShieldedTxSubscriptionResponse> = {
        next: (payload) => {
          log.debug(`Received data:\n${JSON.stringify(payload)}`);
          receivedEvents.push(payload);
        },
        complete: () => {
          log.debug('Completed sent from Indexer');
        },
      };

      const unsubscribe = indexerWsClient.subscribeToShieldedTransactionEvents(
        shieldedTxSubscriptionHandler,
        sessionId,
      );

      await new Promise((res) => setTimeout(res, 2000));

      unsubscribe();

      expect(receivedEvents.length).toBeGreaterThanOrEqual(1);
      receivedEvents.forEach((event) => {
        expect(event).toBeSuccess();
      });
    });
  });
});
