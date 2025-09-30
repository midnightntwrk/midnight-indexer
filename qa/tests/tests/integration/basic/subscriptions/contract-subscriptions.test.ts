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
import type { BlockOffset } from '@utils/indexer/indexer-types';
import {
  IndexerWsClient,
  SubscriptionHandlers,
  ContractActionSubscriptionResponse,
  GraphQLCompleteMessage,
} from '@utils/indexer/websocket-client';
import { EventCoordinator } from '@utils/event-coordinator';
import type { TestContext } from 'vitest';

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
});
