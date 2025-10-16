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

import os from 'os';
import fs from 'fs';
import path from 'path';
import { TestContext } from 'vitest';
import log from '@utils/logging/logger';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { ToolkitWrapper, DeployContractResult } from '@utils/toolkit/toolkit-wrapper';

// Use a unique /out dir so artifacts are easy to inspect if needed.
const outDir = fs.mkdtempSync(path.join(os.tmpdir(), 'toolkit-deploy-'));

describe('contract actions', () => {
  let toolkit: ToolkitWrapper;
  let result: DeployContractResult;

  beforeAll(async () => {
    toolkit = new ToolkitWrapper({
      targetDir: outDir, // mounts to /out in the running toolkit container
    });

    await toolkit.start();

    result = await toolkit.deployContract();
  }, 300_000);

  afterAll(async () => {
    await toolkit.stop();
  });

  describe('a midnight contract successfully deployed on chain', async () => {
    test('should reported the address of the contract', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['ContractActions', 'ContractDeploy', 'Query', 'Toolkit'],
      };

      const contractAddressRaw = result.contractAddress;

      log.debug(`contractAddressRaw: ${contractAddressRaw}`);

      // Basic assertions
      expect(contractAddressRaw).toMatch(/^[0-9a-f]{64}$/i);
      expect(fs.existsSync(result.deployTxPath)).toBe(true);
      expect(fs.existsSync(result.statePath)).toBe(true);
    });

    test('should be reported by a contract query by address using the untagged address', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['ContractActions', 'ContractDeploy', 'Query', 'Toolkit'],
      };

      const contractAddressRaw = result.contractAddress;

      const response = await new IndexerHttpClient().getContractAction(contractAddressRaw);
      expect(response).toBeSuccess();
      expect(response.data?.contractAction).not.toBeNull();
      expect(response.data?.contractAction?.address).toBe(contractAddressRaw);
    });

    test('should not be reported by a contract query by address using the tagged address', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['ContractActions', 'ContractDeploy', 'Query', 'Toolkit', 'Negative'],
      };

      const contractAddressRaw = result.addressTagged;

      const response = await new IndexerHttpClient().getContractAction(contractAddressRaw);
      expect(response).toBeSuccess();
      expect(response.data?.contractAction).toBeNull();
    });
  });
});
