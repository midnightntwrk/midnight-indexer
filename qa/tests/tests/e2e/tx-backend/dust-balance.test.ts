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
import type { DustBalance } from '@utils/toolkit/toolkit-wrapper';
import { createTxBackend } from '@utils/tx-backend';
import { DustBalanceSchema } from '@utils/toolkit/schemas';
import { seedFingerprint } from '@utils/moth/moth-backend';

const BACKEND_STARTUP_TIMEOUT = 60_000;
const DUST_BALANCE_TIMEOUT = 180_000;

// The suite is named after the backend TX_BACKEND selects, so a pass is read as
// coverage of that backend. On moth the first read may restore the wallet from
// the cache global setup warmed, which is why the timeout is generous.
const backend = createTxBackend();
const BACKEND_LABEL = backend.name === 'moth' ? 'Moth' : 'Toolkit';

describe(`dust balance query using ${backend.name}`, () => {
  beforeAll(() => backend.start(), BACKEND_STARTUP_TIMEOUT);

  afterAll(() => backend.stop());

  describe('a dust balance query with a valid wallet seed', () => {
    /**
     * A dust balance query should return a valid dust balance object according to the
     * requested schema, whichever backend serves it: the toolkit's `dust-balance` command or
     * the moth-synced wallet.
     *
     * @given a wallet seed
     * @when we query the dust balance for that seed
     * @then we should receive a valid DustBalance object according to the requested schema
     */
    test(
      'should respond with a dust balance according to the requested schema',
      async (ctx: TestContext) => {
        ctx.task!.meta.custom = {
          labels: ['Query', 'Dust', BACKEND_LABEL, 'Balance', 'SchemaValidation'],
        };

        // The transfer funding seed is the one global setup warms for moth (the
        // light seed, when one is configured). It equals the funding seed under
        // the toolkit backend.
        const walletSeed = dataProvider.getTransferFundingSeed();

        log.debug(`Querying dust balance for seed fingerprint: ${seedFingerprint(walletSeed)}`);

        const dustBalance: DustBalance = await backend.dustBalance(walletSeed);

        log.debug('Checking if we actually received a dust balance');
        expect(dustBalance).toBeDefined();

        const validationResult = DustBalanceSchema.safeParse(dustBalance);
        expect(
          validationResult.success,
          `DUST balance schema validation failed: ${JSON.stringify(validationResult.error, null, 2)}`,
        ).toBe(true);

        expect(dustBalance.total).toBeGreaterThan(0);
        log.debug(`Dust balance total: ${dustBalance.total}`);
      },
      DUST_BALANCE_TIMEOUT,
    );
  });
});
