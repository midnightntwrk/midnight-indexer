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
import { LocalDataUtils } from '@utils/local-data-utils';

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
  addressUntagged: string;
  addressTagged: string;
  contractAddress: string;
  coinPublic: string;
  deployTxPath: string;
  statePath: string;
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
    callKey: string = 'increment',
    rngSeed: string = '0000000000000000000000000000000000000000000000000000000000000037',
  ): Promise<ToolkitTransactionResult> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    const { env } = await import('../../environment/model.js');
    const localDataPath = join(__dirname, '../../data/static', env.getEnvName(), 'local.json');

    const localData = JSON.parse(readFileSync(localDataPath, 'utf8'));
    const contractAddressUntagged = localData['contract-address-untagged'];
    const contractAddressTagged = localData['contract-address-tagged'];
    const coinPublic = localData['coin-public'];

    if (!contractAddressUntagged || !contractAddressTagged || !coinPublic) {
      throw new Error('Missing required contract data in local.json');
    }

    const intentFile = `/out/${callKey}_intent.bin`;
    const txFile = `/out/${callKey}_tx.mn`;
    const stateFile = `/out/current_state.mn`;
    const privateStateFile = `/out/${callKey}_private_state.json`;

    log.info('Getting current contract state...');
    const contractStateResult = await this.startedContainer.exec([
      '/midnight-node-toolkit',
      'contract-state',
      '--contract-address',
      contractAddressTagged,
      '--dest-file',
      stateFile,
    ]);

    if (contractStateResult.exitCode !== 0) {
      const errorMessage =
        contractStateResult.stderr || contractStateResult.output || 'Unknown error occurred';
      throw new Error(`Failed to get contract state: ${errorMessage}`);
    }

    log.info(`Generating ${callKey} circuit intent...`);
    const generateIntentResult = await this.startedContainer.exec([
      '/midnight-node-toolkit',
      'generate-intent',
      'circuit',
      '-c',
      '/toolkit-js/test/contract/contract.config.ts',
      '--toolkit-js-path',
      '/toolkit-js',
      '--contract-address',
      contractAddressUntagged,
      '--coin-public',
      coinPublic,
      '--input-onchain-state',
      stateFile,
      '--input-private-state',
      '/out/initial_state.json',
      '--output-intent',
      intentFile,
      '--output-private-state',
      privateStateFile,
      '--output-zswap-state',
      `/out/${callKey}_zswap.json`,
      callKey,
    ]);

    if (generateIntentResult.exitCode !== 0) {
      const errorMessage =
        generateIntentResult.stderr || generateIntentResult.output || 'Unknown error occurred';
      throw new Error(`Failed to generate circuit intent: ${errorMessage}`);
    }

    log.info('Converting intent to transaction...');
    const sendIntentResult = await this.startedContainer.exec([
      '/midnight-node-toolkit',
      'send-intent',
      '--intent-file',
      intentFile,
      '--compiled-contract-dir',
      '/toolkit-js/test/contract/managed/counter',
      '--to-bytes',
      '--dest-file',
      txFile,
    ]);

    if (sendIntentResult.exitCode !== 0) {
      const errorMessage =
        sendIntentResult.stderr || sendIntentResult.output || 'Unknown error occurred';
      throw new Error(`Failed to send intent: ${errorMessage}`);
    }

    log.info('Submitting transaction to network...');
    const result = await this.startedContainer.exec([
      '/midnight-node-toolkit',
      'generate-txs',
      '--src-files',
      txFile,
      '-r',
      '1',
      'send',
    ]);

    if (result.exitCode !== 0) {
      const errorMessage = result.stderr || result.output || 'Unknown error occurred';
      throw new Error(`Failed to submit transaction: ${errorMessage}`);
    }

    const rawOutput = result.output.trim();
    return this.parseTransactionOutput(rawOutput);
  }

  async deployContract(opts?: {
    contractConfigPath?: string;
    compiledContractDir?: string;
    network?: string;
    enableLogging?: boolean;
    writeTestData?: boolean;
    dataDir?: string;
  }): Promise<DeployContractResult> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    const enableLogging = opts?.enableLogging ?? false;
    const writeTestData = opts?.writeTestData ?? false;
    const dataDir = opts?.dataDir ?? `data/static/${env.getEnvName()}`;

    if (enableLogging) {
      log.info('='.repeat(80));
      log.info('CONTRACT DEPLOYMENT');
      log.info('='.repeat(80));
      log.info('\n1. Starting contract deployment...');
    }

    const outDir = this.config.targetDir!;

    const contractConfigPath =
      opts?.contractConfigPath ?? '/toolkit-js/test/contract/contract.config.ts';
    const compiledContractDir =
      opts?.compiledContractDir ?? '/toolkit-js/test/contract/managed/counter';
    const network = (opts?.network ?? this.runtime.network).toLowerCase();

    const deployIntent = 'deploy.bin';
    const deployTx = 'deploy_tx.mn';
    const addressFile = 'contract_address.mn';
    const stateFile = 'contract_state.mn';
    const initialPrivateState = 'initial_state.json';

    const outDeployIntent = join(outDir, deployIntent);
    const outDeployTx = join(outDir, deployTx);
    const outAddressFile = join(outDir, addressFile);
    const outStateFile = join(outDir, stateFile);
    const outInitialState = join(outDir, initialPrivateState);
    const zswapFile = 'temp.json';
    const coinPublicSeed = '0000000000000000000000000000000000000000000000000000000000000001';
    const addressInfo = await this.showAddress(coinPublicSeed);
    const coinPublic = addressInfo.coinPublic;

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
      if (!existsSync(outDeployIntent) || !existsSync(outInitialState)) {
        throw new Error('generate-intent deploy did not produce expected outputs');
      }
    }

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
      if (!existsSync(outDeployTx)) {
        throw new Error('send-intent did not produce /out/deploy_tx.mn');
      }
    }

    {
      const result = await this.startedContainer.exec([
        '/midnight-node-toolkit',
        'generate-txs',
        '--src-files',
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

    const result = await this.startedContainer.exec([
      '/midnight-node-toolkit',
      'contract-address',
      '--network',
      network,
      '--src-file',
      '/out/deploy_tx.mn',
      '--dest-file',
      '/out/contract_address.json',
    ]);
    if (result.exitCode !== 0) {
      const e = result.stderr || result.output || 'Unknown error';
      throw new Error(`contract-address failed: ${e}`);
    }

    let contractAddressInfo: any;

    if (result.output && result.output.trim()) {
      try {
        contractAddressInfo = JSON.parse(result.output.trim());
      } catch (e) {
        // Failed to parse stdout, will try file
      }
    }

    if (!contractAddressInfo) {
      const addressJsonPath = join(outDir, 'contract_address.json');
      if (existsSync(addressJsonPath)) {
        const addressFileContent = readFileSync(addressJsonPath, 'utf8').trim();
        try {
          contractAddressInfo = JSON.parse(addressFileContent);
        } catch (e) {
          contractAddressInfo = {
            tagged: addressFileContent,
            untagged: addressFileContent.replace(/^.*:/, ''),
          };
        }
      } else {
        throw new Error('contract-address did not produce output or file');
      }
    }

    fs.writeFileSync(outAddressFile, contractAddressInfo.tagged + '\n', 'utf8');

    const hexPrefix = '6d69646e696768743a636f6e74726163742d616464726573735b76325d3a';
    let contractAddress = contractAddressInfo.untagged;
    if (contractAddress.startsWith(hexPrefix)) {
      contractAddress = contractAddress.substring(hexPrefix.length);
    }

    {
      const result = await this.startedContainer.exec([
        '/midnight-node-toolkit',
        'contract-state',
        '--contract-address',
        contractAddressInfo.tagged,
        '--dest-file',
        '/out/contract_state.mn',
      ]);
      if (result.exitCode !== 0) {
        const e = result.stderr || result.output || 'Unknown error';
        throw new Error(`contract-state failed: ${e}`);
      }
      if (!existsSync(outStateFile)) {
        throw new Error('contract-state did not produce /out/contract_state.mn');
      }
    }

    const deployResult = {
      addressUntagged: contractAddressInfo.untagged,
      addressTagged: contractAddressInfo.tagged,
      contractAddress,
      coinPublic,
      deployTxPath: outDeployTx,
      statePath: outStateFile,
    };

    if (enableLogging) {
      log.info('\n✅ Contract deployed successfully!');
      log.info(`   Address (Untagged):    ${deployResult.addressUntagged}`);
      log.info(`   Address (Tagged):      ${deployResult.addressTagged}`);
      log.info(`   Contract Address:      ${deployResult.contractAddress}`);
      log.info(`   Coin Public:           ${deployResult.coinPublic}`);
    }

    if (writeTestData) {
      if (enableLogging) {
        log.info('\n2. Updating local.json with deployment data from indexer...');
      }
      const localDataUtils = new LocalDataUtils(dataDir);
      await localDataUtils.writeDeploymentData(deployResult);

      if (enableLogging) {
        log.info('\nThe test will use this deployed contract to make contract calls.');
        log.info('='.repeat(80));
      }
    }

    return deployResult;
  }
}

export { ToolkitWrapper, ToolkitConfig ,ToolkitTransactionResult};
