// This file is part of midnightntwrk/midnight-indexer
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

import { writeFileSync, readFileSync } from 'fs';
import { join } from 'path';
import { DeployContractResult } from './toolkit/toolkit-wrapper';
import { getContractDeploymentHashes } from '../tests/e2e/test-utils';

export interface LocalData {
  'contract-address-untagged': string;
  'coin-public': string;
  'deploy-tx-hash': string;
  'deploy-block-hash': string;
}

/**
 * Utility class for managing local.json data
 */
export class LocalDataUtils {
  private dataDir: string;

  constructor(dataDir = 'data/static/undeployed') {
    this.dataDir = dataDir;
  }

  /**
   * Writes deployment data to local.json file
   *
   * @param deployResult - The deployment result from toolkit
   */
  async writeDeploymentData(deployResult: DeployContractResult): Promise<void> {
    const { txHash, blockHash } = await getContractDeploymentHashes(deployResult.addressUntagged);

    const localData: LocalData = {
      'contract-address-untagged': deployResult.addressUntagged,
      'coin-public': deployResult.coinPublic,
      'deploy-tx-hash': txHash,
      'deploy-block-hash': blockHash,
    };

    const localJsonPath = join(this.dataDir, 'local.json');

    console.log(`   Writing deployment data to: ${localJsonPath}`);
    writeFileSync(localJsonPath, JSON.stringify(localData, null, 2) + '\n', 'utf-8');

    console.log('\n' + '='.repeat(80));
    console.log('✅ SUCCESS - local.json updated with deployment data:');
    console.log('='.repeat(80));
    console.log(JSON.stringify(localData, null, 2));
    console.log('='.repeat(80));
  }

  /**
   * Reads existing local.json data
   *
   * @returns LocalData object
   */
  readLocalData(): LocalData {
    const localJsonPath = join(this.dataDir, 'local.json');

    try {
      const content = readFileSync(localJsonPath, 'utf8');
      return JSON.parse(content);
    } catch (error) {
      throw new Error(`Failed to read local.json: ${error}`);
    }
  }

  /**
   * Updates specific fields in local.json
   *
   * @param updates - Partial LocalData object with fields to update
   */
  updateLocalData(updates: Partial<LocalData>): void {
    const existingData = this.readLocalData();
    const updatedData = { ...existingData, ...updates };

    const localJsonPath = join(this.dataDir, 'local.json');
    writeFileSync(localJsonPath, JSON.stringify(updatedData, null, 2) + '\n', 'utf-8');

    console.log(`   Updated local.json with: ${JSON.stringify(updates, null, 2)}`);
  }

  /**
   * Validates that all required fields are present in local data
   *
   * @param data - LocalData object to validate
   * @returns boolean indicating if data is valid
   */
  validateLocalData(data: LocalData): boolean {
    const requiredFields: (keyof LocalData)[] = [
      'contract-address-untagged',
      'coin-public',
      'deploy-tx-hash',
      'deploy-block-hash',
    ];

    return requiredFields.every((field) => {
      const value = data[field];
      return value !== undefined && value !== null && value !== '';
    });
  }
}
