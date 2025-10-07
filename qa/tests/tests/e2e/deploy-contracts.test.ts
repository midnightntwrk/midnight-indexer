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
//
//

import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";

import { ToolkitWrapper } from "../../utils/toolkit/toolkit-wrapper";

describe("deploy contracts via toolkit wrapper", () => {
  it(
    "deploys the sample counter contract and returns its address",
    async () => {
      const t0 = Date.now();

      // Use a unique /out dir so artifacts are easy to inspect if needed.
      const outDir = fs.mkdtempSync(path.join(os.tmpdir(), "toolkit-deploy-"));

      const wrapper = new ToolkitWrapper({
        targetDir: outDir, // mounts to /out in the running toolkit container
        // nodeTag, containerName, etc. are auto-filled from env by the wrapper
      });

      try {
        await wrapper.start();

        // Deploy using the wrapper
        const res = await wrapper.deployContract();
        const { toolkitImage, nodeContainer, network } = wrapper.runtime;

        const ms = Date.now() - t0;
                
        // One-liner summary; helpful but not noisy
        const contractAddressRaw =
        (res as any).addressRaw ??
        fs.readFileSync(path.join(outDir, "contract_address.mn"), "utf8").trim();

        console.log(
        `contract-address=${contractAddressRaw} | toolkit=${toolkitImage} | node=${nodeContainer} | network=${network} | tx=${path.basename(
         res.deployTxPath,
        )} | out=${outDir} | dur=${ms}ms`,
        );

        // Basic assertions
        expect(res.addressHex).toMatch(/^[0-9a-f]{64}$/i);
        expect(fs.existsSync(res.deployTxPath)).toBe(true);
        expect(fs.existsSync(res.statePath)).toBe(true);
      } finally {
        await wrapper.stop();
      }
    },
    120_000,
  );
});