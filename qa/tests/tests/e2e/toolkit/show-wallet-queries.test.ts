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
import { ToolkitWrapper } from '@utils/toolkit/toolkit-wrapper';
import {
  type PrivateWalletState,
  type PublicWalletState,
  PrivateWalletStateSchema,
  PublicWalletStateSchema,
} from '@utils/toolkit/schemas';
import { env } from 'environment/model';
import { seedFingerprint } from '@utils/moth/moth-backend';

const TOOLKIT_STARTUP_TIMEOUT = 60_000;

// Under TX_BACKEND=moth the wallet state comes from moth's synced wallet, not
// from the toolkit's `show-wallet`, so these tests then exercise the moth
// mapping into the toolkit's schema rather than the toolkit's own output. The
// suite is named after the backend so a pass is not read as toolkit coverage.
// The first read in a worker restores the wallet from the cache global setup
// warmed, which can outlast a toolkit call against a short local chain.
const isMoth = env.getTxBackend() === 'moth';
const BACKEND = isMoth ? 'moth' : 'toolkit';
const BACKEND_LABEL = isMoth ? 'Moth' : 'Toolkit';
const WALLET_STATE_TIMEOUT = isMoth ? 180_000 : TOOLKIT_STARTUP_TIMEOUT;

describe(`show wallet queries using ${BACKEND}`, () => {
  let toolkit: ToolkitWrapper;

  // The container is still needed under moth: `showAddress` derives the
  // unshielded address through the toolkit.
  beforeAll(async () => {
    toolkit = new ToolkitWrapper({});
    await toolkit.start();
  }, TOOLKIT_STARTUP_TIMEOUT);

  afterAll(async () => {
    await toolkit.stop();
  });

  describe(`private wallet state query using ${BACKEND}`, () => {
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
        const walletState: PrivateWalletState = await toolkit.showPrivateWalletState(walletSeed);

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

  describe(`public wallet state query using ${BACKEND}`, () => {
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

        // Get the unshielded address from the seed
        const addressInfo = await toolkit.showAddress(walletSeed);
        const walletAddress = addressInfo.unshielded;

        log.debug(`Querying public wallet state for address: ${walletAddress}`);
        const publicWalletState: PublicWalletState = await toolkit.showPublicWalletState(
          walletAddress,
          walletSeed,
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
