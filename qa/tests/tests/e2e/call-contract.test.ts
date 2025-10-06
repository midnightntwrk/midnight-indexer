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

import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";

import { ToolkitWrapper } from "../../utils/toolkit/toolkit-wrapper";

describe("call contract via toolkit wrapper", () => {
  it(
    "makes a complete contract call and verifies it appears as ContractCall in GraphQL",
    async () => {
      const t0 = Date.now();

      // Use a unique /out dir so artifacts are easy to inspect if needed.
      const outDir = fs.mkdtempSync(path.join(os.tmpdir(), "toolkit-call-"));

      const wrapper = new ToolkitWrapper({
        targetDir: outDir, // mounts to /out in the running toolkit container
        // nodeTag, containerName, etc. are auto-filled from env by the wrapper
      });

      try {
        await wrapper.start();

        // Step 1: Use existing Merkle Tree Contract address
        const contractAddress = "e51d090bd7eda55742d0fc4e8143311c7534bc1da7eb36b45b01007321a1aac2";
        console.log(`Using existing contract at: ${contractAddress}`);

        // Step 2: Generate the contract call transaction
        console.log("Generating contract call transaction...");
        const callRes = await wrapper.callContract({
          contractAddress,
          methodName: "increment", // Counter contract has increment method
        });

        console.log(`Generated call transaction: ${callRes.callTxPath}`);

        // Step 3: Send the contract call transaction to the network
        console.log("Sending transaction to network...");
        const sendResult = await wrapper.startedContainer!.exec([
          "/midnight-node-toolkit",
          "generate-txs",
          "--src-files",
          `/out/${path.basename(callRes.callTxPath)}`,
          "--dest-url",
          "ws://127.0.0.1:9944",
          "send"
        ]);

        if (sendResult.exitCode !== 0) {
          throw new Error(`Failed to send contract call: ${sendResult.stderr || sendResult.output}`);
        }

        console.log("Contract call transaction sent to network");

        // Step 4: Wait for the transaction to be processed and indexed
        console.log("Waiting for transaction to be processed and indexed...");
        await new Promise(resolve => setTimeout(resolve, 45000)); // 45 seconds wait time

        // Step 5: Query the GraphQL API to verify we get ContractCall
        console.log("Querying GraphQL API...");
        const response = await fetch('http://localhost:8088/api/v1/graphql', {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
          },
          body: JSON.stringify({
            query: `
              query GetContractAction($ADDRESS: String!) {
                contractAction(address: $ADDRESS) {
                  __typename
                  address
                  ... on ContractCall {
                    deploy {
                      address
                      unshieldedBalances {  
                        tokenType
                        amount
                      }
                    }
                    entryPoint
                    unshieldedBalances {
                      tokenType
                      amount
                    }
                  }
                }
              }
            `,
            variables: {
              ADDRESS: contractAddress
            }
          })
        });

        if (!response.ok) {
          throw new Error(`GraphQL request failed: ${response.status} ${response.statusText}`);
        }

        const result = await response.json();
        console.log("GraphQL Response:", JSON.stringify(result, null, 2));

        // Step 6: Verify we get ContractCall, not ContractDeploy
        expect(result.data.contractAction.__typename).toBe("ContractCall");
        expect(result.data.contractAction.entryPoint).toBe("increment");

        const ms = Date.now() - t0;
        console.log(`Complete contract call test completed in ${ms}ms`);
      } finally {
        await wrapper.stop();
      }
    },
    180_000, // Increased timeout for complete flow
  );
});