// This file is part of midnightntwrk/midnight-indexer
// Copyright (C) Midnight Foundation
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

import fs from "fs";
import path from "path";
import * as commentJson from "comment-json";
import { TARGET_ENV } from "./env.js";
import { Transaction, UnshieldedUtxo } from "./indexer-types.js";

// ============================================================================
// Type Definitions
// ============================================================================

/**
 * Represents a block with transactions
 */
interface Block {
  hash: string;
  height: number;
  transactions: TransactionWithType[];
}

// ============================================================================
// Custom Error Classes
// ============================================================================

/**
 * Base error class for test data handler errors
 */
class TestDataHandlerError extends Error {
  constructor(
    message: string,
    public readonly context?: Record<string, any>,
  ) {
    super(message);
    this.name = "TestDataHandlerError";
    Error.captureStackTrace(this, this.constructor);
  }
}

/**
 * Error thrown when file operations fail
 */
class FileOperationError extends TestDataHandlerError {
  constructor(
    message: string,
    public readonly filePath: string,
    context?: Record<string, any>,
  ) {
    super(message, { ...context, filePath });
    this.name = "FileOperationError";
  }
}

/**
 * Error thrown when parsing fails
 */
class ParseError extends TestDataHandlerError {
  constructor(
    message: string,
    public readonly data: string,
    context?: Record<string, any>,
  ) {
    super(message, { ...context, dataPreview: data.substring(0, 100) });
    this.name = "ParseError";
  }
}

/**
 * Error thrown when data validation fails
 */
class ValidationError extends TestDataHandlerError {
  constructor(message: string, context?: Record<string, any>) {
    super(message, context);
    this.name = "ValidationError";
  }
}

/**
 * Base transaction interface with common fields
 */
interface BaseTransaction {
  __typename: string;
  hash: string;
  identifiers?: any;
}

/**
 * Regular transaction with contract actions and unshielded UTXOs
 */
interface RegularTransaction extends BaseTransaction {
  __typename: "RegularTransaction";
  contractActions?: ContractAction[];
  unshieldedCreatedOutputs?: UnshieldedUtxo[];
  unshieldedSpentOutputs?: UnshieldedUtxo[];
}

/**
 * System transaction
 */
interface SystemTransaction extends BaseTransaction {
  __typename: "SystemTransaction";
}

/**
 * Union type for all transaction types
 */
type TransactionWithType = RegularTransaction | SystemTransaction;

/**
 * Contract action interface
 */
interface ContractAction {
  address: string;
  __typename: string;
}

/**
 * Structure for blocks.jsonc file
 */
interface BlockDataFile {
  latest: string;
  "other-blocks": string[];
}

/**
 * Structure for transactions.jsonc file
 */
interface TransactionDataFile {
  "regular-transactions": Transaction[];
  "system-transactions": Transaction[];
}

/**
 * Entry for a single contract action with metadata
 */
interface ContractActionEntry {
  "action-type": string;
  "block-height": number;
  "block-hash": string;
}

/**
 * Structure for a contract with its actions
 */
interface ContractWithActions {
  "contract-address": string;
  "contract-actions": ContractActionEntry[];
}

/**
 * Structure for contract-actions.jsonc file (array of contracts)
 */
type ContractActionsDataFile = ContractWithActions[];

/**
 * Map of contract addresses to their actions
 */
interface ContractActionsMap {
  [address: string]: ContractActionEntry[];
}

/**
 * One owner's unspent holding of a single unshielded token type, as of scan time
 */
interface UnspentHolder {
  owner: string;
  "unspent-value": string;
  "unspent-utxos": number;
}

/**
 * A custom (non-NIGHT) unshielded token type with the owners holding it unspent
 */
interface CustomTokenType {
  "token-type": string;
  "unspent-holders": UnspentHolder[];
}

/**
 * Structure for unshielded-token-types.jsonc file
 */
interface UnshieldedTokenTypesDataFile {
  scan: {
    "from-block": number;
    "to-block": number;
    "scanned-at": string;
  };
  night: {
    "unspent-utxos": number;
    "distinct-owners": number;
  };
  "custom-tokens": CustomTokenType[];
}

/**
 * NIGHT, the chain's native unshielded token, is the all-zero token type. It is on
 * every chain and dominates every scan, so it is summarised rather than listed as a
 * candidate.
 */
const NIGHT_TOKEN_TYPE = "0".repeat(64);

