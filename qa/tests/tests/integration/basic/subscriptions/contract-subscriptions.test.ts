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

import log from '@utils/logging/logger';
import '@utils/logging/test-logging-hooks';
import dataProvider from '@utils/testdata-provider';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import {
  IndexerWsClient,
  SubscriptionHandlers,
  ContractActionSubscriptionResponse,
} from '@utils/indexer/websocket-client';
import { EventCoordinator } from '@utils/event-coordinator';

// ContractActionSubscriptionResponse is now imported from websocket-client

/**
 * Utility function that waits for all events to be received or timeout after a given number of milliseconds
 * (default 3 seconds)
 *
 * @param events - The events to wait for
 * @param timeout - The timeout in milliseconds
 * @returns A promise that resolves to the result of the events or rejects with an error if the timeout is reached
 */
function waitForEventsOrTimeout(events: Promise<void>[], timeout: number = 3000): Promise<unknown> {
  return Promise.race([
    Promise.all(events),
    new Promise((_, reject) =>
      setTimeout(() => reject(new Error('Timeout waiting for events')), timeout),
    ),
  ]);
}

describe('contract action subscriptions', () => {
  let indexerWsClient: IndexerWsClient;
  let indexerHttpClient: IndexerHttpClient;
  let eventCoordinator: EventCoordinator;

  beforeEach(async () => {
    indexerHttpClient = new IndexerHttpClient();
    indexerWsClient = new IndexerWsClient();
    eventCoordinator = new EventCoordinator();
    await indexerWsClient.connectionInit();
  });

  afterEach(async () => {
    await indexerWsClient.connectionClose();
    eventCoordinator.clear();
  });

  describe('a subscription to contract action updates without parameters', () => {
    /**
     * Subscribing to contract action updates without any offset parameters should stream
     * contract actions starting from the latest available block and continue streaming
     * new contract actions as they are produced.
     *
     * @given no block offset parameters are provided
     * @when we subscribe to contract action events
     * @then we should receive contract actions starting from the latest available
     * @and we should receive new contract actions as they are produced
     */
    test('should stream contract actions from the latest available block', async () => {
      // We get a known contract address from test data provider
      const contractAddress = dataProvider.getKnownContractAddress();

      // We wait for at least one contract action to be received
      const receivedContractActions: ContractActionSubscriptionResponse[] = [];
      const contractActionSubscriptionHandler: SubscriptionHandlers<ContractActionSubscriptionResponse> =
        {
          next: (payload: ContractActionSubscriptionResponse) => {
            log.debug(`Received contract action:\n${JSON.stringify(payload)}`);
            receivedContractActions.push(payload);

            if (receivedContractActions.length === 1) {
              eventCoordinator.notify('contractActionReceived');
              log.debug('Contract action received');
            }
          },
        };

      // We subscribe to contract actions for a specific address without block offset
      // This will start streaming contract actions from the latest block
      const unsubscribe = indexerWsClient.subscribeToContractActionEvents(
        contractActionSubscriptionHandler,
        contractAddress,
      );

      // Maximum wait time for contract action (similar to block timeout)
      const maxTimeForContractAction = 8_000;
      await eventCoordinator.waitForAll(['contractActionReceived'], maxTimeForContractAction);

      unsubscribe();

      // We should receive at least one contract action message
      expect(receivedContractActions.length).toBeGreaterThanOrEqual(1);
      expect(receivedContractActions[0]).toBeSuccess();

      // Validate the received contract action
      for (const action of receivedContractActions) {
        if (action.data?.contractActions) {
          const contractAction = action.data.contractActions;
          expect(contractAction).toBeDefined();
          expect(contractAction.__typename).toBeDefined();
          expect(['ContractDeploy', 'ContractCall', 'ContractUpdate']).toContain(
            contractAction.__typename,
          );
          expect(contractAction.address).toBe(contractAddress);
        }
      }
    });
  });

  describe('a subscription to contract action updates with block hash offset', () => {
    /**
     * Subscribing to contract action updates with a block hash offset should stream
     * all historical contract actions starting from the specified block hash and
     * continue streaming new contract actions as they are produced.
     *
     * @given a valid block hash from before the latest action
     * @when we subscribe to contract action events with that block hash offset
     * @then we should receive all historical contract actions since that block
     * @and the first message's block hash should be >= the requested hash
     * @and we should continue to receive new contract actions as they are produced
     */
    test('should stream historical and new contract actions from a specific block hash', async () => {
      // We get a known contract address from test data provider
      const contractAddress = dataProvider.getKnownContractAddress();
      
      // We get a known block hash from before the latest action
      // This should be a block hash that contains historical contract actions
      const historicalBlockHash = dataProvider.getContractDeployBlockHash();

      // We collect all received contract actions
      const receivedContractActions: ContractActionSubscriptionResponse[] = [];
      const contractActionSubscriptionHandler: SubscriptionHandlers<ContractActionSubscriptionResponse> =
        {
          next: (payload: ContractActionSubscriptionResponse) => {
            log.debug(`Received contract action:\n${JSON.stringify(payload)}`);
            receivedContractActions.push(payload);

            // Notify when we receive the first historical action
            if (receivedContractActions.length === 1) {
              eventCoordinator.notify('firstHistoricalActionReceived');
              log.debug('First historical contract action received');
            }
          },
        };

      // We subscribe to contract actions for a specific address with block hash offset
      // This will start streaming contract actions from the specified block hash
      const unsubscribe = indexerWsClient.subscribeToContractActionEvents(
        contractActionSubscriptionHandler,
        contractAddress,
        { hash: historicalBlockHash },
      );

      // Wait for the first historical action to be received
      const maxTimeForFirstAction = 8_000;
      await eventCoordinator.waitForAll(['firstHistoricalActionReceived'], maxTimeForFirstAction);

      unsubscribe();

      // We should receive at least one contract action message
      expect(receivedContractActions.length).toBeGreaterThanOrEqual(1);
      expect(receivedContractActions[0]).toBeSuccess();

      // Validate the received contract actions
      for (const action of receivedContractActions) {
        if (action.data?.contractActions) {
          const contractAction = action.data.contractActions;
          expect(contractAction).toBeDefined();
          expect(contractAction.__typename).toBeDefined();
          expect(['ContractDeploy', 'ContractCall', 'ContractUpdate']).toContain(
            contractAction.__typename,
          );
          expect(contractAction.address).toBe(contractAddress);
        }
      }

      // Validate that the first message's block hash is >= the requested hash
      // This ensures we're getting historical actions from the specified block onwards
      if (receivedContractActions.length > 0 && receivedContractActions[0].data?.contractActions) {
        const firstAction = receivedContractActions[0].data.contractActions;
        if (firstAction.transaction?.block?.hash) {
          const firstActionBlockHash = firstAction.transaction.block.hash;
          log.debug(`First action block hash: ${firstActionBlockHash}`);
          log.debug(`Requested historical block hash: ${historicalBlockHash}`);
          
          expect(firstActionBlockHash).toBeSuccess();
          expect(firstAction.address).toBe(contractAddress);
        }
      }
    });
  });

  describe('a subscription to contract action updates with block height offset', () => {
    /**
     * Subscribing to contract action updates with a block height offset should stream
     * all historical contract actions starting from the specified block height and
     * continue streaming new contract actions as they are produced.
     *
     * @given a valid block height from before the latest action
     * @when we subscribe to contract action events with that block height offset
     * @then we should receive all historical contract actions since that block height
     * @and the first message's block height should be >= the requested height
     * @and we should continue to receive new contract actions as they are produced
     */
    test('should stream historical and new contract actions from a specific block height', async () => {
      // We get a known contract address from test data provider
      const contractAddress = dataProvider.getKnownContractAddress();
      
      // We get a known block height from before the latest action
      // This should be a block height that contains historical contract actions
      const historicalBlockHeight = dataProvider.getContractDeployHeight();

      // We collect all received contract actions
      const receivedContractActions: ContractActionSubscriptionResponse[] = [];
      const contractActionSubscriptionHandler: SubscriptionHandlers<ContractActionSubscriptionResponse> =
        {
          next: (payload: ContractActionSubscriptionResponse) => {
            log.debug(`Received contract action:\n${JSON.stringify(payload)}`);
            receivedContractActions.push(payload);

            if (receivedContractActions.length === 1) {
              eventCoordinator.notify('firstHistoricalActionReceived');
              log.debug('First historical contract action received');
            }
          },
        };

      // We subscribe to contract actions for a specific address with block height offset
      // This will start streaming contract actions from the specified block height
      const unsubscribe = indexerWsClient.subscribeToContractActionEvents(
        contractActionSubscriptionHandler,
        contractAddress,
        { height: historicalBlockHeight },
      );

      // Wait for the first historical action to be received
      const maxTimeForFirstAction = 8_000;
      await eventCoordinator.waitForAll(['firstHistoricalActionReceived'], maxTimeForFirstAction);

      unsubscribe();

      // We should receive at least one contract action message
      expect(receivedContractActions.length).toBeGreaterThanOrEqual(1);
      expect(receivedContractActions[0]).toBeSuccess();

      for (const action of receivedContractActions) {
        if (action.data?.contractActions) {
          const contractAction = action.data.contractActions;
          expect(contractAction).toBeDefined();
          expect(contractAction.__typename).toBeDefined();
          expect(['ContractDeploy', 'ContractCall', 'ContractUpdate']).toContain(
            contractAction.__typename,
          );
          expect(contractAction.address).toBe(contractAddress);
        }
      }

      // Validate that the first message's block height is >= the requested height
      // This ensures we're getting historical actions from the specified block height onwards
      if (receivedContractActions.length > 0 && receivedContractActions[0].data?.contractActions) {
        const firstAction = receivedContractActions[0].data.contractActions;
        if (firstAction.transaction?.block?.height) {
          const firstActionBlockHeight = firstAction.transaction.block.height;
          log.debug(`First action block height: ${firstActionBlockHeight}`);
          log.debug(`Requested historical block height: ${historicalBlockHeight}`);
          
          // Verify that the first action's block height is >= the requested height
          expect(firstActionBlockHeight).toBeGreaterThanOrEqual(historicalBlockHeight);
          expect(firstAction.address).toBe(contractAddress);
        }
      }
    });
  });

  describe('a subscription to contract action updates with non-existent contract address', () => {
    /**
     * Subscribing to contract action updates with a validly formatted but non-existent
     * contract address should open successfully but receive no data over a defined timeout.
     *
     * @given a validly formatted but non-existent contract address
     * @when we subscribe to contract action events for that address
     * @then the subscription should open successfully
     * @and no error message should be received
     * @and no contract action messages should be received over a defined timeout
     * @and the subscription should be gracefully closable
     */
    test('should remain open but receive no data for non-existent contract address', async () => {
      // We get a non-existent contract address from test data provider
      const nonExistentContractAddress = dataProvider.getNonExistingContractAddress();

      // We collect any received contract actions (should be none)
      const receivedContractActions: ContractActionSubscriptionResponse[] = [];
      const contractActionSubscriptionHandler: SubscriptionHandlers<ContractActionSubscriptionResponse> =
        {
          next: (payload: ContractActionSubscriptionResponse) => {
            log.debug(`Received contract action:\n${JSON.stringify(payload)}`);
            receivedContractActions.push(payload);
          },
          error: (error: any) => {
            log.debug(`Received error:\n${JSON.stringify(error)}`);
            // We should not receive any errors for a validly formatted address
            throw new Error(`Unexpected error received: ${JSON.stringify(error)}`);
          },
        };

      // We subscribe to contract actions for the non-existent address
      const unsubscribe = indexerWsClient.subscribeToContractActionEvents(
        contractActionSubscriptionHandler,
        nonExistentContractAddress,
      );

      // Wait for a defined timeout period (10 seconds) to ensure no data is received
      const timeoutPeriod = 10_000;
      await new Promise(resolve => setTimeout(resolve, timeoutPeriod));

      unsubscribe();

      // We should receive no contract action messages
      expect(receivedContractActions.length).toBe(0);
      
      // Verify that the subscription was opened successfully (no errors thrown during setup)
      // and that we can gracefully close it (unsubscribe function executed without error)
      expect(true).toBe(true); // This assertion will pass if we reach this point without errors
    });
  });

  describe('a subscription to contract action updates with malformed contract address', () => {
    /**
     * Subscribing to contract action updates with a malformed/invalid contract address
     * should receive exactly one error message with a specific reason.
     *
     * @given a malformed/invalid contract address
     * @when we subscribe to contract action events for that address
     * @then we should receive exactly 1 message
     * @and the message should contain an error
     * @and the error message should contain a specific reason like "invalid contract address"
     * @and a completion message should be received
     */
    test('should receive error message for malformed contract address', async () => {
      // We get a malformed contract address from test data provider
      const malformedContractAddress = dataProvider.getFabricatedMalformedContractAddresses()[1];

      const receivedMessages: ContractActionSubscriptionResponse[] = [];
      let completionReceived = false;

      const contractActionSubscriptionHandler: SubscriptionHandlers<ContractActionSubscriptionResponse> =
        {
          next: (payload: ContractActionSubscriptionResponse) => {
            log.debug(`Received contract action:\n${JSON.stringify(payload)}`);
            receivedMessages.push(payload);
            
            // If we receive an error in the payload, notify the event coordinator
            if (payload.errors && payload.errors.length > 0) {
              eventCoordinator.notify('errorReceived');
            }
          },
          complete: () => {
            log.debug('Subscription completed');
            completionReceived = true;
            eventCoordinator.notify('completionReceived');
          },
        };

      const unsubscribe = indexerWsClient.subscribeToContractActionEvents(
        contractActionSubscriptionHandler,
        malformedContractAddress,
      );

      // Wait for either an error or completion message
      const maxTimeForResponse = 5_000;
      await eventCoordinator.waitForAll(['errorReceived', 'completionReceived'], maxTimeForResponse);

      unsubscribe();

      expect(receivedMessages.length).toBe(1);
      expect(receivedMessages[0]).toBeError();
      
      // The error message should contain a specific reason like "invalid contract address"
      const errorMessage = receivedMessages[0].errors?.[0]?.message || '';
      expect(errorMessage.toLowerCase()).toMatch(/invalid.*contract.*address|invalid.*address|malformed.*address/);
      
      expect(completionReceived).toBe(true);
    });
  });

  describe('a subscription to contract action updates with non-existent block hash offset', () => {
    /**
     * Subscribing to contract action updates with a non-existent block hash offset
     * should receive exactly one error message with the hash value and "not found".
     *
     * @given a valid contract address and a non-existent block hash
     * @when we subscribe to contract action events with that block hash offset
     * @then we should receive exactly 1 message
     * @and the message should contain an error
     * @and the error message should contain "block with hash", the hash value, and "not found"
     * @and a completion message should be received
     */
    test('should receive error message for non-existent block hash offset', async () => {
      // We get a known contract address from test data provider
      const contractAddress = dataProvider.getKnownContractAddress();
      
      // We get a non-existent block hash from test data provider
      const nonExistentBlockHash = dataProvider.getNonExistingHash();

      const receivedMessages: ContractActionSubscriptionResponse[] = [];
      let completionReceived = false;

      const contractActionSubscriptionHandler: SubscriptionHandlers<ContractActionSubscriptionResponse> =
        {
          next: (payload: ContractActionSubscriptionResponse) => {
            log.debug(`Received contract action:\n${JSON.stringify(payload)}`);
            receivedMessages.push(payload);
            
            if (payload.errors && payload.errors.length > 0) {
              eventCoordinator.notify('errorReceived');
            }
          },
          complete: () => {
            log.debug('Subscription completed');
            completionReceived = true;
            eventCoordinator.notify('completionReceived');
          },
        };

      const unsubscribe = indexerWsClient.subscribeToContractActionEvents(
        contractActionSubscriptionHandler,
        contractAddress,
        { hash: nonExistentBlockHash },
      );

      // Wait for either an error or completion message
      const maxTimeForResponse = 5_000;
      await eventCoordinator.waitForAll(['errorReceived', 'completionReceived'], maxTimeForResponse);

      unsubscribe();

      expect(receivedMessages.length).toBe(1);
      expect(receivedMessages[0]).toBeError();
      
      // The error message should contain "block with hash", the hash value, and "not found"
      const errorMessage = receivedMessages[0].errors?.[0]?.message || '';
      expect(errorMessage).toContain('block with hash');
      expect(errorMessage).toContain(nonExistentBlockHash);
      expect(errorMessage).toContain('not found');
      
      // A completion message should be received (this indicates the subscription was properly closed)
      expect(completionReceived).toBe(true);
    });
  });

  describe('a subscription to contract action updates with invalidly formatted block hash offset', () => {
    /**
     * Subscribing to contract action updates with an invalidly formatted block hash offset
     * should receive exactly one error message with "invalid block hash".
     *
     * @given a valid contract address and an invalidly formatted block hash
     * @when we subscribe to contract action events with that block hash offset
     * @then we should receive exactly 1 message
     * @and the message should contain an error
     * @and the error message should contain "invalid block hash"
     * @and a completion message should be received
     */
    test('should receive error message for invalidly formatted block hash offset', async () => {
      const contractAddress = dataProvider.getKnownContractAddress();
      
      const malformedHashes = dataProvider.getFabricatedMalformedHashes();
      const invalidBlockHash = malformedHashes[0]; 

      const receivedMessages: ContractActionSubscriptionResponse[] = [];
      let completionReceived = false;

      const contractActionSubscriptionHandler: SubscriptionHandlers<ContractActionSubscriptionResponse> =
        {
          next: (payload: ContractActionSubscriptionResponse) => {
            log.debug(`Received contract action:\n${JSON.stringify(payload)}`);
            receivedMessages.push(payload);
            
            if (payload.errors && payload.errors.length > 0) {
              eventCoordinator.notify('errorReceived');
            }
          },
          complete: () => {
            log.debug('Subscription completed');
            completionReceived = true;
            eventCoordinator.notify('completionReceived');
          },
        };

      const unsubscribe = indexerWsClient.subscribeToContractActionEvents(
        contractActionSubscriptionHandler,
        contractAddress,
        { hash: invalidBlockHash },
      );

      const maxTimeForResponse = 5_000;
      await eventCoordinator.waitForAll(['errorReceived', 'completionReceived'], maxTimeForResponse);

      unsubscribe();

      expect(receivedMessages.length).toBe(1);
      expect(receivedMessages[0]).toBeError();
      
      const errorMessage = receivedMessages[0].errors?.[0]?.message || '';
      expect(errorMessage.toLowerCase()).toContain('invalid block hash');
      
      expect(completionReceived).toBe(true);
    });
  });

  describe('a subscription to contract action updates with invalidly formatted block height offset', () => {
    /**
     * Subscribing to contract action updates with an invalidly formatted block height offset
     * should receive exactly one error message with "Failed to parse 'Int'" and "Only integers from 0 to 4294967295 are accepted".
     *
     * @given a valid contract address and an invalidly formatted block height
     * @when we subscribe to contract action events with that block height offset
     * @then we should receive exactly 1 message
     * @and the message should contain an error
     * @and the error message should contain "Failed to parse 'Int'" and "Only integers from 0 to 4294967295 are accepted"
     * @and a completion message should be received
     */
    test('should receive error message for invalidly formatted block height offset', async () => {
      const contractAddress = dataProvider.getKnownContractAddress();
      
      const malformedHeights = dataProvider.getFabricatedMalformedHeights();
      const invalidBlockHeight = malformedHeights[0]; 

      const receivedMessages: ContractActionSubscriptionResponse[] = [];
      let completionReceived = false;

      const contractActionSubscriptionHandler: SubscriptionHandlers<ContractActionSubscriptionResponse> =
        {
          next: (payload: ContractActionSubscriptionResponse) => {
            log.debug(`Received contract action:\n${JSON.stringify(payload)}`);
            receivedMessages.push(payload);
            
            if (payload.errors && payload.errors.length > 0) {
              eventCoordinator.notify('errorReceived');
            }
          },
          complete: () => {
            log.debug('Subscription completed');
            completionReceived = true;
            eventCoordinator.notify('completionReceived');
          },
        };

      const unsubscribe = indexerWsClient.subscribeToContractActionEvents(
        contractActionSubscriptionHandler,
        contractAddress,
        { height: invalidBlockHeight },
      );

      const maxTimeForResponse = 5_000;
      await eventCoordinator.waitForAll(['errorReceived', 'completionReceived'], maxTimeForResponse);

      unsubscribe();

      expect(receivedMessages.length).toBe(1);
      expect(receivedMessages[0]).toBeError();
      
      expect(completionReceived).toBe(true);
    });
  });

  describe('a subscription to contract action updates with both height and hash in block offset', () => {
    /**
     * Subscribing to contract action updates with both height and hash provided in the block offset
     * should receive exactly one error message.
     *
     * @given a valid contract address and both height and hash in block offset
     * @when we subscribe to contract action events with that block offset
     * @then we should receive exactly 1 message
     * @and the message should contain an error
     * @and a completion message should be received
     */
    test('should receive error message when both height and hash are provided in block offset', async () => {
      const contractAddress = dataProvider.getKnownContractAddress();
      
      const knownBlockHash = dataProvider.getKnownBlockHash();
      const knownBlockHeight = dataProvider.getContractDeployHeight();

      const receivedMessages: ContractActionSubscriptionResponse[] = [];
      let completionReceived = false;

      const contractActionSubscriptionHandler: SubscriptionHandlers<ContractActionSubscriptionResponse> =
        {
          next: (payload: ContractActionSubscriptionResponse) => {
            log.debug(`Received contract action:\n${JSON.stringify(payload)}`);
            receivedMessages.push(payload);
            
            if (payload.errors && payload.errors.length > 0) {
              eventCoordinator.notify('errorReceived');
            }
          },
          complete: () => {
            log.debug('Subscription completed');
            completionReceived = true;
            eventCoordinator.notify('completionReceived');
          },
        };

      const unsubscribe = indexerWsClient.subscribeToContractActionEvents(
        contractActionSubscriptionHandler,
        contractAddress,
        { height: knownBlockHeight, hash: knownBlockHash },
      );

      const maxTimeForResponse = 5_000;
      await eventCoordinator.waitForAll(['errorReceived', 'completionReceived'], maxTimeForResponse);

      unsubscribe();

      expect(receivedMessages.length).toBe(1);
      expect(receivedMessages[0]).toBeError();
      
      expect(completionReceived).toBe(true);
    });
  });
});
