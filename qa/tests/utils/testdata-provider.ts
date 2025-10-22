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
import { parse } from 'jsonc-parser';
import { env } from '../environment/model';

function importJsoncData(filePath: string): any {
  const fileContent = fs.readFileSync(filePath, 'utf-8');
  return parse(fileContent);
}

class TestDataProvider {
  private unshieldedAddresses: Record<string, string>;
  private blocks: Record<string, string>;
  private contracts: any[];

  constructor() {
    this.unshieldedAddresses = {};
    this.blocks = {};
    this.contracts = [];
  }

  async init(): Promise<this> {
    const envName = env.getEnvName();
    const baseDir = `data/static/${envName}`;

    this.blocks = importJsoncData(`${baseDir}/blocks.jsonc`);
    this.contracts = importJsoncData(`${baseDir}/contract-actions.jsonc`);
    this.unshieldedAddresses = importJsoncData(`${baseDir}/unshielded-addresses.json`);

    return this;
  }

  getFundingSeed() {
    // Build the environment-specific variable name (e.g., FUNDING_SEED_PREVIEW)
    const envName = env.getEnvName().toUpperCase();
    const envVarName = `FUNDING_SEED_${envName}`;

    // Try environment-specific variable first
    const fundingSeed = process.env[envVarName];

    if (fundingSeed) {
      return fundingSeed;
    }

    // Default fallback
    const undeployedFundingSeed = '0'.repeat(63) + '1';
    return undeployedFundingSeed;
  }

  getUnshieldedAddress(property: string) {
    if (
      !this.unshieldedAddresses.hasOwnProperty(property) ||
      this.unshieldedAddresses[property] === undefined
    ) {
      throw new Error(
        `Test data provider is missing the unshielded address data for ${property} for ${env.getEnvName()} environment`,
      );
    }
    return this.unshieldedAddresses[property];
  }

  private findContractAction(actionType: string): any {
    // Contracts is an array of contract objects with a contract-actions array
    // NOTE: it could be empty if there are no contracts with all the actions types
    for (const contract of this.contracts as any[]) {
      if (contract['contract-actions']) {
        const action = contract['contract-actions'].find(
          (a: any) => a['action-type'] === actionType,
        );
        if (action) {
          return action;
        }
      }
    }
    return null;
  }

  private getBlockData(actionType: string): Promise<string> {
    const action = this.findContractAction(actionType);
    if (action && action['block-hash'] !== undefined) {
      return Promise.resolve(action['block-hash']);
    }
    return Promise.reject(
      new Error(
        `Test data provider missing the block hash for action type ${actionType} in ${env.getEnvName()} environment`,
      ),
    );
  }

  private getBlockHeightOfContractAction(actionType: string): Promise<number> {
    const action = this.findContractAction(actionType);
    if (action && action['block-height'] !== undefined) {
      return Promise.resolve(parseInt(action['block-height']));
    }
    return Promise.reject(
      new Error(
        `Test data provider is missing the block height for action type ${actionType} in ${env.getEnvName()} environment`,
      ),
    );
  }

  getKnownBlockHash() {
    return this.getBlockData('ContractDeploy');
  }

  getContractDeployBlockHash() {
    return this.getBlockData('ContractDeploy');
  }

  getContractUpdateBlockHash() {
    return this.getBlockData('ContractUpdate');
  }

  getContractDeployBlockHeight() {
    return this.getBlockHeightOfContractAction('ContractDeploy');
  }

  getContractCallBlockHeight() {
    return this.getBlockHeightOfContractAction('ContractCall');
  }

  getContractUpdateBlockHeight() {
    return this.getBlockHeightOfContractAction('ContractUpdate');
  }

  getViewingKey() {
    return 'mn_shield-esk_undeployed1d45kgmnfva58gwn9de3hy7tsw35k7m3dwdjkxun9wskkketetdmrzhf6wdwg0q0t85zu4sgm8ldgf66hkxmupkjn3spfncne2gtykttjjhjq2mjpxh8';
  }

