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

type JsonValue = string | number | boolean | null | JsonObject | JsonArray;
type JsonObject = { [key: string]: JsonValue };
type JsonArray = JsonValue[];

export interface ContractActionInfo {
  'action-type': string;
  'block-height': number;
  'block-hash': string;
}

export interface ContractInfo {
  'contract-address': string;
  'contract-actions': ContractActionInfo[];
}

/**
 * Imports and parses JSONC data from a file.
 * @param filePath - The path to the JSONC file.
 * @returns The parsed JSON data.
 */
function importJsoncData(filePath: string): JsonValue {
  const fileContent = fs.readFileSync(filePath, 'utf-8');
  return parse(fileContent);
}

/**
 * Provides test data for various test scenarios across different environments.
 * The data is loaded from environment-specific JSON files during initialization.
 */
class TestDataProvider {
  private contracts: ContractInfo[];
  private cardanoRewardAddresses: Record<string, string>;
  private unshieldedAddresses: Record<string, string>;

  constructor() {
    this.contracts = [];
    this.cardanoRewardAddresses = {};
    this.unshieldedAddresses = {};
  }

  /**
   * Initializes the test data provider by loading environment-specific data files.
   * @returns A promise that resolves to the initialized TestDataProvider instance.
   */
  async init(): Promise<this> {
    const envName = env.getEnvName();
    const baseDir = `data/static/${envName}`;

    this.contracts = importJsoncData(
      `${baseDir}/contract-actions.jsonc`,
    ) as unknown as ContractInfo[];
    this.unshieldedAddresses = importJsoncData(`${baseDir}/unshielded-addresses.json`) as Record<
      string,
      string
    >;
    this.cardanoRewardAddresses = importJsoncData(`${baseDir}/cardano-stake-keys.jsonc`) as Record<
      string,
      string
    >;

    return this;
  }

  /**
   * Gets the funding seed for the current environment.
   * First checks for an environment-specific variable (e.g., FUNDING_SEED_PREVIEW),
   * then falls back to a default seed for undeployed environments.
   * @returns The funding seed as a string.
   */
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

  /**
   * Retrieves an unshielded address from the test data by property name.
   * @param property - The property name of the unshielded address to retrieve.
   * @returns The unshielded address as a string.
   * @throws Error if the property is not found or undefined for the current environment.
   */
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

  /**
   * Searches through contracts to find a specific contract action by type.
   * @param actionType - The type of contract action to find (e.g., 'ContractDeploy', 'ContractCall').
   * @returns The contract action object if found, null otherwise.
   */
  private findContractAction(actionType: string): ContractActionInfo | null {
    // Contracts is an array of contract objects with a contract-actions array
    // NOTE: it could be empty if there are no contracts with all the actions types
    for (const contract of this.contracts) {
      const action = contract['contract-actions'].find((a) => a['action-type'] === actionType);
      if (action) {
        return action;
      }
    }
    return null;
  }

  /**
   * Retrieves the block hash associated with a specific contract action type.
   * @param actionType - The type of contract action.
   * @returns A promise that resolves to the block hash.
   * @throws Error if the action type is not found or has no block hash.
   */
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

  /**
   * Retrieves the block height associated with a specific contract action type.
   * @param actionType - The type of contract action.
   * @returns A promise that resolves to the block height as a number.
   * @throws Error if the action type is not found or has no block height.
   */
  private getBlockHeightOfContractAction(actionType: string): Promise<number> {
    const action = this.findContractAction(actionType);
    if (action && action['block-height'] !== undefined) {
      return Promise.resolve(action['block-height']);
    }
    return Promise.reject(
      new Error(
        `Test data provider is missing the block height for action type ${actionType} in ${env.getEnvName()} environment`,
      ),
    );
  }

  /**
   * Gets a known block hash from contract deployment action.
   * @returns A promise that resolves to the block hash.
   */
  getKnownBlockHash() {
    return this.getBlockData('ContractDeploy');
  }

  /**
   * Gets the block hash where a contract was deployed.
   * @returns A promise that resolves to the deployment block hash.
   */
  getContractDeployBlockHash() {
    return this.getBlockData('ContractDeploy');
  }

  /**
   * Gets the block hash where a contract was updated.
   * @returns A promise that resolves to the update block hash.
   */
  getContractUpdateBlockHash() {
    return this.getBlockData('ContractUpdate');
  }

  /**
   * Gets the block height where a contract was deployed.
   * @returns A promise that resolves to the deployment block height.
   */
  getContractDeployBlockHeight() {
    return this.getBlockHeightOfContractAction('ContractDeploy');
  }

  /**
   * Gets the block height where a contract was called.
   * @returns A promise that resolves to the contract call block height.
   */
  getContractCallBlockHeight() {
    return this.getBlockHeightOfContractAction('ContractCall');
  }

  /**
   * Gets the block height where a contract was updated.
   * @returns A promise that resolves to the update block height.
   */
  getContractUpdateBlockHeight() {
    return this.getBlockHeightOfContractAction('ContractUpdate');
  }

  /**
   * Returns an array of fabricated malformed hash values for negative testing.
   * These include invalid hex strings, incorrect lengths, and other malformed formats.
   * @returns An array of malformed hash strings.
   */
  getFabricatedMalformedHashes() {
    return [
      '0', // half byte
      '000000000000000000000000000000000000000000000000000000000000000G', // Not a valid hex string
      '000000000000000000000000000000000000000000000000000000000000000@', // Not a valid hex string
      '00000000000000000000000000000000000000000000000000000000000062', // 31 bytes (too short)
      '000000000000000000000000000000000000000000000000000000000000000066', // 33 bytes (too long)
    ];
  }

