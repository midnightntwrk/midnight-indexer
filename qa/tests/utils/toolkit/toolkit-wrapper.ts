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
import { execSync } from 'child_process';
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
  nodeToolkitTag?: string;
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
  private contractDir?: string;

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
      } catch (_error) {
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

    const envName = env.getCurrentEnvironmentName();

    this.config.containerName = config.containerName || `mn-toolkit-${envName}-${randomId}`;
    this.config.targetDir = config.targetDir || resolve('./.tmp/toolkit');
    this.config.nodeTag = config.nodeTag || env.getNodeVersion();
    this.config.nodeToolkitTag = config.nodeToolkitTag || env.getNodeToolkitVersion();
    this.config.warmupCache = config.warmupCache || false;

    // Ensure the target directory exists
    if (!fs.existsSync(this.config.targetDir)) {
      fs.mkdirSync(this.config.targetDir, { recursive: true });
    }

    // This block is making sure that if a golden cache directory is available, we use it.
    if (this.config.warmupCache) {
      log.debug('Warmup cache is enabled, using the golden cache directory');
      this.config.syncCacheDir = `${this.config.targetDir}/.sync_cache-${envName}`;

      // Check if there is any .bin file in the golden cache directory
      if (
        fs.existsSync(this.config.syncCacheDir) &&
        fs.readdirSync(this.config.syncCacheDir).some((file) => file.endsWith('.bin'))
      ) {
        console.debug(`[SETUP] Golden cache file found at: ${this.config.syncCacheDir}, using it`);
      } else {
        console.debug(`[SETUP] Golden cache directory not found at: ${this.config.syncCacheDir}`);
      }
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

    log.debug(`NODE_TAG         : ${this.config.nodeTag}`);
    log.debug(`NODE_TOOLKIT_TAG : ${this.config.nodeToolkitTag}`);
    log.debug(`Toolkit target dir     : ${this.config.targetDir}`);
    log.debug(`Toolkit container name : ${this.config.containerName}`);
    log.debug(`Toolkit sync cache dir : ${this.config.syncCacheDir}`);

    // Set up contract directory path
    this.contractDir = join(this.config.targetDir!, 'contract');

    // Prepare bind mounts
    const bindMounts = [
      {
        source: this.config.targetDir,
        target: '/out',
      },
      {
        source: this.config.syncCacheDir,
        target: `/.cache/sync`,
      },
    ];

    // Add contract directory mount if it exists (will be created in start())
    if (this.contractDir) {
      bindMounts.push({
        source: this.contractDir,
        target: '/toolkit-js/contract',
      });
    }

    this.container = new GenericContainer(
      `ghcr.io/midnight-ntwrk/midnight-node-toolkit:${this.config.nodeToolkitTag}`,
    )
      .withName(this.config.containerName)
      .withNetworkMode('host') // equivalent to --network host
      .withEntrypoint([]) // equivalent to --entrypoint ""
      .withBindMounts(bindMounts)
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

    // Copy contract directory from toolkit image
    if (this.contractDir) {
      log.debug('Copying contract directory from toolkit image...');
      try {
        const toolkitImage = `ghcr.io/midnight-ntwrk/midnight-node-toolkit:${this.config.nodeToolkitTag}`;

        // Create temporary container to copy from
        const tmpContainerId = execSync(`docker create ${toolkitImage}`, {
          encoding: 'utf-8',
        }).trim();

        try {
          // Copy contract directory
          execSync(`docker cp ${tmpContainerId}:/toolkit-js/test/contract ${this.contractDir}`, {
            encoding: 'utf-8',
            stdio: 'inherit',
          });
          log.debug(`Contract directory copied to: ${this.contractDir}`);
        } finally {
          // Clean up temporary container
          execSync(`docker rm -v ${tmpContainerId}`, { encoding: 'utf-8', stdio: 'ignore' });
        }
      } catch (error) {
        log.warn(
          `Failed to copy contract directory: ${error}. Intent-based deployment may not work.`,
        );
      }
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
    // before it gets to validate the arguments that are wrong on purpose.
    let output: ToolkitTransactionResult;
    try {
      output = await this.generateSingleTx(
        '0'.repeat(64), // Invalid seed
        'unshielded',
        (await this.showAddress('0'.repeat(63) + '9')).unshielded,
        1,
      );
      log.debug(`Warmup cache output:\n${JSON.stringify(output, null, 2)}`);
    } catch (_error) {
      log.debug(
        'Heads up, we are expecting an error here, the following log message is only reported for debugging purposes',
      );
      log.debug(`Warmup cache error: ${_error}`);
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
    if (addressResult.exitCode !== 0) {
      const e = addressResult.stderr || addressResult.output || 'Unknown error';
      throw new Error(`contract-address failed: ${e}`);
    }

    return addressResult.output.trim();
  }

  /**
   * Call a smart contract function using the intent-based approach.
   * This method:
   * 1. Gets the current contract state from the chain
   * 2. Generates a circuit intent for the specified contract call
   * 3. Converts the intent to a transaction
   * 4. Submits it to the network
   *
   * @param callKey - The contract function to call (e.g., 'increment'). Defaults to 'increment'.
   * @param deploymentResult - The deployment result object from deployContract. The contract-address-untagged and coin-public will be extracted.
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
    const coinPublic = deploymentResult['coin-public'];

    if (!contractAddressUntagged) {
      throw new Error(
        'Contract address is missing in deployment result. The contract deployment may have failed. ' +
          'Please check deployment logs and ensure deployContract() completed successfully.',
      );
    }

    if (!coinPublic) {
      throw new Error('Coin public key is missing in deployment result.');
    }

    const callTx = `${callKey}_tx.mn`;
    const callIntent = `${callKey}_intent.bin`;
    const contractStateFile = 'contract_state.mn';
    const callPrivateState = `${callKey}_ps_state.json`;
    const callZswapState = `${callKey}_zswap_state.json`;

    // Step 1: Get contract state from chain
    log.info('Getting contract state from chain...');
    const stateResult = await this.startedContainer.exec([
      '/midnight-node-toolkit',
      'contract-state',
      '--src-url',
      env.getNodeWebsocketBaseURL(),
      '--contract-address',
      contractAddressUntagged,
      '--dest-file',
      `/out/${contractStateFile}`,
    ]);

    if (stateResult.exitCode !== 0) {
      const e = stateResult.stderr || stateResult.output || 'Unknown error';
      throw new Error(`contract-state failed: ${e}`);
    }

    // Step 2: Generate call intent
    log.info(`Generating call intent for ${callKey} entrypoint...`);
    const intentResult = await this.startedContainer.exec([
      '/midnight-node-toolkit',
      'generate-intent',
      'circuit',
      '-c',
      '/toolkit-js/contract/contract.config.ts',
      '--coin-public',
      coinPublic,
      '--input-onchain-state',
      `/out/${contractStateFile}`,
      '--input-private-state',
      '/out/initial_state.json',
      '--contract-address',
      contractAddressUntagged,
      '--output-intent',
      `/out/${callIntent}`,
      '--output-private-state',
      `/out/${callPrivateState}`,
      '--output-zswap-state',
      `/out/${callZswapState}`,
      callKey,
    ]);

    if (intentResult.exitCode !== 0) {
      const e = intentResult.stderr || intentResult.output || 'Unknown error';
      throw new Error(`generate-intent circuit failed: ${e}`);
    }

    // Verify intent file was created
    const checkIntentResult = await this.startedContainer.exec([
      'sh',
      '-c',
      `test -f /out/${callIntent} && echo "EXISTS" || echo "MISSING"`,
    ]);
    if (!checkIntentResult.output.includes('EXISTS')) {
      throw new Error(`Intent file not found at /out/${callIntent}`);
    }

    // Step 3: Convert intent to transaction
    log.info('Converting call intent to transaction...');
    const sendIntentResult = await this.startedContainer.exec([
      '/midnight-node-toolkit',
      'send-intent',
      '--intent-file',
      `/out/${callIntent}`,
      '--compiled-contract-dir',
      '/toolkit-js/contract/managed/counter',
      '--to-bytes',
      '--dest-file',
      `/out/${callTx}`,
    ]);

    if (sendIntentResult.exitCode !== 0) {
      const e = sendIntentResult.stderr || sendIntentResult.output || 'Unknown error';
      throw new Error(`send-intent failed: ${e}`);
    }

    log.info('Sending call transaction to node...');
    const sendResult = await this.startedContainer.exec([
      '/midnight-node-toolkit',
      'generate-txs',
      '--src-file',
      `/out/${callTx}`,
      '--dest-url',
      env.getNodeWebsocketBaseURL(),
      '-r',
      '1',
      'send',
    ]);

    if (sendResult.exitCode !== 0) {
      const e = sendResult.stderr || sendResult.output || 'Unknown error';
      throw new Error(`generate-txs send failed: ${e}`);
    }

    const rawOutput = sendResult.output.trim();
    return this.parseTransactionOutput(rawOutput);
  }

  /**
   * Deploy a smart contract to the network using the intent-based approach.
   * This method generates a deployment intent, converts it to a transaction, submits it to the network,
   * and retrieves both tagged and untagged contract addresses.
   *
   * @returns A promise that resolves to the deployment result containing untagged address, tagged address, coin public key, and transaction hashes.
   * @throws Error if the container is not started or if any step in the deployment process fails.
   */
  async deployContract(): Promise<DeployContractResult> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    const outDir = this.config.targetDir!;
    const deployTx = 'deploy_tx.mn';
    const deployIntent = 'deploy.bin';

    const coinPublicSeed = '0000000000000000000000000000000000000000000000000000000000000001';
    const addressInfo = await this.showAddress(coinPublicSeed);
    const coinPublic = addressInfo.coinPublic;

    // Use intent-based deployment approach
    log.info('Generating deploy intent...');
    const intentResult = await this.startedContainer.exec([
      '/midnight-node-toolkit',
      'generate-intent',
      'deploy',
      '-c',
      '/toolkit-js/contract/contract.config.ts',
      '--coin-public',
      coinPublic,
      '--authority-seed',
      coinPublicSeed,
      '--output-intent',
      `/out/${deployIntent}`,
      '--output-private-state',
      '/out/initial_state.json',
      '--output-zswap-state',
      '/out/temp.json',
      '20',
    ]);

    if (intentResult.exitCode !== 0) {
      const e = intentResult.stderr || intentResult.output || 'Unknown error';
      throw new Error(`generate-intent deploy failed: ${e}`);
    }

    // Verify intent file was created
    const checkIntentResult = await this.startedContainer.exec([
      'sh',
      '-c',
      `test -f /out/${deployIntent} && echo "EXISTS" || echo "MISSING"`,
    ]);
    if (!checkIntentResult.output.includes('EXISTS')) {
      throw new Error(`Intent file not found at /out/${deployIntent}`);
    }

    // Convert intent to transaction
    log.info('Converting intent to transaction...');
    const sendIntentResult = await this.startedContainer.exec([
      '/midnight-node-toolkit',
      'send-intent',
      '--intent-file',
      `/out/${deployIntent}`,
      '--compiled-contract-dir',
      'contract/managed/counter',
      '--to-bytes',
      '--dest-file',
      `/out/${deployTx}`,
    ]);

    if (sendIntentResult.exitCode !== 0) {
      const e = sendIntentResult.stderr || sendIntentResult.output || 'Unknown error';
      throw new Error(`send-intent failed: ${e}`);
    }

    const outDeployTx = join(outDir, deployTx);
    if (!fs.existsSync(outDeployTx)) {
      throw new Error('send-intent did not produce expected output file');
    }

    log.info('Sending deployment transaction to node...');
    const sendResult = await this.startedContainer.exec([
      '/midnight-node-toolkit',
      'generate-txs',
      '--src-file',
      `/out/${deployTx}`,
      '--dest-url',
      env.getNodeWebsocketBaseURL(),
      '-r',
      '1',
      'send',
    ]);

    if (sendResult.exitCode !== 0) {
      const e = sendResult.stderr || sendResult.output || 'Unknown error';
      throw new Error(`generate-txs send failed: ${e}`);
    }

    // Get contract address first (needed for result)
    const contractAddressTagged = await this.getContractAddress(deployTx, 'tagged');
    const contractAddressUntagged = await this.getContractAddress(deployTx, 'untagged');

    // Extract transaction hash and block hash from output
    const sendOutput = sendResult.output.trim();
    const txHashMatch = sendOutput.match(/"midnight_tx_hash":"(0x[^"]+)"/);
    const blockHashMatch = sendOutput.match(/"block_hash":"(0x[^"]+)"/);

    let txHash = '';
    let blockHash = '';

    if (txHashMatch) {
      txHash = txHashMatch[1].replace(/^0x/, ''); // Remove 0x prefix to match indexer format
    }
    if (blockHashMatch) {
      blockHash = blockHashMatch[1].replace(/^0x/, ''); // Remove 0x prefix to match indexer format
    }

    if (!txHash || !blockHash) {
      log.warn(
        `Could not extract transaction/block hash from send output. They will be available from indexer queries later.`,
      );
    }

    return {
      'contract-address-untagged': contractAddressUntagged,
      'contract-address-tagged': contractAddressTagged,
      'coin-public': coinPublic,
      'deploy-tx-hash': txHash,
      'deploy-block-hash': blockHash,
    };
  }

  /**
   * Switch the maintenance authority for a contract.
   * This is an optional step that can fail if the contract is not ready or authority seeds are incorrect.
   *
   * @param contractAddress - The untagged contract address
   * @param authoritySeed - The current authority seed
   * @param newAuthoritySeed - The new authority seed to switch to
   * @param fundingSeed - The seed to use for funding the transaction (defaults to authoritySeed)
   * @param counter - The counter value for the transaction (default: 0)
   * @param rngSeed - The random number generator seed (default: fixed seed)
   * @returns A promise that resolves to the transaction result, or null if the switch failed
   * @throws Error if the container is not started
   */
  async switchMaintenanceAuthority(
    contractAddress: string,
    authoritySeed: string = '0000000000000000000000000000000000000000000000000000000000000001',
    newAuthoritySeed: string = '1000000000000000000000000000000000000000000000000000000000000001',
    fundingSeed?: string,
    counter: number = 0,
    rngSeed: string = '0000000000000000000000000000000000000000000000000000000000000001',
  ): Promise<ToolkitTransactionResult | null> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    const fundingSeedToUse = fundingSeed ?? authoritySeed;
    const txFile = '/out/authority_switch_tx.mn';

    log.info('Generating maintenance authority switch transaction...');
    const result = await this.startedContainer.exec([
      '/midnight-node-toolkit',
      'generate-txs',
      '--src-url',
      env.getNodeWebsocketBaseURL(),
      'contract-simple',
      'maintenance',
      '--funding-seed',
      fundingSeedToUse,
      '--authority-seed',
      authoritySeed,
      '--new-authority-seed',
      newAuthoritySeed,
      '--counter',
      counter.toString(),
      '--rng-seed',
      rngSeed,
      '--contract-address',
      contractAddress,
      '--to-bytes',
      '--dest-file',
      txFile,
    ]);

    if (result.exitCode !== 0) {
      log.warn(
        `Failed to generate authority switch transaction: ${result.stderr || result.output}`,
      );
      log.warn('Continuing without authority switch - will use default authority for maintenance');
      return null;
    }

    log.info('Submitting authority switch transaction to network...');
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
      log.warn(
        `Failed to submit authority switch transaction: ${sendResult.stderr || sendResult.output}`,
      );
      return null;
    }

    const rawOutput = sendResult.output.trim();
    return this.parseTransactionOutput(rawOutput);
  }

  /**
   * Perform contract maintenance by removing and/or upserting entrypoints.
   * This method generates a maintenance transaction and submits it to the network.
   *
   * @param contractAddress - The untagged contract address
   * @param options - Maintenance options
   * @param options.authoritySeed - The authority seed to use (default: default seed)
   * @param options.fundingSeed - The seed to use for funding (defaults to authoritySeed)
   * @param options.counter - The counter value for the transaction (default: 0)
   * @param options.rngSeed - The random number generator seed (default: fixed seed)
   * @param options.removeEntrypoints - Array of entrypoint names to remove (e.g., ['decrement'])
   * @param options.upsertEntrypoints - Array of paths to verifier files to upsert (e.g., ['/toolkit-js/contract/managed/counter/keys/increment.verifier'])
   * @returns A promise that resolves to the transaction result
   * @throws Error if the container is not started or if maintenance transaction generation fails
   */
  async maintainContract(
    contractAddress: string,
    options: {
      authoritySeed?: string;
      fundingSeed?: string;
      counter?: number;
      rngSeed?: string;
      removeEntrypoints?: string[];
      upsertEntrypoints?: string[];
    } = {},
  ): Promise<ToolkitTransactionResult> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    const {
      authoritySeed = '0000000000000000000000000000000000000000000000000000000000000001',
      fundingSeed,
      counter = 0,
      rngSeed = '0000000000000000000000000000000000000000000000000000000000000001',
      removeEntrypoints = [],
      upsertEntrypoints = [],
    } = options;

    const fundingSeedToUse = fundingSeed ?? authoritySeed;
    const txFile = '/out/maintenance_tx.mn';

    // Build the command arguments
    const commandArgs: string[] = [
      '/midnight-node-toolkit',
      'generate-txs',
      '--src-url',
      env.getNodeWebsocketBaseURL(),
      'contract-simple',
      'maintenance',
      '--funding-seed',
      fundingSeedToUse,
      '--authority-seed',
      authoritySeed,
      '--counter',
      counter.toString(),
      '--rng-seed',
      rngSeed,
      '--contract-address',
      contractAddress,
    ];

    // Add remove-entrypoint flags
    for (const entrypoint of removeEntrypoints) {
      commandArgs.push('--remove-entrypoint', entrypoint);
    }

    // Add upsert-entrypoint flags
    for (const verifierPath of upsertEntrypoints) {
      commandArgs.push('--upsert-entrypoint', verifierPath);
    }

    commandArgs.push('--to-bytes', '--dest-file', txFile);

    log.info('Generating contract maintenance transaction...');
    const result = await this.startedContainer.exec(commandArgs);

    if (result.exitCode !== 0) {
      const errorMessage = result.stderr || result.output || 'Unknown error occurred';
      throw new Error(`Failed to generate maintenance transaction: ${errorMessage}`);
    }

    log.info('Submitting maintenance transaction to network...');
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
      throw new Error(`Failed to submit maintenance transaction: ${errorMessage}`);
    }

    const rawOutput = sendResult.output.trim();
    return this.parseTransactionOutput(rawOutput);
  }

  /**
   * Prepare contract maintenance by copying verifier files.
   * This is a helper method to prepare the contract directory for maintenance operations.
   *
   * @param sourceVerifier - The source verifier file path (relative to contract directory)
   * @param targetVerifier - The target verifier file path (relative to contract directory)
   * @throws Error if the source verifier file doesn't exist
   */
  private prepareVerifierFile(sourceVerifier: string, targetVerifier: string): void {
    if (!this.contractDir || !fs.existsSync(this.contractDir)) {
      throw new Error('Contract directory not available');
    }

    const sourcePath = join(this.contractDir, sourceVerifier);
    const targetPath = join(this.contractDir, targetVerifier);

    if (!fs.existsSync(sourcePath)) {
      throw new Error(`Source verifier file not found: ${sourcePath}`);
    }

    // Ensure target directory exists
    const targetDir = join(targetPath, '..');
    if (!fs.existsSync(targetDir)) {
      fs.mkdirSync(targetDir, { recursive: true });
    }

    fs.copyFileSync(sourcePath, targetPath);
    log.debug(`Copied ${sourceVerifier} to ${targetVerifier}`);
  }

  /**
   * Perform contract maintenance with optional authority switch and verifier preparation.
   * This is a high-level method that handles the complete maintenance flow including:
   * - Optional authority switch
   * - Verifier file preparation
   * - Maintenance transaction submission
   *
   * @param contractAddress - The untagged contract address
   * @param options - Maintenance options
   * @param options.authoritySeed - The authority seed to use (default: default seed)
   * @param options.newAuthoritySeed - Optional new authority seed to switch to before maintenance
   * @param options.fundingSeed - The seed to use for funding (defaults to authoritySeed)
   * @param options.counter - The counter value for the transaction (default: 0)
   * @param options.rngSeed - The random number generator seed (default: fixed seed)
   * @param options.removeEntrypoints - Array of entrypoint names to remove (e.g., ['decrement'])
   * @param options.upsertEntrypoints - Array of paths to verifier files to upsert (e.g., ['/toolkit-js/contract/managed/counter/keys/increment.verifier'])
   * @param options.prepareVerifiers - Optional array of {source, target} verifier file pairs to copy before maintenance
   * @param options.waitAfterCall - Wait time in ms after contract call before maintenance (default: 15000)
   * @param options.waitAfterAuthoritySwitch - Wait time in ms after authority switch (default: 10000)
   * @returns A promise that resolves to the transaction result
   * @throws Error if the container is not started or if maintenance transaction generation fails
   */
  async performContractMaintenance(
    contractAddress: string,
    options: {
      authoritySeed?: string;
      newAuthoritySeed?: string;
      fundingSeed?: string;
      counter?: number;
      rngSeed?: string;
      removeEntrypoints?: string[];
      upsertEntrypoints?: string[];
      prepareVerifiers?: Array<{ source: string; target: string }>;
      waitAfterCall?: number;
      waitAfterAuthoritySwitch?: number;
    } = {},
  ): Promise<ToolkitTransactionResult> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    const {
      authoritySeed = '0000000000000000000000000000000000000000000000000000000000000001',
      newAuthoritySeed,
      fundingSeed,
      counter = 0,
      rngSeed = '0000000000000000000000000000000000000000000000000000000000000001',
      removeEntrypoints = [],
      upsertEntrypoints = [],
      prepareVerifiers = [],
      waitAfterCall = 15_000,
      waitAfterAuthoritySwitch = 10_000,
    } = options;

    // Wait for previous operations to finalize
    if (waitAfterCall > 0) {
      await new Promise((resolve) => setTimeout(resolve, waitAfterCall));
    }

    // Step 1: Try to switch maintenance authority if requested
    let maintenanceAuthoritySeed = authoritySeed;
    if (newAuthoritySeed) {
      try {
        const authoritySwitchResult = await this.switchMaintenanceAuthority(
          contractAddress,
          authoritySeed,
          newAuthoritySeed,
        );
        if (authoritySwitchResult) {
          await new Promise((resolve) => setTimeout(resolve, waitAfterAuthoritySwitch));
          maintenanceAuthoritySeed = newAuthoritySeed;
        }
      } catch (error) {
        log.debug(`Authority switch failed, continuing with default authority: ${error}`);
      }
    }

    // Step 2: Prepare verifier files if needed
    for (const { source, target } of prepareVerifiers) {
      this.prepareVerifierFile(source, target);
    }

    // Step 3: Perform maintenance
    return this.maintainContract(contractAddress, {
      authoritySeed: maintenanceAuthoritySeed,
      fundingSeed: fundingSeed ?? maintenanceAuthoritySeed,
      counter,
      rngSeed,
      removeEntrypoints,
      upsertEntrypoints,
    });
  }
}

export { ToolkitWrapper, ToolkitConfig };
