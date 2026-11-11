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

import path from 'node:path';
import { fileURLToPath } from 'node:url';
import '@utils/logging/test-logging-hooks';
import log from '@utils/logging/logger';
import dataProvider from '@utils/testdata-provider';
import {
  COMPACT_COMPILER_VERSION,
  compileCompactContract,
  compiledRuntimeVersion,
} from '@utils/compact/compact-compiler';
import { env } from 'environment/model';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import type { ContractAction } from '@utils/indexer/indexer-types';
import { getBlockByHashWithRetry, getTransactionByHashWithRetry, retry } from './test-utils';
import {
  ToolkitWrapper,
  type CustomContractCall,
  type CustomContractSpec,
  type DeployContractResult,
  type ToolkitTransactionResult,
} from '@utils/toolkit/toolkit-wrapper';

// First use also builds the Compact toolchain image and pulls the toolkit image.
const SETUP_TIMEOUT = 900_000; // 15 minutes
const CONTRACT_ACTION_TIMEOUT = 600_000; // 10 minutes: deploy plus two proven calls
const TEST_TIMEOUT = 120_000; // 2 minutes

/** See the comment in the `.compact` source for how the circuit gets a shielded balance. */
const SHIELDED_BALANCE_DIR = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../../contracts/shielded-balance',
);
const SHIELDED_BALANCE_SOURCE = 'shielded-balance.compact';
const SHIELDED_BALANCE: CustomContractSpec = { configFile: 'shielded-balance.config.ts' };