  getFabricatedMalformedHashes() {
    return [
      '0', // half byte
      '000000000000000000000000000000000000000000000000000000000000000G', // Not a valid hex string
      '000000000000000000000000000000000000000000000000000000000000000@', // Not a valid hex string
      '00000000000000000000000000000000000000000000000000000000000062', // 31 bytes (too short)
      '000000000000000000000000000000000000000000000000000000000000000066', // 33 bytes (too long)
    ];
  }

  getFabricatedMalformedIdentifiers() {
    return [
      '000000000000000000000000000000000000000000000000000000000000000G', // Not a valid hex string
      '000000000000000000000000000000000000000000000000000000000000000@', // Not a valid hex string
      '0', // Half byte
    ];
  }

  getFabricatedMalformedHeights() {
    return [
      -1, // negative height
      0.5, // not an integer
      2 ** 32, // 32-bit overflow
    ];
  }

  getKnownContractAddress() {
    if (this.contracts.length === 0 || !this.contracts[0]['contract-address']) {
      throw new Error(
        `Test data provider is missing the known contract address data for ${env.getEnvName()} environment`,
      );
    }
    return this.contracts[0]['contract-address'];
  }

  getNonExistingContractAddress() {
    // Return a valid format address that doesn't exist
    return '000200e99d4445695a6244a01ab00d592825e2703c3f9a928f01429561585ce2db1e79';
  }

  getFabricatedMalformedContractAddresses() {
    return [
      ' ', // space
      '0', // too short
      null as any, // null
      undefined as any, // undefined
      NaN as any, // NaN
      Infinity as any, // Infinity
      -Infinity as any, // -Infinity
      false as any, // false
      true as any, // true
      '000200e99d4445695a6244a01ab00d592825e2703c3f9a928f01429561585ce2db1e7', // too short (63 chars)
      '000200e99d4445695a6244a01ab00d592825e2703c3f9a928f01429561585ce2db1e78a', // too long (65 chars)
      '000200e99d4445695a6244a01ab00d592825e2703c3f9a928f01429561585ce2db1e7g', // invalid hex character
      '000200e99d4445695a6244a01ab00d592825e2703c3f9a928f01429561585ce2db1e7@', // special character
      '000200e99d4445695a6244a01ab00d592825e2703c3f9a928f01429561585ce2db1e7 ', // trailing space
      ' 000200e99d4445695a6244a01ab00d592825e2703c3f9a928f01429561585ce2db1e78', // leading space
      ' 000200e99d4445695a6244a01ab00d592825e2703c3f9a928f01429561585ce2db1e78 ', // leading and trailing space
    ];
  }

  getBoundaryContractAddresses() {
    return [
      '0000000000000000000000000000000000000000000000000000000000000000000000000000', // all zeros
      '0000000000000000000000000000000000000000000000000000000000000000000000000001', // all zeros except first byte
      '1111111111111111111111111111111111111111111111111111111111111111111111111111', // all ones
      'ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff', // highest hex value
    ];
  }

  getNonExistingHash() {
    // Return a valid format hash that doesn't exist (all zeros)
    return '0000000000000000000000000000000000000000000000000000000000000000';
  }

  // Lazy load local data (read from file each time, as it's generated at runtime)
  private loadLocalData() {
    const envName = env.getEnvName();
    const baseDir = `data/static/${envName}`;
    return importJsoncData(`${baseDir}/local.json`);
  }

  getLocalDeployTxHash() {
    const local = this.loadLocalData();
    if (!local.hasOwnProperty('deploy-tx-hash') || local['deploy-tx-hash'] === undefined) {
      throw new Error(
        `Test data provider is missing the deploy-tx-hash data for ${env.getEnvName()} environment`,
      );
    }
    return local['deploy-tx-hash'];
  }

  getLocalDeployBlockHash() {
    const local = this.loadLocalData();
    if (!local.hasOwnProperty('deploy-block-hash') || local['deploy-block-hash'] === undefined) {
      throw new Error(
        `Test data provider is missing the deploy-block-hash data for ${env.getEnvName()} environment`,
      );
    }
    return local['deploy-block-hash'];
  }
}

const dataProvider = await new TestDataProvider().init();
export default dataProvider;
