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

import log from '@utils/logging/logger';
import { env } from '../../environment/model';
import '@utils/logging/test-logging-hooks';

import { ToolkitWrapper, ToolkitTransactionResult } from '@utils/toolkit/toolkit-wrapper';
import { IndexerHttpClient } from '@utils/indexer/http-client';

// To run: yarn test e2e
describe('mn-toolkit', () => {
  let toolkit: ToolkitWrapper;

  beforeAll(async () => {
    toolkit = new ToolkitWrapper({
      containerName: `mn-toolkit-${env.getEnvName()}`,
      targetDir: '/tmp/toolkit/',
      chain: `${env.getEnvName()}`,
      nodeTag: '0.16.2-71d3d861',
    });
    await toolkit.start();
  });

  afterAll(async () => {
    await toolkit.stop();
  });

  test('mn-toolkit show shielded address test', async () => {
    const seed = '0000000000000000000000000000000000000000000000000000000000000001';

    const address = await toolkit.showAddress(seed, 'shielded');

    console.log('Shielded address:', address);

    expect(address).toMatch(/^mn_shield-addr_/);
  });

  test('mn-toolkit show unshielded address test', async () => {
    const seed = '0000000000000000000000000000000000000000000000000000000987654321';

    const address = await toolkit.showAddress(seed, 'unshielded');

    console.log('Unshielded address:', address);

    expect(address).toMatch(/^mn_addr_/);
  });

  test('mn-toolkit show viewing key test', async () => {
    const seed = '0000000000000000000000000000000000000000000000000000000987654321';

    const viewingKey = await toolkit.showViewingKey(seed);

    console.log('Viewing key:', viewingKey);

    expect(viewingKey).toMatch(/^mn_shield-esk_/);
  });

  // test('mn-toolkit submit single unshielded tx test', async () => {
  //   // const sourceSeed = '1113354e9a4fb7bff5e049929197acfcf6dcb4fc1ab3205d92ba9c21813c8906';
  //   const sourceSeed = '0000000000000000000000000000000000000000000000000000000000000001';
  //   const destinationSeed = '0000000000000000000000000000000000000000000000000000000987654321';

  //   const unshieldedAddress = await toolkit.showAddress(destinationSeed, 'unshielded');

  //   console.log('Destination unshielded address:', unshieldedAddress);

  //   const transactionResult: ToolkitTransactionResult = await toolkit.generateSingleTx(sourceSeed, 'unshielded', unshieldedAddress, .5);

  //   console.log('Block hash      :', transactionResult.blockHash);
  //   console.log('Transaction hash:', transactionResult.txHash);

  //   const block = await (new IndexerHttpClient()).getBlockByOffset({ hash: transactionResult.blockHash });

  //   console.log('Block:', JSON.stringify(block.data?.block, null, 2));

  // }, 60000);

  // test('mn-toolkit submit single shielded tx test', async () => {
  //   // const sourceSeed = '1113354e9a4fb7bff5e049929197acfcf6dcb4fc1ab3205d92ba9c21813c8906';
  //   const sourceSeed = '0000000000000000000000000000000000000000000000000000000000000001';
  //   const destinationSeed = '0000000000000000000000000000000000000000000000000000000987654321';

  //   const shieldedAddress = await toolkit.showAddress(destinationSeed, 'shielded');

  //   console.log('Destination shielded address:', shieldedAddress);

  //   const transactionResult: ToolkitTransactionResult = await toolkit.generateSingleTx(sourceSeed, 'shielded', shieldedAddress, 1);

  //   console.log('Block hash      :', transactionResult.blockHash);
  //   console.log('Transaction hash:', transactionResult.txHash);
  // }, 60000);
});
