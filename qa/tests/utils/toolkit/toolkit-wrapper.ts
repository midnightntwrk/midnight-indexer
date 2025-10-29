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

import fs from 'fs';
import { join, resolve } from 'path';
import { retry } from '../retry-helper';
import log from '@utils/logging/logger';
import { env } from '../../environment/model';
import { GenericContainer, StartedTestContainer } from 'testcontainers';
import { getContractDeploymentHashes } from '../../tests/e2e/test-utils';

export type AddressType = 'shielded' | 'unshielded';

export type ShowAddressOption =
  | 'shielded'
  | 'unshielded'
  | 'coin-public'
  | 'coin-public-tagged'
  | 'unshielded-user-address-untagged';

interface AddressInfo {
  shielded: string;
  unshielded: string;
  coinPublic: string;
  coinPublicTagged: string;
  unshieldedUserAddressUntagged: string;
}

interface ToolkitConfig {
  containerName?: string;
  targetDir?: string;
  chain?: string;
  nodeTag?: string;
  syncCacheDir?: string;
  coinSeed?: string;
  warmupCache?: boolean;
}

export interface ToolkitTransactionResult {
  txHash: string;
  blockHash: string;
  status: 'sent' | 'confirmed';
  rawOutput: string;
}

interface LogEntry {
  level: string;
  message: string;
  target: string;
  timestamp: number;
  midnight_tx_hash?: string;
  block_hash?: string;
}

export interface DeployContractResult {
  'contract-address-untagged': string;
  'contract-address-tagged': string;
  'coin-public': string;
  'deploy-tx-hash': string;
  'deploy-block-hash': string;
}

class ToolkitWrapper {
  private container: GenericContainer;
  private startedContainer?: StartedTestContainer;
  private config: ToolkitConfig;

  private parseTransactionOutput(output: string): ToolkitTransactionResult {
    const lines = output.trim().split('\n');
    const jsonLines = lines.filter((line) => line.trim().startsWith('{'));

    let txHash = '';
    let blockHash: string | undefined;
    let status: 'sent' | 'confirmed' = 'sent';

    // Parse the JSON log entries
    for (const line of jsonLines) {
      try {
        const logEntry: LogEntry = JSON.parse(line);

        if (logEntry.midnight_tx_hash) {
          txHash = logEntry.midnight_tx_hash;
        }

        if (logEntry.block_hash) {
          blockHash = logEntry.block_hash;
          status = 'confirmed';
        }
      } catch (error) {
        continue;
      }
    }

    if (!txHash) {
      throw new Error('Could not extract transaction hash from toolkit output');
    }

    // Remove 0x prefix if present to match indexer API format (which doesn't use 0x prefix)
    const removeHexPrefix = (hash: string) => (hash.startsWith('0x') ? hash.slice(2) : hash);

    return {
      txHash: removeHexPrefix(txHash),
      blockHash: blockHash ? removeHexPrefix(blockHash) : '',
      status,
      rawOutput: output,
    };
  }

  constructor(config: ToolkitConfig) {
    this.config = config;

    const randomId = Math.random().toString(36).slice(2, 12);

    const envName = env.getEnvName();

    this.config.containerName = config.containerName || `mn-toolkit-${envName}-${randomId}`;
    this.config.targetDir = config.targetDir || resolve('./.tmp/toolkit');
    this.config.nodeTag = config.nodeTag || env.getNodeVersion();
    this.config.warmupCache = config.warmupCache || false;

    // Ensure the target directory exists
    if (!fs.existsSync(this.config.targetDir)) {
      fs.mkdirSync(this.config.targetDir, { recursive: true });
      log.debug(`Created target directory: ${this.config.targetDir}`);
    }

    // This block is making sure that if we explicitly provide a target dir
    if (this.config.warmupCache) {
      this.config.syncCacheDir = `${this.config.targetDir}/.sync_cache-${envName}`;
    } else {
      this.config.syncCacheDir = `${this.config.targetDir}/.sync_cache-${envName}-${randomId}`;
      // copy the golden sync cache directory to the instance-specific cache
      const goldenCacheDir = `${this.config.targetDir}/.sync_cache-${envName}`;

      if (!fs.existsSync(goldenCacheDir)) {
        fs.mkdirSync(goldenCacheDir);
        log.warn(
          `Golden cache directory not found at: ${goldenCacheDir}\n` +
            `Please ensure the global setup has run to warm up the cache, or run with warmupCache: true first.`,
        );
      }

      fs.cpSync(goldenCacheDir, this.config.syncCacheDir, { recursive: true });
      log.debug(
        `Copied sync cache from golden cache to instance cache: ${this.config.syncCacheDir}`,
      );
    }

    log.debug(`Toolkit container name   : ${this.config.containerName}`);
    log.debug(`Toolkit target dir       : ${this.config.targetDir}`);
    log.debug(`Toolkit node/toolkit tag : ${this.config.nodeTag}`);
    log.debug(`Toolkit sync cache dir   : ${this.config.syncCacheDir}`);

    this.container = new GenericContainer(
      `ghcr.io/midnight-ntwrk/midnight-node-toolkit:${this.config.nodeTag}`,
    )
      .withName(this.config.containerName)
      .withNetworkMode('host') // equivalent to --network host
      .withEntrypoint([]) // equivalent to --entrypoint ""
      .withBindMounts([
        {
          source: this.config.targetDir,
          target: '/out',
        },
        {
          source: this.config.syncCacheDir,
          target: `/.sync_cache`,
        },
      ])
      .withCommand(['sleep', 'infinity']); // equivalent to sleep infinity
  }

