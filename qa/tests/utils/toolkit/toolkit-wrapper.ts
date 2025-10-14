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
import path from 'path';
import log from '@utils/logging/logger';
import { retry } from '../retry-helper';
import { env } from '../../environment/model';
import { GenericContainer, StartedTestContainer } from 'testcontainers';

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

interface ContractAddressInfo {
  tagged: string;
  untagged: string;
}

interface ToolkitConfig {
  containerName?: string;
  targetDir?: string;
  chain?: string;
  nodeTag?: string;
  syncCacheDir?: string;
  coinSeed?: string;
}

export interface ToolkitTransactionResult {
  txHash: string;
  blockHash?: string;
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
  addressRaw: string;
  addressUntagged: string;
  addressTagged: string;
  deployTxPath: string;
  statePath: string;
  outDir: string;
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
        // Skip lines that aren't valid JSON
        continue;
      }
    }

    if (!txHash) {
      throw new Error('Could not extract transaction hash from toolkit output');
    }

    return {
      txHash,
      blockHash,
      status,
      rawOutput: output,
    };
  }

  constructor(config: ToolkitConfig) {
    this.config = config;

    const randomId = Math.random().toString(36).slice(2, 12);

    this.config.containerName =
      config.containerName || `mn-toolkit-${env.getEnvName()}-${randomId}`;
    this.config.targetDir = config.targetDir || '/tmp/toolkit/';
    this.config.nodeTag = config.nodeTag || env.getNodeVersion();
    this.config.syncCacheDir = `${this.config.targetDir}/.sync_cache-${env.getEnvName()}-${randomId}`;

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

  async start() {
    this.startedContainer = await retry(async () => this.container.start(), {
      maxRetries: 2,
      delayMs: 2_000,
      retryLabel: 'start toolkit container',
    });
  }

  async stop() {
    if (this.startedContainer) {
      await this.startedContainer.stop();
    }
  }

  /**
   * Show address information from a seed
   *
   * @param seed - The seed to use
   * @param networkId - The network ID to use (default: current target environment)
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

    // Extract the json object and return it as is
    return JSON.parse(response.output);
  }

  /**
   * Show viewing key information from a seed
   *
   * @param seed - The seed to use
   * @param networkId - The network ID to use (default: current target environment)
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
   * Generate a single shie.ded or unshieldedtransaction
   *
   * @param sourceSeed - The source seed to use
   * @param addressType - The address type to use
   * @param destinationAddress - The destination address to use
   * @param amount - The amount to use
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
   * Get the contract address from a deployed transaction
   *
   * @param contractFile - The contract file to use
   * @param tagType - Whether the address should be tagged or untagged
   * @returns The contract address
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
   * Deploy a contract
   *
   * @param opts - The options for the deployment
   * @returns
   */
  async deployContract(opts?: {
    contractConfigPath?: string;
    compiledContractDir?: string;
    network?: string;
  }): Promise<DeployContractResult> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }
    const outDir = this.config.targetDir!;

    const contractConfigPath =
      opts?.contractConfigPath ?? '/toolkit-js/test/contract/contract.config.ts';
    const compiledContractDir =
      opts?.compiledContractDir ?? '/toolkit-js/test/contract/managed/counter';

    const zswapFile = 'temp.json';
    const deployTx = 'deploy_tx.mn';
    const deployIntent = 'deploy.bin';
    const stateFile = 'contract_state.mn';
    const addressFile = 'contract_address.mn';
    const initialPrivateState = 'initial_state.json';

    const outDeployIntent = path.join(outDir, deployIntent);
    const outDeployTx = path.join(outDir, deployTx);
    const outAddressFile = path.join(outDir, addressFile);
    const outStateFile = path.join(outDir, stateFile);
    const outInitialState = path.join(outDir, initialPrivateState);

    const coinPublicSeed = '00000000000000000000000000000001';
    const addressInfo = await this.showAddress(coinPublicSeed);
    const coinPublic = addressInfo.coinPublic;
    let addressRaw = '';

    // 1) generate-intent deploy
    {
      const result = await this.startedContainer.exec([
        '/midnight-node-toolkit',
        'generate-intent',
        'deploy',
        '-c',
        contractConfigPath,
        '--output-intent',
        `/out/${deployIntent}`,
        '--output-private-state',
        `/out/${initialPrivateState}`,
        '--coin-public',
        coinPublic,
        '--output-zswap-state',
        `/out/${zswapFile}`,
      ]);
      if (result.exitCode !== 0) {
        const e = result.stderr || result.output || 'Unknown error';
        throw new Error(`generate-intent deploy failed: ${e}`);
      }
      if (!fs.existsSync(outDeployIntent) || !fs.existsSync(outInitialState)) {
        throw new Error('generate-intent deploy did not produce expected outputs');
      }
    }

    // 2) send-intent -> bytes (.mn)
    {
      const result = await this.startedContainer.exec([
        '/midnight-node-toolkit',
        'send-intent',
        '--intent-file',
        `/out/${deployIntent}`,
        '--compiled-contract-dir',
        compiledContractDir,
        '--to-bytes',
        '--dest-file',
        `/out/${deployTx}`,
      ]);
      if (result.exitCode !== 0) {
        const e = result.stderr || result.output || 'Unknown error';
        throw new Error(`send-intent failed: ${e}`);
      }
      if (!fs.existsSync(outDeployTx)) {
        throw new Error(`send-intent did not produce /out/${deployTx}`);
      }
    }

    // 3) generate-txs ... send
    {
      const result = await this.startedContainer.exec([
        '/midnight-node-toolkit',
        'generate-txs',
        '--src-file',
        `/out/${deployTx}`,
        '-r',
        '1',
        'send',
      ]);
      if (result.exitCode !== 0) {
        const e = result.stderr || result.output || 'Unknown error';
        throw new Error(`generate-txs send failed: ${e}`);
      }
    }

    // Get tagged and untagged contract addresss and build an object with them
    const contractAddressInfo = {
      tagged: await this.getContractAddress(deployTx, 'tagged'),
      untagged: await this.getContractAddress(deployTx, 'untagged'),
    };

    log.debug(`Contract address info:\n${JSON.stringify(contractAddressInfo, null, 2)}`);

    // persist EXACTLY the typed string (no JSON)
    fs.writeFileSync(outAddressFile, contractAddressInfo.tagged + '\n', 'utf8');

    // share with later steps
    addressRaw = contractAddressInfo.tagged;

    if (!fs.existsSync(outAddressFile)) {
      throw new Error('contract-address did not produce /out/contract_address.mn');
    }

    const raw = fs.readFileSync(outAddressFile, 'utf8').trim();

    // 5) quick state read — pass the address exactly as written by the toolkit
    {
      const result = await this.startedContainer.exec([
        '/midnight-node-toolkit',
        'contract-state',
        '--contract-address',
        addressRaw,
        '--dest-file',
        '/out/contract_state.mn',
      ]);
      if (result.exitCode !== 0) {
        const e = result.stderr || result.output || 'Unknown error';
        throw new Error(`contract-state failed: ${e}`);
      }
      if (!fs.existsSync(outStateFile)) {
        throw new Error('contract-state did not produce /out/contract_state.mn');
      }
    }

    return {
      addressRaw: raw,
      addressUntagged: contractAddressInfo.untagged,
      addressTagged: contractAddressInfo.tagged,
      deployTxPath: outDeployTx,
      statePath: outStateFile,
      outDir,
    };
  }
}

export { ToolkitWrapper, ToolkitConfig };
