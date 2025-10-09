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
import log from '@utils/logging/logger';
import { env, networkIdByEnvName } from '../../environment/model';
import { GenericContainer, StartedTestContainer } from 'testcontainers';
import { existsSync, readFileSync } from 'fs';
import { join } from 'path';

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
  toolkitImage?: string;
  nodeContainer?: string;
  network?: string;
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
  public readonly runtime!: { toolkitImage: string; nodeContainer: string; network: string };

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

    const toolkitImage =
      config.toolkitImage ??
      process.env.TOOLKIT_IMAGE ??
      `ghcr.io/midnight-ntwrk/midnight-node-toolkit:${process.env.NODE_TAG ?? '0.17.0-rc.2'}`;

    const nodeContainer =
      config.nodeContainer ?? process.env.NODE_CONTAINER ?? 'midnight-indexer-node-1';

    const network = (config.network ?? process.env.TARGET_ENV ?? 'undeployed').toLowerCase();

    this.runtime = { toolkitImage, nodeContainer, network };

    log.debug(`Toolkit container name: ${this.config.containerName}`);
    log.debug(`Toolkit target dir: ${this.config.targetDir}`);
    log.debug(`Toolkit node tag: ${this.config.nodeTag}`);
    log.debug(`Toolkit sync cache dir: ${this.config.syncCacheDir}`);

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
    const image = this.runtime.toolkitImage;
    this.startedContainer = await this.container.start();
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
   * @returns The address information as a JSON object
   */
  async showAddress(seed: string): Promise<AddressInfo> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    const response = await this.startedContainer.exec([
      '/midnight-node-toolkit',
      'show-address',
      '--network',
      env.getEnvName().toLowerCase(),
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

  async showViewingKey(seed: string): Promise<string> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    const result = await this.startedContainer.exec([
      '/midnight-node-toolkit',
      'show-viewing-key',
      '--network',
      env.getEnvName().toLowerCase(),
      '--seed',
      seed,
    ]);

    if (result.exitCode !== 0) {
      const errorMessage = result.stderr || result.output || 'Unknown error occurred';
      throw new Error(`Toolkit command failed with exit code ${result.exitCode}: ${errorMessage}`);
    }

    return result.output.trim();
  }

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

  async callContract(
    contractAddress: string,
    callKey: string,
    rngSeed: string,
  ): Promise<ToolkitTransactionResult> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    // Write contract address to a file in HEX-ENCODED tagged format AS TEXT
    // The targetDir (e.g., /tmp/toolkit/) is mounted to /out in the container
    // File format: hex("midnight:contract-address[v2]:" + raw_address_bytes)
    const tag = Buffer.from('midnight:contract-address[v2]:', 'utf8');
    const addressBytes = Buffer.from(contractAddress, 'hex');
    const taggedAddressHex = Buffer.concat([tag, addressBytes]).toString('hex');

    const contractAddressFile = `/out/contract_address_call.mn`;
    const writeCommand = `echo -n "${taggedAddressHex}" > ${contractAddressFile}`; // Write as hex text, not binary

    const writeResult = await this.startedContainer.exec(['sh', '-c', writeCommand]);

    if (writeResult.exitCode !== 0) {
      throw new Error(
        `Failed to write contract address to file: ${writeResult.stderr || writeResult.output}`,
      );
    }

    // Pass the FILE PATH directly - the toolkit expects a path to the hex-encoded tagged address file
    const result = await this.startedContainer.exec([
      '/midnight-node-toolkit',
      'generate-txs',
      'contract-calls',
      'call',
      '--call-key',
      callKey,
      '--rng-seed',
      rngSeed,
      '--contract-address',
      contractAddressFile,
    ]);

    if (result.exitCode !== 0) {
      const errorMessage = result.stderr || result.output || 'Unknown error occurred';
      throw new Error(`Toolkit command failed with exit code ${result.exitCode}: ${errorMessage}`);
    }

    const rawOutput = result.output.trim();
    return this.parseTransactionOutput(rawOutput);
  }
}

export { ToolkitWrapper, ToolkitConfig, ToolkitTransactionResult };