// ============================================================================
// Validation Functions
// ============================================================================

/**
 * Validates that a block has required fields
 * @param block - Block object to validate
 * @throws ValidationError if block is invalid
 */
function validateBlock(block: any): block is Block {
  if (!block || typeof block !== "object") {
    throw new ValidationError("Block must be an object", { block });
  }

  if (typeof block.hash !== "string" || block.hash.trim() === "") {
    throw new ValidationError("Block must have a valid hash string", { block });
  }

  if (typeof block.height !== "number" || block.height < 0) {
    throw new ValidationError("Block must have a valid height number", {
      block,
    });
  }

  if (!Array.isArray(block.transactions)) {
    throw new ValidationError("Block must have a transactions array", {
      block,
    });
  }

  return true;
}

/**
 * Validates that an array is not empty
 * @param array - Array to validate
 * @param arrayName - Name of the array for error message
 * @throws ValidationError if array is empty
 */
function validateNonEmptyArray<T>(array: T[], arrayName: string): void {
  if (array.length === 0) {
    throw new ValidationError(`${arrayName} cannot be empty`);
  }
}

/**
 * Updates test data files in the specified folder
 * @param folderPath - Path to the test data folder
 * @param dataFile - Path to the data file containing blocks
 */
export function updateTestDataFiles(
  folderPath: string,
  sourceBlockDataFile: string,
): void {
  try {
    // Validate input parameters
    if (!folderPath || typeof folderPath !== "string") {
      throw new ValidationError("folderPath must be a non-empty string", {
        folderPath,
      });
    }

    if (!sourceBlockDataFile || typeof sourceBlockDataFile !== "string") {
      throw new ValidationError(
        "sourceBlockDataFile must be a non-empty string",
        { sourceBlockDataFile },
      );
    }

    // Read source block data file
    const sourceBlockData = readFileContent(sourceBlockDataFile);

    updateBlockDataFile(folderPath, sourceBlockData);
    updateTransactionDataFile(folderPath, sourceBlockData);
    updateContractDataFile(folderPath, sourceBlockData);
    updateUnshieldedTokenTypesDataFile(folderPath, sourceBlockData);

    console.info("[INFO ] - All test data files updated successfully");
  } catch (error) {
    if (error instanceof TestDataHandlerError) {
      console.error(`[ERROR] - ${error.name}: ${error.message}`, error.context);
      throw error;
    }
    console.error(
      "[ERROR] - Unexpected error updating test data files:",
      error,
    );
    throw new TestDataHandlerError("Failed to update test data files", {
      originalError: error,
    });
  }
}

/**
 * Safely reads file content with error handling
 * @param filePath - Path to the file to read
 * @returns File content as string
 * @throws FileOperationError if file cannot be read
 */
function readFileContent(filePath: string): string {
  try {
    if (!fs.existsSync(filePath)) {
      throw new FileOperationError(`File not found: ${filePath}`, filePath);
    }
    return fs.readFileSync(filePath, "utf8");
  } catch (error) {
    if (error instanceof FileOperationError) {
      throw error;
    }
    throw new FileOperationError(`Failed to read file: ${filePath}`, filePath, {
      originalError: error,
    });
  }
}

/**
 * Parses block data from JSONL format
 * @param sourceBlockData - Source data containing blocks in JSONL format
 * @returns Array of parsed block objects
 * @throws ParseError if JSON parsing fails
 * @throws ValidationError if block data is invalid
 */
function parseBlockData(sourceBlockData: string): Block[] {
  try {
    const lines = sourceBlockData
      .split("\n")
      .filter((line) => line.trim() !== "");

    if (lines.length === 0) {
      throw new ParseError(
        "No valid data lines found in source block data",
        sourceBlockData,
      );
    }

    const blocks: Block[] = [];

    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];
      try {
        const block = JSON.parse(line);
        validateBlock(block);
        blocks.push(block as Block);
      } catch (error) {
        if (error instanceof ValidationError) {
          throw error;
        }
        throw new ParseError(`Failed to parse block at line ${i + 1}`, line, {
          lineNumber: i + 1,
          originalError: error,
        });
      }
    }

    return blocks;
  } catch (error) {
    if (error instanceof TestDataHandlerError) {
      throw error;
    }
    throw new ParseError("Failed to parse block data", sourceBlockData, {
      originalError: error,
    });
  }
}

/**
 * Ensures the target directory exists, creates it if necessary
 * @param folderPath - Base folder path
 * @returns The full path to the target directory
 * @throws FileOperationError if directory cannot be created
 */
