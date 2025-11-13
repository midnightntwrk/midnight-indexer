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
import type { TestContext } from 'vitest';
import '@utils/logging/test-logging-hooks';
import {
  ToolkitWrapper,
  type PrivateWalletState,
  type PublicWalletState,
} from '@utils/toolkit/toolkit-wrapper';
import { PrivateWalletStateSchema, PublicWalletStateSchema } from '@utils/indexer/graphql/schema';
import { validateSchema } from '../test-utils';

const TOOLKIT_STARTUP_TIMEOUT = 60_000;

// Common test seed used across tests
const TEST_WALLET_SEED = '0000000000000000000000000000000000000000000000000000000000000001';

describe('show wallet queries using toolkit', () => {
  let toolkit: ToolkitWrapper;

  beforeAll(async () => {
    toolkit = new ToolkitWrapper({});
    await toolkit.start();
  }, TOOLKIT_STARTUP_TIMEOUT);

  afterAll(async () => {
    await toolkit.stop();
  });

  describe('private wallet state query using toolkit', () => {
    /**
     * A private wallet state query using the toolkit's showPrivateWalletState method should return
     * a valid private wallet state object according to the requested schema.
     *
     * @given we have a toolkit instance and a wallet seed
     * @when we call showPrivateWalletState with the seed
     * @then we should receive a valid PrivateWalletState object according to the requested schema
     */
    test('should respond with a private wallet state according to the requested schema', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Wallet', 'Toolkit', 'PrivateState', 'SchemaValidation'],
      };

      log.debug(`Querying private wallet state for seed: ${TEST_WALLET_SEED}`);
      const walletState: PrivateWalletState =
        await toolkit.showPrivateWalletState(TEST_WALLET_SEED);

      log.debug('Checking if we actually received a private wallet state');
      expect(walletState).toBeDefined();

      expect(() => {
        validateSchema(walletState, PrivateWalletStateSchema, 'private wallet state');
      }).not.toThrow();

      // Log summary for debugging
      log.debug(
        `Wallet state: ${Object.keys(walletState.coins).length} coins, ${walletState.utxos.length} UTXOs, ${walletState.dust_utxos.length} dust UTXOs`,
      );
      const totalCoinsValue = Object.values(walletState.coins).reduce(
        (sum, coin) => sum + coin.value,
        0,
      );
      const totalUtxosValue = walletState.utxos.reduce((sum, utxo) => sum + utxo.value, 0);
      log.debug(`Total coins value: ${totalCoinsValue}, Total UTXOs value: ${totalUtxosValue}`);
    });
  });

  describe('public wallet state query using toolkit', () => {
    /**
     * A public wallet state query using the toolkit's showPublicWalletState method should return
     * a valid public wallet state object according to the requested schema.
     *
     * @given we have a toolkit instance and a wallet address
     * @when we call showPublicWalletState with the address
     * @then we should receive a valid PublicWalletState object according to the requested schema
     */
    test('should respond with a public wallet state according to the requested schema', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Wallet', 'Toolkit', 'PublicState', 'SchemaValidation'],
      };

      // Get the unshielded address from the seed
      const addressInfo = await toolkit.showAddress(TEST_WALLET_SEED);
      const walletAddress = addressInfo.unshielded;

      log.debug(`Querying public wallet state for address: ${walletAddress}`);
      const publicWalletState: PublicWalletState =
        await toolkit.showPublicWalletState(walletAddress);

      log.debug('Checking if we actually received a public wallet state');
      expect(publicWalletState).toBeDefined();

      expect(() => {
        validateSchema(publicWalletState, PublicWalletStateSchema, 'public wallet state');
      }).not.toThrow();

      // Log summary for debugging
      log.debug(
        `Public wallet state: ${Object.keys(publicWalletState.coins).length} coins, ${publicWalletState.utxos.length} UTXOs, ${publicWalletState.dust_utxos.length} dust UTXOs`,
      );
    });
  });
});
