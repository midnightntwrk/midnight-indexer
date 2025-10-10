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

import fs from "fs";
import path from "path";
import * as commentJson from "comment-json";
import { TARGET_ENV } from "./env.js";
import { Transaction } from "./indexer-types.js";

/**
 * Updates test data files in the specified folder
 * @param folderPath - Path to the test data folder
 * @param dataFile - Path to the data file containing blocks
 */
export function updateTestDataFiles(
  folderPath: string,
  sourceBlockDataFile: string
): void {
  // List the files in the folder
  const sourceBlockData = fs.readFileSync(sourceBlockDataFile, "utf8");

  updateBlockDataFile(folderPath, sourceBlockData);
  updateTransactionDataFile(folderPath, sourceBlockData);
  //updateContracDataFile(folderPath, sourceBlockData);
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
  sourceBlockData: string
): void {
  // Parse the data and extract all block hashes
  const inputDataArray = sourceBlockData
    .split("\n")
    .filter((line) => line.trim() !== "")
    .map((line) => {
      const block = JSON.parse(line);
      return block.hash;
    });

  // Ensure the target directory exists before writing
  const targetDir = path.join(folderPath, `${TARGET_ENV}`);
  if (!fs.existsSync(targetDir)) {
    fs.mkdirSync(targetDir, { recursive: true });
  }

  // Create the data object
  const startIndex = inputDataArray.length - 100;
  const endIndex = -1;
  
  const targetFileName = `blocks.jsonc`;
  const targetFilePath = path.join(targetDir, targetFileName);
  const templateFilePath = path.join(__dirname, '../templates', targetFileName);
  
  let dataObject: any;
  
  // Always use template as the source of truth for structure and comments
  if (fs.existsSync(templateFilePath)) {
    // Use template file which has comments
    const templateContent = fs.readFileSync(templateFilePath, 'utf8');
    dataObject = commentJson.parse(templateContent);
  } else {
    throw new Error(`Template ${templateFilePath} file not found`);
  }
  
  // Update the data
  dataObject.latest = inputDataArray[0];
  dataObject["other-blocks"] = inputDataArray.slice(startIndex, endIndex);
  
  const jsonContent = commentJson.stringify(dataObject, null, 2);

  // Write the data to the target folder
  fs.writeFileSync(targetFilePath, jsonContent);

  console.info(
    `[INFO ] - Block data file updated: ${folderPath}/${TARGET_ENV}/blocks.jsonc`
  );
}

/**
 * Helper function to filter transactions by type from block data
 * @param sourceBlockData - Source data containing blocks
 * @param transactionTypeName - The transaction type to filter by (e.g., "RegularTransaction", "SystemTransaction")
 * @param excludeFields - Optional array of field names to exclude from transactions
 * @returns Array of transactions of the specified type
 */
function filterTransactionsByType(
  sourceBlockData: string,
  transactionTypeName: string,
  excludeFields: string[] = []
): Transaction[] {
  return sourceBlockData
    .split("\n")
    .filter((line) => line.trim() !== "")
    .map((line) => JSON.parse(line))
    .flatMap((block) => {
      return block.transactions
        .filter((transaction: Transaction | any) => {
          return transaction.__typename === transactionTypeName;
        })
        .map((transaction: any) => {
          // Remove excluded fields if specified
          if (excludeFields.length > 0) {
            const filteredTransaction = { ...transaction };
            excludeFields.forEach((field) => {
              delete filteredTransaction[field];
            });
            return filteredTransaction;
          }
          return transaction;
        });
    });
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
  sourceBlockData: string
): void {

  // Ensure the target directory exists before writing
  const targetDir = path.join(folderPath, `${TARGET_ENV}`);
  if (!fs.existsSync(targetDir)) {
    fs.mkdirSync(targetDir, { recursive: true });
  }

  const targetFileName = `transactions.jsonc`;
  const targetFilePath = path.join(targetDir, targetFileName);
  const templateFilePath = path.join(__dirname, '../templates', targetFileName);

  // Placeholder for the final data object
  let dataObject: any;

  // Always use template as the source of truth for structure and comments
  if (fs.existsSync(templateFilePath)) {
    // Use template file which has comments
    const templateContent = fs.readFileSync(templateFilePath, 'utf8');
    dataObject = commentJson.parse(templateContent);
  } else {
    throw new Error(`Template ${templateFilePath} file not found`);
  }

  // Then update the destination data file from the template and the filtered transactions
  dataObject["regular-transactions"] = filterTransactionsByType(
    sourceBlockData,
    "RegularTransaction",
    ["protocolVersion", "contractActions"] // exclude these fields
  );
  dataObject["system-transactions"] = filterTransactionsByType(
    sourceBlockData,
    "SystemTransaction",
    ["protocolVersion", "contractActions"] // exclude these fields
  );

  const jsonContent = commentJson.stringify(dataObject, null, 2);

  // Write the data to the target folder
  fs.writeFileSync(targetFilePath, jsonContent);

  console.info(
    `[INFO ] - Transaction data file updated: ${folderPath}/${TARGET_ENV}/transactions.jsonc`
  );
}

/**
 * Updates the contract data file: if any part of the data is not available
 * it will be filled with "<N/A>"
 *
 * @param destinationPath - Path to the test data folder
 * @param sourceBlockData - Path to the data file containing blocks
 */
function updateContracDataFile(
  destinationPath: string,
  sourceBlockData: string
): void {
  // Parse the data making sure the line is not empty and only
  // filter the blocks that contain contract actions
  const dataArray = sourceBlockData
    .split("\n")
    .map((line) => {
      if (line.trim() !== "") {
        return JSON.parse(line);
      }
    })
    .filter((block) => {
      if (block !== undefined) {
        // Not all the transactions are RegularTransaction, so we need to filter them out
        // SystemTeransactions don't have contractActions
        return block.transactions.some((transaction: Transaction | any) => {
          if (transaction.__typename === "RegularTransaction") {
            return transaction.contractActions.length > 0;
          }
          return false;
        });
      }
      return false;
    });

  // The contract actions data structure will hold the address and the contract actions
  // with the height of the block where the contract action was executed
  const contractActionsMap: {
    [key: string]: { [key: string]: { height: number; hash: string }[] };
  } = {};

  // Iterate over the dataArray and find the contract actions, storing the height
  // and the hash of the block where the contract action was executed
  for (const block of dataArray) {
    for (const transaction of block.transactions) {
      if (transaction.__typename === "RegularTransaction") {
        for (const contractAction of transaction.contractActions) {
          const address: string = contractAction.address;
          const contractActionType: string = contractAction.__typename;
          if (!contractActionsMap[address]) {
            contractActionsMap[address] = {
              ContractDeploy: [],
              ContractCall: [],
              ContractUpdate: [],
            };
          }
          const current = contractActionsMap[address];
          current[contractActionType.trim() as keyof typeof current].push({
            height: block.height,
            hash: block.hash,
          });
        }
      }
    }
  }

  // Write the data to the target folder
  fs.writeFileSync(
    path.join(destinationPath, `${TARGET_ENV}`, `contracts-actions.json`),
    JSON.stringify(contractActionsMap, null, 2)
  );

  console.info(
    `[INFO ] - Contract actions data file updated: ${destinationPath}/${TARGET_ENV}/contracts-actions.json`
  );
}