function ensureTargetDirectory(folderPath: string): string {
  try {
    const targetDir = path.join(folderPath, `${TARGET_ENV}`);

    if (!fs.existsSync(targetDir)) {
      fs.mkdirSync(targetDir, { recursive: true });
      console.info(`[INFO ] - Created directory: ${targetDir}`);
    }

    return targetDir;
  } catch (error) {
    throw new FileOperationError(
      `Failed to create directory: ${folderPath}/${TARGET_ENV}`,
      path.join(folderPath, `${TARGET_ENV}`),
      { originalError: error },
    );
  }
}

/**
 * Builds all necessary file paths for a given file name
 * @param targetDir - Target directory path
 * @param fileName - Name of the file
 * @returns Object containing target and template file paths
 */
function buildFilePaths(
  targetDir: string,
  fileName: string,
): {
  targetFilePath: string;
  templateFilePath: string;
} {
  return {
    targetFilePath: path.join(targetDir, fileName),
    templateFilePath: path.join(__dirname, "../templates", fileName),
  };
}

/**
 * Loads a template file and parses it as JSONC
 * @param templateFilePath - Path to the template file
 * @returns Parsed template object
 * @throws FileOperationError if template file not found or cannot be read
 * @throws ParseError if template cannot be parsed
 */
function loadTemplateFile<T>(templateFilePath: string): T {
  try {
    if (!fs.existsSync(templateFilePath)) {
      throw new FileOperationError(
        `Template file not found: ${templateFilePath}`,
        templateFilePath,
      );
    }

    const templateContent = readFileContent(templateFilePath);

    try {
      return commentJson.parse(templateContent) as T;
    } catch (error) {
      throw new ParseError(
        `Failed to parse template file: ${templateFilePath}`,
        templateContent,
        { originalError: error },
      );
    }
  } catch (error) {
    if (error instanceof TestDataHandlerError) {
      throw error;
    }
    throw new FileOperationError(
      `Failed to load template file: ${templateFilePath}`,
      templateFilePath,
      { originalError: error },
    );
  }
}

/**
 * Writes data to a JSON file and logs the operation
 * @param filePath - Path to write the file
 * @param data - Data to write
 * @param logMessage - Message to log after successful write
 * @throws FileOperationError if file cannot be written
 */
function writeJsonFile<T>(filePath: string, data: T, logMessage: string): void {
  try {
    const jsonContent = commentJson.stringify(data, null, 2);
    fs.writeFileSync(filePath, jsonContent, "utf8");
    console.info(`[INFO ] - ${logMessage}`);
  } catch (error) {
    throw new FileOperationError(
      `Failed to write file: ${filePath}`,
      filePath,
      { originalError: error },
    );
  }
}

/**
 * Helper function to filter transactions by type from block data
 * @param sourceBlockData - Source data containing blocks
 * @param transactionTypeName - The transaction type to filter by (e.g., "RegularTransaction", "SystemTransaction")
 * @param includeFields - Optional array of field names to include in transactions. If empty, all fields are included.
 * @returns Array of transactions of the specified type
 */
function filterTransactionsByType(
  sourceBlockData: string,
  transactionTypeName: string,
  includeFields: string[] = [],
): Transaction[] {
  const blocks: Block[] = parseBlockData(sourceBlockData);

  return blocks.flatMap((block: Block) => {
    return block.transactions
      .filter((transaction: TransactionWithType) => {
        return transaction.__typename === transactionTypeName;
      })
      .map((transaction: TransactionWithType) => {
        // If includeFields is specified, only include those fields
        if (includeFields.length > 0) {
          const filteredTransaction: Partial<TransactionWithType> = {};
          includeFields.forEach((field: string) => {
            if (Object.prototype.hasOwnProperty.call(transaction, field)) {
              (filteredTransaction as Record<string, any>)[field] = (
                transaction as Record<string, any>
              )[field];
            }
          });
          return filteredTransaction as Transaction;
        }
        return transaction as Transaction;
      });
  });
}

