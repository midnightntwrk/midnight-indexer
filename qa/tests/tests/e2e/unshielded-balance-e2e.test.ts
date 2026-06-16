// This file is part of midnightntwrk/midnight-indexer
// Copyright (C) Midnight Foundation
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

// SCAFFOLD for #1253 (for @whankinsiv): deploy minter → mint N to self → send M < N →
// assert the indexer reports the contract's N - M remainder via unshieldedBalances.
// describe.skip until the two TODOs (MINTER_CONTRACT paths; ledger-8 validation) are wired.

import type { TestContext } from 'vitest';
import '@utils/logging/test-logging-hooks';
import dataProvider from '@utils/testdata-provider';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { ToolkitWrapper, type MinterContract } from '@utils/toolkit/toolkit-wrapper';

const TOOLKIT_WRAPPER_TIMEOUT = 120_000;
const CONTRACT_ACTION_TIMEOUT = 180_000;
const TEST_TIMEOUT = 30_000;

// TODO(#1253): set to the compiled minter contract paths inside the toolkit container.
const MINTER_CONTRACT: MinterContract = {
  compiledContractDir: '/toolkit-js/contract/out',
  configFile: '/toolkit-js/contract/minter.config.ts',
  toolkitJsPath: '/toolkit-js',
};

const DOMAIN_SEP = 'feeb000000000000000000000000000000000000000000000000000000000000';
const MINT_AMOUNT = 1000;
const SEND_AMOUNT = 400; // remainder the contract should still hold: 600

describe.skip('unshielded balance indexing (deploy + mint + verify) [#1253 scaffold]', () => {
  let indexerHttpClient: IndexerHttpClient;
  let toolkit: ToolkitWrapper;
  let fundingSeed: string;

  beforeAll(async () => {
    indexerHttpClient = new IndexerHttpClient();
    fundingSeed = dataProvider.getFundingSeed();
    toolkit = new ToolkitWrapper({});
    await toolkit.start();
  }, TOOLKIT_WRAPPER_TIMEOUT);

  afterAll(async () => {
    await toolkit.stop();
  });

  test(
    'reports the contract remainder after mint-to-self then partial send',
    async (context: TestContext) => {
      context.task!.meta.custom = {
        labels: ['ContractAction', 'unshieldedBalances', 'Mint', 'E2E'],
      };

      // Deploy → mint N to self → send M to user.
      const result = await toolkit.deployMintSendUnshielded({
        contract: MINTER_CONTRACT,
        domainSep: DOMAIN_SEP,
        mintAmount: MINT_AMOUNT,
        sendAmount: SEND_AMOUNT,
        fundingSeed,
      });

      // TODO(#1253): wait for the indexer to catch up (retry helper) before asserting.
      const response = await indexerHttpClient.getContractAction(result.contractAddress);
      const balances = response.data?.contractAction?.unshieldedBalances ?? [];

      const held = balances.find((balance) => balance.tokenType === result.tokenType);
      expect(held).toBeDefined();
      expect(held?.amount).toBe(String(result.expectedRemainder));
    },
    CONTRACT_ACTION_TIMEOUT + TEST_TIMEOUT,
  );
});