  /**
   * Start the toolkit container
   * This method starts the Docker container with retry logic to handle transient failures.
   *
   * @returns A promise that resolves when the container has successfully started
   *
   * @throws Error if the container fails to start after the maximum number of retries
   */
  async start() {
    // Clean up output directory from previous runs (excluding sync cache)
    if (this.config.targetDir && fs.existsSync(this.config.targetDir)) {
      const files = fs.readdirSync(this.config.targetDir);
      for (const file of files) {
        if (!file.startsWith('.sync_cache')) {
          const filePath = join(this.config.targetDir, file);
          fs.rmSync(filePath, { recursive: true, force: true });
        }
      }
      log.debug(`Cleaned output directory: ${this.config.targetDir}`);
    }

    this.startedContainer = await retry(async () => this.container.start(), {
      maxRetries: 2,
      delayMs: 2_000,
      retryLabel: 'start toolkit container',
    });
  }

  /**
   * Stop the toolkit container and cleanup resources
   *
   * This method stops the running Docker container and removes the instance-specific sync cache
   * directory (unless warmupCache is enabled). Cleanup errors are logged as warnings but do not
   * throw exceptions.
   *
   * @returns A promise that resolves when the container has stopped and cleanup is complete
   */
  async stop() {
    if (this.startedContainer) {
      await this.startedContainer.stop();
    }

    // Cleanup instance-specific cache directory (not the golden cache)
    if (!this.config.warmupCache && this.config.syncCacheDir) {
      try {
        fs.rmSync(this.config.syncCacheDir, { recursive: true, force: true });
        log.debug(`Cleaned up instance-specific sync cache: ${this.config.syncCacheDir}`);
      } catch (error) {
        log.warn(`Failed to cleanup sync cache: ${error}`);
      }
    }
  }

  /**
   * Warm up the cache by generating a single unshielded transaction
   * This method displays sync progress to the console during warmup.
   *
   * @returns void
   */
  async warmupCache() {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    // We use generate single tx to warm up the cache because it will try to sync the cache
    // before it gets to validate the arguments that are explicitly wrong.
    try {
      await this.generateSingleTx(
        '0'.repeat(64), // Invalid seed
        'unshielded',
        (await this.showAddress('0'.repeat(63) + '9')).unshielded,
        1,
      );
    } catch (error) {
      // Do nothing as we are actually expecting an error
    }
  }

  /**
   * Show address information from a seed
   *
   * @param seed - The seed to use
   * @param networkId - The network ID to use (default: current target environment)
   *
   * @returns The address information as a JSON object
   */
  async showAddress(seed: string, networkId?: string): Promise<AddressInfo> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    const response = await this.startedContainer.exec([
      '/midnight-node-toolkit',
      'show-address',
      '--network',
      networkId ?? env.getNetworkId().toLowerCase(),
      '--seed',
      seed,
    ]);

    if (response.exitCode !== 0) {
      const errorMessage = response.stderr || response.output || 'Unknown error occurred';
      throw new Error(
        `Toolkit command failed with exit code ${response.exitCode}: ${errorMessage}`,
      );
    }