/**
 * Updates the block data file: if any part of the data is not available
 * it will be filled with "<N/A>"
 *
 * The way the block data file is filled works this way:
 * 1. We have the genesis block hash
 * 2. We have up to 100 blocks starting from the latest going backwards
 *
 * The file looks something like this:
 *
 * {
 *   "genesis": "004ce01767cefd51cd29668a1df90ddce577a7409ccde7bcb225b5fedfc16f72",
 *   "other-blocks": [
 *     "004ce01767cefd51cd29668a1df90ddce577a7409ccde7bcb225b5fedfc16f72",
 *     "004ce01767cefd51cd29668a1df90ddce577a7409ccde7bcb225b5fedfc16f72",
 *   ]
 *   "latest": "004ce01767cefd51cd29668a1df90ddce577a7409ccde7bcb225b5fedfc16f72",
 * }
 *
 * @param folderPath - Path to the test data folder
 * @param sourceBlockData - Path to the data file containing blocks
 */
function updateBlockDataFile(
  folderPath: string,
  sourceBlockData: string,
): void {
  try {
    // Parse the data and extract all block hashes
    const blocks: Block[] = parseBlockData(sourceBlockData);
    validateNonEmptyArray(blocks, "Blocks array");

    const inputDataArray: string[] = blocks.map((block) => block.hash);

    // Ensure the target directory exists before writing
    const targetDir: string = ensureTargetDirectory(folderPath);

    // Build file paths
    const targetFileName = `blocks.jsonc`;
    const { targetFilePath, templateFilePath } = buildFilePaths(
      targetDir,
      targetFileName,
    );

    // Load template and update data
    const dataObject: BlockDataFile =
      loadTemplateFile<BlockDataFile>(templateFilePath);

    const maxBlocks = 100;
    const startIndex = Math.max(0, inputDataArray.length - maxBlocks);

    dataObject.latest = inputDataArray[inputDataArray.length - 1];
    dataObject["other-blocks"] = inputDataArray.slice(startIndex);

    // Write the data to the target folder
    writeJsonFile<BlockDataFile>(
      targetFilePath,
      dataObject,
      `Block data file updated: ${folderPath}/${TARGET_ENV}/blocks.jsonc`,
    );
  } catch (error) {
    if (error instanceof TestDataHandlerError) {
      throw error;
    }
    throw new TestDataHandlerError("Failed to update block data file", {
      folderPath,
      originalError: error,
    });
  }
}

/**
 * Updates the transaction data file: if any part of the data is not available
 * it will be filled with "<N/A>"
 *
 * @param folderPath - Path to the test data folder
 * @param sourceBlockData - Path to the data file containing blocks
 */
function updateTransactionDataFile(
  folderPath: string,
  sourceBlockData: string,
): void {
  try {
    // Ensure the target directory exists before writing
    const targetDir: string = ensureTargetDirectory(folderPath);

    // Build file paths
    const targetFileName = `transactions.jsonc`;
    const { targetFilePath, templateFilePath } = buildFilePaths(
      targetDir,
      targetFileName,
    );

    // Load template and update data
    const dataObject: TransactionDataFile =
      loadTemplateFile<TransactionDataFile>(templateFilePath);

    dataObject["regular-transactions"] = filterTransactionsByType(
      sourceBlockData,
      "RegularTransaction",
      ["hash", "identifiers"], // include only these fields in the final records
    );
    dataObject["system-transactions"] = filterTransactionsByType(
      sourceBlockData,
      "SystemTransaction",
      ["hash", "identifiers"], // include only these fields in the final records
    );

    // Write the data to the target folder
    writeJsonFile<TransactionDataFile>(
      targetFilePath,
      dataObject,
      `Transaction data file updated: ${folderPath}/${TARGET_ENV}/transactions.jsonc`,
    );
  } catch (error) {
    if (error instanceof TestDataHandlerError) {
      throw error;
    }
    throw new TestDataHandlerError("Failed to update transaction data file", {
      folderPath,
      originalError: error,
    });
  }
}

/**
 * Updates the contract data file
 *
 * This file has a strong requirement, it will contain only contracts that have
 * all 3 action types: ContractDeploy, ContractCall, ContractUpdate
 *
 * If not such contracts exist, the file will contain an empty array
 *
 * @param destinationPath - Path to the test data folder
 * @param sourceBlockData - Path to the data file containing blocks
 */