// Undeployed only: the toolkit rebuilds ledger state from genesis on every call, which on a
// long-lived deployed chain takes hours per step (see Env gating in the test conventions).
describe
  .skipIf(!env.isUndeployedEnv())
  .sequential('a contract whose balance map holds a shielded token', () => {
    let toolkit: ToolkitWrapper;
    let fundingSeed: string;
    let deployment: DeployContractResult;
    let contractAddress: string;
    let holdCall: CustomContractCall;
    let holdDecoded: string;
    let hold: ToolkitTransactionResult | undefined;
    let ping: ToolkitTransactionResult | undefined;

    beforeAll(async () => {
      fundingSeed = dataProvider.getFundingSeed();

      const compiledDir = await compileCompactContract({
        sourceDir: SHIELDED_BALANCE_DIR,
        sourceFile: SHIELDED_BALANCE_SOURCE,
        stage: [SHIELDED_BALANCE.configFile],
      });
      const requiredRuntime = compiledRuntimeVersion(compiledDir);
      log.info(
        `shielded-balance compiled by compactc ${COMPACT_COMPILER_VERSION} ` +
          `for compact-runtime ${requiredRuntime ?? 'unknown'}`,
      );

      toolkit = new ToolkitWrapper({
        customContractDir: compiledDir,
        compactcVersion: COMPACT_COMPILER_VERSION,
      });
      await toolkit.start();
      if (requiredRuntime) {
        await toolkit.assertCompactRuntimeSupported(requiredRuntime);
      }
    }, SETUP_TIMEOUT);

    // The toolkit only exists once the image build and the compile succeeded;
    // stopping an unassigned one here would bury the failure that stopped setup.
    afterAll(async () => {
      await toolkit?.stop();
    });

    /**
     * Build one call against the contract's current on-chain state. Each call is
     * built only after the previous one is in a block: building two up front makes
     * them spend the same DUST fee input and the node rejects the second one.
     */
    async function buildCall(circuitId: string, label: string): Promise<CustomContractCall> {
      const stateFile = await toolkit.snapshotContractState(contractAddress, `${label}_state.bin`);
      return toolkit.generateCustomContractCall({
        circuitId,
        deploymentResult: deployment,
        contract: SHIELDED_BALANCE,
        onchainStateFile: stateFile,
        label,
        fundingSeed,
      });
    }

    /**
     * Deploy and build the `holdShielded` call, but do not send it yet. Sending is
     * part of the tests below: the wrapper learns a call's block from the indexer
     * (the toolkit does not print it), so on an indexer that stops at that block
     * the send itself never completes. Doing it inside a test turns that into a
     * clear failure instead of a setup error that skips everything.
     */
    beforeAll(async () => {
      deployment = await toolkit.deployCustomContract(SHIELDED_BALANCE, [], fundingSeed);
      contractAddress = deployment['contract-address-untagged'];
      log.info(`shielded-balance contract deployed at ${contractAddress}`);

      holdCall = await buildCall('holdShielded', 'hold_shielded');
      holdDecoded = await toolkit.showTransaction(holdCall);
    }, CONTRACT_ACTION_TIMEOUT);

    /** Fail the dependent tests with a message that points at the real cause. */
    function requireHold(): ToolkitTransactionResult {
      expect(hold, 'holdShielded was never indexed, see the earlier test').toBeDefined();
      return hold!;
    }

    /** The contract action the indexer reports for this contract at a given transaction. */
    async function contractActionAt(txHash: string): Promise<ContractAction> {
      const response = await retry(
        () =>
          new IndexerHttpClient().getContractAction(contractAddress, {
            transactionOffset: { hash: txHash },
          }),
        (r) => r.data?.contractAction != null,
        30,
        2000,
      );
      expect(response).toBeSuccess();
      return response.data!.contractAction;
    }

    test(
      'the holdShielded call should declare a shielded token in its unshielded inputs',
      async () => {
        // Checked with the toolkit itself, not the indexer, so the fixture can never
        // silently stop producing the condition under test.
        log.debug(`holdShielded as decoded by the toolkit:\n${holdDecoded}`);
        expect(holdDecoded).toMatch(/unshielded_inputs:\s*\{\s*Shielded\(\s*ShieldedTokenType\(/);
      },
      TEST_TIMEOUT,
    );

    test(
      'the indexer should index the block that holds the holdShielded call',
      async () => {
        let sendError: unknown;
        try {
          hold = await toolkit.sendCustomContractCall(holdCall);
        } catch (error) {
          sendError = error;
        }
        expect(
          sendError,
          'the node took holdShielded, but the indexer never indexed it; the chain-indexer ' +
            'has most likely stopped at that block',
        ).toBeUndefined();
        log.info(`holdShielded included in block ${hold!.blockHash} (tx ${hold!.txHash})`);

        const response = await getBlockByHashWithRetry(hold!.blockHash);
        expect(response).toBeSuccess();
        expect(response.data?.block?.hash).toBeDefined();
      },
      CONTRACT_ACTION_TIMEOUT,
    );

    test(
      'the indexer should keep indexing past that block',
      async () => {
        const holdResult = requireHold();
        // Built only now that holdShielded is in a block: building both up front
        // makes them spend the same DUST fee input and the node rejects the second.
        ping = await toolkit.sendCustomContractCall(await buildCall('ping', 'ping_after_hold'));
        log.info(`ping included in block ${ping.blockHash} (tx ${ping.txHash})`);

        const holdBlock = await getBlockByHashWithRetry(holdResult.blockHash);
        const pingTx = await getTransactionByHashWithRetry(ping.txHash);
        expect(pingTx).toBeSuccess();
        expect(pingTx.data?.transactions?.length, `ping tx ${ping.txHash} was never indexed`).toBe(
          1,
        );
        const pingHeight = pingTx.data!.transactions![0].block!.height;
        expect(pingHeight).toBeGreaterThan(holdBlock.data!.block!.height);
      },
      CONTRACT_ACTION_TIMEOUT,
    );

    test(
      'the holdShielded contract action should report no unshielded balance',
      async () => {
        const action = await contractActionAt(requireHold().txHash);
        expect(action.__typename).toBe('ContractCall');
        // The contract holds only the shielded token, and unshieldedBalances is
        // documented to report unshielded tokens only.
        expect(action.unshieldedBalances).toEqual([]);
      },
      TEST_TIMEOUT,
    );

    test(
      'the later ping contract action should also report no unshielded balance',
      async () => {
        requireHold();
        expect(ping, 'ping was never sent, see the earlier test').toBeDefined();
        const action = await contractActionAt(ping!.txHash);
        expect(action.__typename).toBe('ContractCall');
        expect(action.unshieldedBalances).toEqual([]);
      },
      TEST_TIMEOUT,
    );
  });