    return JSON.parse(response.output);
  }

  /**
   * Show viewing key information from a seed
   *
   * @param seed - The seed to use
   * @param networkId - The network ID to use (default: current target environment)
   *
   * @returns The viewing key as a string
   */
  async showViewingKey(seed: string, networkId?: string): Promise<string> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    const result = await this.startedContainer.exec([
      '/midnight-node-toolkit',
      'show-viewing-key',
      '--network',
      networkId ?? env.getNetworkId().toLowerCase(),
      '--seed',
      seed,
    ]);

    if (result.exitCode !== 0) {
      const errorMessage = result.stderr || result.output || 'Unknown error occurred';
      throw new Error(`Toolkit command failed with exit code ${result.exitCode}: ${errorMessage}`);
    }

    return result.output.trim();
  }

  /**
   * Generate and submit a single shielded or unshielded transaction
   *
   * @param sourceSeed - The source seed to use
   * @param addressType - The address type to use
   * @param destinationAddress - The destination address to use
   * @param amount - The amount to use
   *
   * @returns The transaction result
   */
  async generateSingleTx(
    sourceSeed: string,
    addressType: AddressType,
    destinationAddress: string,
    amount: number,
  ): Promise<ToolkitTransactionResult> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    const result = await this.startedContainer.exec([
      '/midnight-node-toolkit',
      'generate-txs',
      '--src-url',
      env.getNodeWebsocketBaseURL(),
      '--dest-url',
      env.getNodeWebsocketBaseURL(),
      'single-tx',
      '--source-seed',
      sourceSeed,
      '--destination-address',
      destinationAddress,
      `--${addressType}-amount`,
      amount.toString(),
    ]);

    log.debug(`Generate single transaction output:\n${result.output}`);

    if (result.exitCode !== 0) {
      const errorMessage = result.stderr || result.output || 'Unknown error occurred';
      throw new Error(`Toolkit command failed with exit code ${result.exitCode}: ${errorMessage}`);
    }

    const rawOutput = result.output.trim();
    return this.parseTransactionOutput(rawOutput);
  }

  /**
   * Extract the contract address from a deployed transaction file.
   * This method uses the toolkit's contract-address command to retrieve either a tagged
   * or untagged contract address.
   *
   * @param contractFile - The name of the contract transaction file (e.g., 'deploy_tx.mn')
   *                       located in the toolkit's output directory (/out/).
   * @param tagType - The format of the address to retrieve: 'tagged' includes the prefix,
   *                  'untagged' returns only the hex address.
   * @returns A promise that resolves to the contract address string in the requested format.
   * @throws Error if the container is not started or if the contract-address command fails.
   */
  async getContractAddress(contractFile: string, tagType: 'tagged' | 'untagged'): Promise<string> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }
    const addressResult = await this.startedContainer.exec([
      '/midnight-node-toolkit',
      'contract-address',
      ...(tagType === 'tagged' ? ['--tagged'] : []),
      '--src-file',
      `/out/${contractFile}`,
    ]);
    log.debug(`contract-address taggedAddress:\n${JSON.stringify(addressResult, null, 2)}`);
    if (addressResult.exitCode !== 0) {
      const e = addressResult.stderr || addressResult.output || 'Unknown error';
      throw new Error(`contract-address failed: ${e}`);
    }

    return addressResult.output.trim();
  }

  /**
   * Call a smart contract function by generating and submitting a circuit transaction.
   * This method retrieves the current contract state, generates a circuit intent for the specified
   * contract call, converts it to a transaction, and submits it to the network.
   *
   * @param callKey - The contract function to call (e.g., 'increment'). Defaults to 'increment'.
   * @param deploymentResult - The deployment result object from deployContract. The contract-address-untagged will be extracted.
   * @param rngSeed - The random number generator seed for the transaction. Defaults to a fixed seed.
   * @returns A promise that resolves to the transaction result containing the transaction hash,
   *          optional block hash, and submission status.
   * @throws Error if the container is not started or if any step in the contract call process fails.
   */
  async callContract(
    callKey: string = 'increment',
    deploymentResult: DeployContractResult,
    rngSeed: string = '0000000000000000000000000000000000000000000000000000000000000037',
  ): Promise<ToolkitTransactionResult> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    // Validate deployment result
    if (!deploymentResult) {
      log.error('No deployment result provided. Cannot call contract without a valid deployment.');
      throw new Error(
        'Deployment result is required but was not provided. Ensure deployContract() succeeded before calling callContract().',
      );
    }

    const contractAddressUntagged = deploymentResult['contract-address-untagged'];

    if (!contractAddressUntagged) {
      log.error('Deployment result is missing contract address. Deployment may have failed.');
      log.debug(`Deployment result received: ${JSON.stringify(deploymentResult, null, 2)}`);
      throw new Error(
        'Contract address is missing in deployment result. The contract deployment may have failed. ' +
          'Please check deployment logs and ensure deployContract() completed successfully.',
      );
    }

    const txFile = `/out/${callKey}_tx.mn`;

    log.info(`Generating ${callKey} contract call...`);
    const result = await this.startedContainer.exec([
      '/midnight-node-toolkit',
      'generate-txs',
      '--dest-file',
      txFile,
      '--to-bytes',
      'contract-simple',
      'call',
      '--call-key',
      callKey,
      '--rng-seed',
      rngSeed,
      '--contract-address',
      contractAddressUntagged,
    ]);

    if (result.exitCode !== 0) {
      const errorMessage = result.stderr || result.output || 'Unknown error occurred';
      throw new Error(`Failed to generate contract call: ${errorMessage}`);
    }

    log.info('Submitting transaction to network...');
    const sendResult = await this.startedContainer.exec([
      '/midnight-node-toolkit',
      'generate-txs',
      '--src-file',
      txFile,
      '--dest-url',
      env.getNodeWebsocketBaseURL(),
      'send',
    ]);

    if (sendResult.exitCode !== 0) {
      const errorMessage = sendResult.stderr || sendResult.output || 'Unknown error occurred';
      throw new Error(`Failed to submit transaction: ${errorMessage}`);
    }

    const rawOutput = sendResult.output.trim();
    return this.parseTransactionOutput(rawOutput);
  }

  /**
   * Deploy a smart contract to the network.
   * This method generates a deployment intent, converts it to a transaction, submits it to the network,
   * and retrieves both tagged and untagged contract addresses.
   *
   * @returns A promise that resolves to the deployment result containing untagged address, tagged address, and coin public key.
   * @throws Error if the container is not started or if any step in the deployment process fails.
   */
  async deployContract(): Promise<DeployContractResult> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    const outDir = this.config.targetDir!;

    const deployTx = 'deploy_tx.mn';

    const outDeployTx = join(outDir, deployTx);
    const coinPublicSeed = '0000000000000000000000000000000000000000000000000000000000000001';
    const addressInfo = await this.showAddress(coinPublicSeed);
    const coinPublic = addressInfo.coinPublic;

    {
      const result = await this.startedContainer.exec([
        '/midnight-node-toolkit',
        'generate-txs',
        '--dest-file',
        `/out/${deployTx}`,
        '--to-bytes',
        'contract-simple',
        'deploy',
        '--rng-seed',
        '0000000000000000000000000000000000000000000000000000000000000037',
      ]);

      log.debug(`contract-simple deploy command output:\n${result.output}`);
      log.debug(`contract-simple deploy command stderr:\n${result.stderr}`);
      log.debug(`contract-simple deploy exit code: ${result.exitCode}`);

      if (result.exitCode !== 0) {
        const e = result.stderr || result.output || 'Unknown error';
        throw new Error(`contract-simple deploy failed: ${e}`);
      }

      log.debug(`Checking for output files:`);
      log.debug(`  ${outDeployTx} exists: ${fs.existsSync(outDeployTx)}`);

      if (!fs.existsSync(outDeployTx)) {
        throw new Error('contract-simple deploy did not produce expected output file');
      }
    }

    {
      const result = await this.startedContainer.exec([
        '/midnight-node-toolkit',
        'generate-txs',
        '--src-file',
        `/out/${deployTx}`,
        '--dest-url',
        env.getNodeWebsocketBaseURL(),
        'send',
      ]);
      if (result.exitCode !== 0) {
        const e = result.stderr || result.output || 'Unknown error';
        throw new Error(`generate-txs send failed: ${e}`);
      }
    }

    const contractAddressTagged = await this.getContractAddress(deployTx, 'tagged');
    const contractAddressUntagged = await this.getContractAddress(deployTx, 'untagged');
    const { txHash, blockHash } = await getContractDeploymentHashes(contractAddressUntagged);

    const deploymentResult = {
      'contract-address-untagged': contractAddressUntagged,
      'contract-address-tagged': contractAddressTagged,
      'coin-public': coinPublic,
      'deploy-tx-hash': txHash,
      'deploy-block-hash': blockHash,
    };

    log.debug(`Contract address info:\n${JSON.stringify(deploymentResult, null, 2)}`);

    return deploymentResult;
  }
}

export { ToolkitWrapper, ToolkitConfig };