function updateContractDataFile(
  destinationPath: string,
  sourceBlockData: string,
): void {
  try {
    // Parse blocks and extract contract actions with their metadata
    const blocks: Block[] = parseBlockData(sourceBlockData);
    validateNonEmptyArray(blocks, "Blocks array");

    // Map to group contract actions by address
    const contractActionsMap: ContractActionsMap = {};

    // Iterate over blocks and extract contract actions
    for (const block of blocks) {
      for (const transaction of block.transactions) {
        if (
          transaction.__typename === "RegularTransaction" &&
          transaction.contractActions
        ) {
          for (const contractAction of transaction.contractActions) {
            const address: string = contractAction.address;
            const actionType: string = contractAction.__typename;

            if (!contractActionsMap[address]) {
              contractActionsMap[address] = [];
            }

            contractActionsMap[address].push({
              "action-type": actionType,
              "block-height": block.height,
              "block-hash": block.hash,
            });
          }
        }
      }
    }

    // Filter to only keep addresses that have all 3 action types
    const requiredActionTypes: string[] = ["ContractDeploy", "ContractCall"];
    const filteredContracts: ContractWithActions[] = Object.entries(
      contractActionsMap,
    )
      .filter(([address, actions]: [string, ContractActionEntry[]]) => {
        const actionTypes: Set<string> = new Set(
          actions.map((action: ContractActionEntry) => action["action-type"]),
        );
        return requiredActionTypes.every((type: string) =>
          actionTypes.has(type),
        );
      })
      .map(
        ([address, actions]: [
          string,
          ContractActionEntry[],
        ]): ContractWithActions => ({
          "contract-address": address,
          "contract-actions": actions,
        }),
      );

    // Log if no contracts match the criteria
    if (filteredContracts.length === 0) {
      console.info(
        "[INFO ] - No contracts found with all required action types (ContractDeploy, ContractCall)",
      );
    }

    // Ensure the target directory exists
    const targetDir: string = ensureTargetDirectory(destinationPath);

    // Build file paths
    const targetFileName = `contract-actions.jsonc`;
    const { targetFilePath, templateFilePath } = buildFilePaths(
      targetDir,
      targetFileName,
    );

    // Load template and populate with data
    const templateArray: ContractActionsDataFile =
      loadTemplateFile<ContractActionsDataFile>(templateFilePath);

    // Clear the template array and populate with actual data to preserve comments
    templateArray.length = 0;
    if (filteredContracts.length > 0) {
      templateArray.push(...filteredContracts);
    }

    // Write the data to the target folder
    writeJsonFile<ContractActionsDataFile>(
      targetFilePath,
      templateArray,
      `Contract actions data file updated: ${destinationPath}/${TARGET_ENV}/contract-actions.jsonc`,
    );
  } catch (error) {
    if (error instanceof TestDataHandlerError) {
      throw error;
    }
    throw new TestDataHandlerError(
      "Failed to update contract actions data file",
      { destinationPath, originalError: error },
    );
  }
}

/**
 * Compares two decimal token amounts, highest first. The values are u128 on chain,
 * so they are compared as BigInt rather than as Number.
 */
function compareAmountsDescending(a: string, b: string): number {
  const left = BigInt(a);
  const right = BigInt(b);
  return left === right ? 0 : left > right ? -1 : 1;
}

/**
 * Aggregates the unshielded UTXOs of the scanned blocks into the token-type
 * candidates: every created output is grouped by token type and owner, keeping the
 * ones still unspent, with the custom token types ordered by their largest single
 * owner holding so the strongest candidate comes first.
 *
 * A UTXO counts as spent either because the indexer resolved its
 * `spentAtTransaction` when the block was streamed, or because the scan itself saw
 * it among a later transaction's spent outputs — the latter also covers the window
 * in which the indexer has not yet linked the spend.
 *
 * @param sourceBlockData - The data containing the scanned blocks
 * @returns The aggregated token types, ready to be written to the fixture
 */
