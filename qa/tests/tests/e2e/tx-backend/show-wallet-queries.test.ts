// This file is part of midnightntwrk/midnight-indexer
// Copyright (C) Midnight Foundation
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
import dataProvider from '@utils/testdata-provider';
import { createTxBackend } from '@utils/tx-backend';
import {
  type PrivateWalletState,
  type PublicWalletState,
  PrivateWalletStateSchema,
  PublicWalletStateSchema,
} from '@utils/toolkit/schemas';
import { seedFingerprint } from '@utils/moth/moth-backend';

const BACKEND_STARTUP_TIMEOUT = 60_000;
const WALLET_STATE_TIMEOUT = 180_000;

// The suite is named after the backend TX_BACKEND selects, so a pass is read as
// coverage of that backend. On moth the first read may restore the wallet from
// the cache global setup warmed, which is why the timeout is generous.
const backend = createTxBackend();
const BACKEND_LABEL = backend.name === 'moth' ? 'Moth' : 'Toolkit';

describe(`show wallet queries using ${backend.name}`, () => {
  beforeAll(() => backend.start(), BACKEND_STARTUP_TIMEOUT);

  afterAll(() => backend.stop());

  describe(`private wallet state query using ${backend.name}`, () => {
    /**
     * A private wallet state query should return a valid private wallet state object according
     * to the requested schema, whichever backend serves it: the toolkit's `show-wallet --seed`
     * command or the moth-synced wallet.
     *
     * @given a wallet seed
     * @when we query the private wallet state for that seed
     * @then we should receive a valid PrivateWalletState object according to the requested schema
     */
    test(
      'should respond with a private wallet state according to the requested schema',
      async (ctx: TestContext) => {
        ctx.task!.meta.custom = {
          labels: ['Query', 'Wallet', BACKEND_LABEL, 'PrivateState', 'SchemaValidation'],
        };

        // The transfer funding seed is the one global setup warms for moth (the
        // light seed, when one is configured). It equals the funding seed under
        // the toolkit backend.
        const walletSeed = dataProvider.getTransferFundingSeed();

        log.debug(
          `Querying private wallet state for seed fingerprint: ${seedFingerprint(walletSeed)}`,
        );
        const walletState: PrivateWalletState = await backend.privateWalletState(walletSeed);

        expect(walletState).toBeDefined();

        const validationResult = PrivateWalletStateSchema.safeParse(walletState);
        expect(
          validationResult.success,
          `PrivateWalletState validation failed: ${JSON.stringify(validationResult.error, null, 2)}`,
        ).toBe(true);
      },
      WALLET_STATE_TIMEOUT,
    );
  });

  describe(`public wallet state query using ${backend.name}`, () => {
    /**
     * A public wallet state query should return a valid public wallet state object according
     * to the requested schema, whichever backend serves it: the toolkit's `show-wallet --address`
     * command or the moth-synced wallet.
     *
     * @given a wallet address
     * @when we query the public wallet state for that address
     * @then we should receive a valid PublicWalletState object according to the requested schema
     */
    test(
      'should respond with a public wallet state according to the requested schema',
      async (ctx: TestContext) => {
        ctx.task!.meta.custom = {
          labels: ['Query', 'Wallet', BACKEND_LABEL, 'PublicState', 'SchemaValidation'],
        };

        // Same seed as the private state query, for the same reason.
        const walletSeed = dataProvider.getTransferFundingSeed();

        const walletAddress = (await backend.addresses(walletSeed)).unshielded;

        log.debug(`Querying public wallet state for address: ${walletAddress}`);
        const publicWalletState: PublicWalletState = await backend.publicWalletState(
          walletSeed,
          walletAddress,
        );

        expect(publicWalletState).toBeDefined();

        const validationResult = PublicWalletStateSchema.safeParse(publicWalletState);
        expect(
          validationResult.success,
          `PublicWalletState validation failed: ${JSON.stringify(validationResult.error, null, 2)}`,
        ).toBe(true);
      },
      WALLET_STATE_TIMEOUT,
    );
  });
});
