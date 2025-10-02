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
import { TARGET_ENV } from "./env.js";
import { Transaction } from "./indexer-types.js";

/**
 * Updates test data files in the specified folder
 * @param folderPath - Path to the test data folder
 * @param dataFile - Path to the data file containing blocks
 */
export function updateTestDataFiles(
  folderPath: string,
  sourceBlockDataFile: string,
): void {
  // List the files in the folder
  const targetFolder = `${folderPath}/${TARGET_ENV}`;
  const sourceBlockData = fs.readFileSync(sourceBlockDataFile, "utf8");

  updateTransactionDataFile(folderPath, sourceBlockData);
  updateContracDataFile(folderPath, sourceBlockData);
}

function updateTransactionDataFile(
  folderPath: string,
  sourceBlockData: string,
): void {
  // Parse the data making sure the line is not empty and only
  // filter the blocks that contain RegularTransactions
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
          return transaction.__typename === "RegularTransaction";
        });
      }
      return false;
    });

  console.info(
    `[INFO ] - Transaction data file updated: ${folderPath}/${TARGET_ENV}/transactions.json`,
  );
}

function updateContracDataFile(
  destinationPath: string,
  sourceBlockData: string,
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
    JSON.stringify(contractActionsMap, null, 2),
  );

  console.info(
    `[INFO ] - Contract actions data file updated: ${destinationPath}/${TARGET_ENV}/contracts-actions.json`,
  );
}