function collectUnshieldedTokenTypes(
  sourceBlockData: string,
): UnshieldedTokenTypesDataFile {
  const blocks: Block[] = parseBlockData(sourceBlockData);
  validateNonEmptyArray(blocks, "Blocks array");

  // (intentHash, outputIndex) identifies an unshielded UTXO
  const utxoKey = (utxo: UnshieldedUtxo): string =>
    `${utxo.intentHash}:${utxo.outputIndex}`;

  const createdUtxos: Map<string, UnshieldedUtxo> = new Map();
  const spentKeys: Set<string> = new Set();

  for (const block of blocks) {
    for (const transaction of block.transactions) {
      if (transaction.__typename !== "RegularTransaction") {
        continue;
      }
      for (const utxo of transaction.unshieldedCreatedOutputs ?? []) {
        createdUtxos.set(utxoKey(utxo), utxo);
        if (utxo.spentAtTransaction) {
          spentKeys.add(utxoKey(utxo));
        }
      }
      for (const utxo of transaction.unshieldedSpentOutputs ?? []) {
        spentKeys.add(utxoKey(utxo));
      }
    }
  }

  const holdingsByToken: Map<
    string,
    Map<string, { value: bigint; utxos: number }>
  > = new Map();
  let nightUtxos = 0;
  const nightOwners: Set<string> = new Set();

  for (const [key, utxo] of createdUtxos) {
    if (spentKeys.has(key)) {
      continue;
    }
    if (utxo.tokenType === NIGHT_TOKEN_TYPE) {
      nightUtxos++;
      nightOwners.add(utxo.owner);
      continue;
    }

    let holdings = holdingsByToken.get(utxo.tokenType);
    if (!holdings) {
      holdings = new Map();
      holdingsByToken.set(utxo.tokenType, holdings);
    }
    const held = holdings.get(utxo.owner) ?? { value: 0n, utxos: 0 };
    holdings.set(utxo.owner, {
      value: held.value + BigInt(utxo.value),
      utxos: held.utxos + 1,
    });
  }

  const customTokens: CustomTokenType[] = [...holdingsByToken.entries()]
    .map(([tokenType, holdings]) => ({
      "token-type": tokenType,
      "unspent-holders": [...holdings.entries()]
        .map(([owner, held]) => ({
          owner,
          "unspent-value": held.value.toString(),
          "unspent-utxos": held.utxos,
        }))
        .sort((a, b) =>
          compareAmountsDescending(a["unspent-value"], b["unspent-value"]),
        ),
    }))
    .sort((a, b) =>
      compareAmountsDescending(
        a["unspent-holders"][0]["unspent-value"],
        b["unspent-holders"][0]["unspent-value"],
      ),
    );

  // Reduced rather than spread into Math.min/max: a long scan holds more blocks
  // than a call can take arguments.
  const heights: number[] = blocks.map((block) => block.height);

  return {
    scan: {
      "from-block": heights.reduce((min, height) => Math.min(min, height)),
      "to-block": heights.reduce((max, height) => Math.max(max, height)),
      "scanned-at": new Date().toISOString(),
    },
    night: {
      "unspent-utxos": nightUtxos,
      "distinct-owners": nightOwners.size,
    },
    "custom-tokens": customTokens,
  };
}

/**
 * Updates the unshielded token types data file, the fixture the custom unshielded
 * token e2e suite picks a spendable token from.
 *
 * The data is aggregated from the scanned blocks alone, so an environment whose
 * chain carries no custom unshielded token produces an empty candidate list and the
 * suite that needs one skips cleanly.
 *
 * @param destinationPath - Path to the test data folder
 * @param sourceBlockData - The data containing the scanned blocks
 */
function updateUnshieldedTokenTypesDataFile(
  destinationPath: string,
  sourceBlockData: string,
): void {
  try {
    const tokenTypes = collectUnshieldedTokenTypes(sourceBlockData);

    if (tokenTypes["custom-tokens"].length === 0) {
      console.info(
        "[INFO ] - No custom (non-NIGHT) unshielded token types found in the " +
          "scanned blocks; writing an empty candidate list",
      );
    }

    // Ensure the target directory exists
    const targetDir: string = ensureTargetDirectory(destinationPath);

    // Build file paths
    const targetFileName = `unshielded-token-types.jsonc`;
    const { targetFilePath, templateFilePath } = buildFilePaths(
      targetDir,
      targetFileName,
    );

    // Load template and populate with data to preserve comments
    const dataObject: UnshieldedTokenTypesDataFile =
      loadTemplateFile<UnshieldedTokenTypesDataFile>(templateFilePath);

    dataObject.scan = tokenTypes.scan;
    dataObject.night = tokenTypes.night;
    dataObject["custom-tokens"] = tokenTypes["custom-tokens"];

    // Write the data to the target folder
    writeJsonFile<UnshieldedTokenTypesDataFile>(
      targetFilePath,
      dataObject,
      `Unshielded token types data file updated: ${destinationPath}/${TARGET_ENV}/unshielded-token-types.jsonc`,
    );
  } catch (error) {
    if (error instanceof TestDataHandlerError) {
      throw error;
    }
    throw new TestDataHandlerError(
      "Failed to update unshielded token types data file",
      { destinationPath, originalError: error },
    );
  }
}
