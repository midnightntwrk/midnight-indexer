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
import {
  awaitIndexedTx,
  getBlockByHashWithRetry,
  getTransactionByHashWithRetry,
  retry,
} from './test-utils';
import {
  ToolkitWrapper,
  type CustomContractCall,
  type CustomContractSpec,
  type DeployContractResult,
  type ToolkitTransactionResult,
} from '@utils/toolkit/toolkit-wrapper';
import { closeMothWallets } from '@utils/moth/moth-backend';
import {
  callCircuitViaMoth,
  closeMothContractContexts,
  deployContractViaMoth,
  describeUnprovenCallViaMoth,
} from '@utils/moth/moth-contracts';
import { mothHandlesContracts } from '@utils/moth/ledger';

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

/**
 * What the suite needs from a transaction backend. Two calls are made, each
 * built only after the previous one is in a block: building two up front makes
 * them spend the same DUST fee input and the node rejects the second one.
 */
interface ShieldedBalanceBackend {
  /** Deploy the contract and return its untagged address. */
  deploy(): Promise<string>;
  /**
   * Build the `holdShielded` call against current state and return the
   * transaction as decoded by the backend itself, not the indexer.
   */
  describeHold(): Promise<string>;
  /** Send a call and return once its transaction and block are known. */
  send(circuitId: 'holdShielded' | 'ping'): Promise<ToolkitTransactionResult>;
  stop(): Promise<void>;
}

/**
 * The toolkit builds a call from a snapshot of the contract state and sends it
 * as a separate step, so `holdShielded` is sent exactly as it was decoded.
 */
async function toolkitBackend(
  compiledDir: string,
  fundingSeed: string,
): Promise<ShieldedBalanceBackend> {
  const toolkit = new ToolkitWrapper({
    customContractDir: compiledDir,
    compactcVersion: COMPACT_COMPILER_VERSION,
  });
  await toolkit.start();
  const requiredRuntime = compiledRuntimeVersion(compiledDir);
  if (requiredRuntime) {
    await toolkit.assertCompactRuntimeSupported(requiredRuntime);
  }

  let deployment: DeployContractResult;
  let holdCall: CustomContractCall | undefined;

  const buildCall = async (circuitId: string, label: string): Promise<CustomContractCall> => {
    const contractAddress = deployment['contract-address-untagged'];
    const stateFile = await toolkit.snapshotContractState(contractAddress, `${label}_state.bin`);
    return toolkit.generateCustomContractCall({
      circuitId,
      deploymentResult: deployment,
      contract: SHIELDED_BALANCE,
      onchainStateFile: stateFile,
      label,
      fundingSeed,
    });
  };

  return {
    deploy: async () => {
      deployment = await toolkit.deployCustomContract(SHIELDED_BALANCE, [], fundingSeed);
      return deployment['contract-address-untagged'];
    },
    describeHold: async () => {
      holdCall = await buildCall('holdShielded', 'hold_shielded');
      return toolkit.showTransaction(holdCall);
    },
    send: async (circuitId) =>
      toolkit.sendCustomContractCall(
        circuitId === 'holdShielded' ? holdCall! : await buildCall('ping', 'ping_after_hold'),
      ),
    stop: () => toolkit.stop(),
  };
}

/**
 * moth builds every call through midnight-js against the contract's live state.
 * `describeHold` builds an unproven `holdShielded` and prints it with the
 * ledger; `send` builds, proves and submits a fresh one, which carries the same
 * effects because the circuit reads no state that changes in between.
 */
function mothBackend(compiledDir: string, fundingSeed: string): ShieldedBalanceBackend {
  const artifactDir = path.join(compiledDir, 'managed');
  let contractAddress: string;
  return {
    deploy: async () => {
      contractAddress = (await deployContractViaMoth(fundingSeed, artifactDir)).contractAddress;
      return contractAddress;
    },
    describeHold: () =>
      describeUnprovenCallViaMoth(fundingSeed, contractAddress, 'holdShielded', artifactDir),
    send: async (circuitId) =>
      awaitIndexedTx(
        await callCircuitViaMoth(fundingSeed, contractAddress, circuitId, artifactDir),
        `the ${circuitId} call`,
      ),
    stop: async () => {
      closeMothContractContexts();
      await closeMothWallets();
    },
  };
}

const isMoth = env.getTxBackend() === 'moth';

// Undeployed only under the toolkit: it rebuilds ledger state from genesis on every call, which
// on a long-lived deployed chain takes hours per step (see Env gating in the test conventions).
// Under TX_BACKEND=moth the calls go through midnight-js on a moth-synced wallet instead, no
// toolkit is started, and the suite runs on deployed envs too.
describe
  .skipIf(!env.isUndeployedEnv() && !isMoth)
  .sequential('a contract whose balance map holds a shielded token', () => {
    let backend: ShieldedBalanceBackend | undefined;
    let contractAddress: string;
    let holdDecoded: string;
    let hold: ToolkitTransactionResult | undefined;
    let ping: ToolkitTransactionResult | undefined;

    beforeAll(async () => {
      const fundingSeed = dataProvider.getFundingSeed();

      const compiledDir = await compileCompactContract({
        sourceDir: SHIELDED_BALANCE_DIR,
        sourceFile: SHIELDED_BALANCE_SOURCE,
        stage: [SHIELDED_BALANCE.configFile],
      });
      log.info(
        `shielded-balance compiled by compactc ${COMPACT_COMPILER_VERSION} ` +
          `for compact-runtime ${compiledRuntimeVersion(compiledDir) ?? 'unknown'}`,
      );

      // moth's contract path is midnight-js 5, whose current era is ledger 9. On a ledger-8
      // chain the toolkit builds the calls, as ToolkitWrapper does for the other contract
      // suites — which is only viable on undeployed, for the reason above.
      const viaMoth = isMoth && (await mothHandlesContracts());
      if (!viaMoth && !env.isUndeployedEnv()) {
        throw new Error(
          'TX_BACKEND=moth cannot build contract calls on this ledger-8 chain, and the toolkit ' +
            'is not usable on a deployed env',
        );
      }
      backend = viaMoth
        ? mothBackend(compiledDir, fundingSeed)
        : await toolkitBackend(compiledDir, fundingSeed);
    }, SETUP_TIMEOUT);

    // The backend only exists once the compile (and, for the toolkit, the image) succeeded;
    // stopping an unassigned one here would bury the failure that stopped setup.
    afterAll(async () => {
      await backend?.stop();
    });

    /**
     * Deploy and build the `holdShielded` call, but do not send it yet. Sending is
     * part of the tests below: both backends learn a call's block from the indexer,
     * so on an indexer that stops at that block the send itself never completes.
     * Doing it inside a test turns that into a clear failure instead of a setup
     * error that skips everything.
     */
    beforeAll(async () => {
      contractAddress = await backend!.deploy();
      log.info(`shielded-balance contract deployed at ${contractAddress}`);
      holdDecoded = await backend!.describeHold();
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
        // Checked with the backend itself, not the indexer, so the fixture can never
        // silently stop producing the condition under test.
        log.debug(`holdShielded as decoded by the backend:\n${holdDecoded}`);
        expect(holdDecoded).toMatch(/unshielded_inputs:\s*\{\s*Shielded\(\s*ShieldedTokenType\(/);
      },
      TEST_TIMEOUT,
    );

    test(
      'the indexer should index the block that holds the holdShielded call',
      async () => {
        let sendError: unknown;
        try {
          hold = await backend!.send('holdShielded');
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
        ping = await backend!.send('ping');
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