  /**
   * Returns an array of fabricated malformed identifier values for negative testing.
   * These include invalid hex strings and incorrect formats.
   * @returns An array of malformed identifier strings.
   */
  getFabricatedMalformedIdentifiers() {
    return [
      '000000000000000000000000000000000000000000000000000000000000000G', // Not a valid hex string
      '000000000000000000000000000000000000000000000000000000000000000@', // Not a valid hex string
      '0', // Half byte
    ];
  }

  /**
   * Returns an array of fabricated malformed height values for negative testing.
   * These include negative numbers, non-integers, and overflow values.
   * @returns An array of malformed height numbers.
   */
  getFabricatedMalformedHeights() {
    return [
      -1, // negative height
      0.5, // not an integer
      2 ** 32, // 32-bit overflow
    ];
  }

  /**
   * Gets a known contract address from the test data.
   * @returns The contract address as a string.
   * @throws Error if no contract address is found in the test data.
   */
  getKnownContractAddress(): string {
    if (this.contracts.length === 0 || !this.contracts[0]['contract-address']) {
      throw new Error(
        `Test data provider is missing the known contract address data for ${env.getEnvName()} environment`,
      );
    }
    return this.contracts[0]['contract-address'];
  }

  /**
   * Returns a valid format contract address that doesn't exist in the blockchain.
   * Used for testing non-existent address scenarios.
   * @returns A non-existing contract address string.
   */
  getNonExistingContractAddress() {
    // Return a valid format address that doesn't exist
    return '000200e99d4445695a6244a01ab00d592825e2703c3f9a928f01429561585ce2db1e79';
  }

  /**
   * Returns an array of fabricated malformed contract addresses for negative testing.
   * These include spaces, invalid characters, incorrect lengths, and type mismatches.
   * @returns An array of malformed contract address values.
   */
  getFabricatedMalformedContractAddresses() {
    return [
      ' ', // space
      '0', // too short
      null as unknown as string, // null
      undefined as unknown as string, // undefined
      NaN as unknown as string, // NaN
      Infinity as unknown as string, // Infinity
      -Infinity as unknown as string, // -Infinity
      false as unknown as string, // false
      true as unknown as string, // true
      '000200e99d4445695a6244a01ab00d592825e2703c3f9a928f01429561585ce2db1e7', // too short (63 chars)
      '000200e99d4445695a6244a01ab00d592825e2703c3f9a928f01429561585ce2db1e78a', // too long (65 chars)
      '000200e99d4445695a6244a01ab00d592825e2703c3f9a928f01429561585ce2db1e7g', // invalid hex character
      '000200e99d4445695a6244a01ab00d592825e2703c3f9a928f01429561585ce2db1e7@', // special character
      '000200e99d4445695a6244a01ab00d592825e2703c3f9a928f01429561585ce2db1e7 ', // trailing space
      ' 000200e99d4445695a6244a01ab00d592825e2703c3f9a928f01429561585ce2db1e78', // leading space
      ' 000200e99d4445695a6244a01ab00d592825e2703c3f9a928f01429561585ce2db1e78 ', // leading and trailing space
    ];
  }

  /**
   * Returns boundary value contract addresses for edge case testing.
   * These include all zeros, all ones, and maximum hex values.
   * @returns An array of boundary contract addresses.
   */
  getBoundaryContractAddresses() {
    return [
      '0000000000000000000000000000000000000000000000000000000000000000000000000000', // all zeros
      '0000000000000000000000000000000000000000000000000000000000000000000000000001', // all zeros except first byte
      '1111111111111111111111111111111111111111111111111111111111111111111111111111', // all ones
      'ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff', // highest hex value
    ];
  }

  /**
   * Returns a valid format hash that doesn't exist in the blockchain.
   * Used for testing non-existent hash scenarios.
   * @returns A non-existing hash string (all zeros).
   */
  getNonExistingHash() {
    // Return a valid format hash that doesn't exist (all zeros)
    return '0000000000000000000000000000000000000000000000000000000000000000';
  }

  /**
   * Retrieves a Cardano stake key from the test data by property name.
   * @param property - The property name of the Cardano stake key to retrieve.
   * @returns The Cardano stake key as a string.
   * @throws Error if the property is not found or undefined for the current environment.
   */
  getCardanoRewardAddress(property: string) {
    if (
      !this.cardanoRewardAddresses.hasOwnProperty(property) ||
      this.cardanoRewardAddresses[property] === undefined
    ) {
      throw new Error(
        `Test data provider is missing the cardano stake key data for ${property} for ${env.getEnvName()} environment`,
      );
    }
    return this.cardanoRewardAddresses[property];
  }

  /**
   * Returns an array of fabricated malformed Cardano stake keys for negative testing.
   * These include empty strings, invalid hex characters, and special characters.
   * @returns An array of malformed Cardano stake key strings.
   */
  getFabricatedMalformedCardanoRewardAddresss() {
    return [
      '', // empty string
      'G'.repeat(64), // invalid hex characters
      '0123456789abcdef@', // special character
    ];
  }
}

const dataProvider = await new TestDataProvider().init();
export default dataProvider;
