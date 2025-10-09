// This file is part of midnightntwrk/midnight-indexer
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
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

// To run: yarn test e2e
describe('mn-toolkit contract calls', () => {
  let toolkit: ToolkitWrapper;

  beforeAll(async () => {
    toolkit = new ToolkitWrapper({});
    await toolkit.start();
  });

  afterAll(async () => {
    await toolkit.stop();
  });

  /**
   * Test contract call using the deployed test contract
   *
   * @given we have a deployed contract at a known address
   * @when we call a contract function using the toolkit
   * @then the transaction should be generated and submitted successfully
   */
  test('mn-toolkit contract call test', async (context: TestContext) => {
    let contractAddress: string;
    try {
      contractAddress = dataProvider.getKnownContractAddress();
    } catch (error) {
      log.warn(error);
      context.skip?.(true, (error as Error).message);
      return;
    }

    const callKey = 'store';
    const rngSeed = '0000000000000000000000000000000000000000000000000000000000000037';

    const result = await toolkit.callContract(contractAddress, callKey, rngSeed);

    log.info(`Contract call transaction hash: ${result.txHash}`);
    log.info(`Contract call status: ${result.status}`);
    if (result.blockHash) {
      log.info(`Contract call block hash: ${result.blockHash}`);
    }

    expect(result.txHash).toMatch(/^0x[a-f0-9]{64}$/);
    expect(result.status).toMatch(/^(sent|confirmed)$/);
    expect(['sent', 'confirmed']).toContain(result.status);
  }, 600000); // Increase timeout to 10 minutes for contract calls (syncing + proving + submitting)
});
