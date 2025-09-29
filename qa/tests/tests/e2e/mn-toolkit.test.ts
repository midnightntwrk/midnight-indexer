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

import { randomBytes } from 'crypto';
import log from '@utils/logging/logger';
import { env } from '../../environment/model';
import '@utils/logging/test-logging-hooks';
import { ToolkitWrapper, ToolkitTransactionResult } from '@utils/toolkit/toolkit-wrapper';

// To run: yarn test e2e
describe('mn-toolkit', () => {
  let toolkit: ToolkitWrapper;
  const seed = randomBytes(32).toString('hex');

  beforeAll(async () => {
    toolkit = new ToolkitWrapper({});
    await toolkit.start();
  });

  afterAll(async () => {
    await toolkit.stop();
  });

  test('mn-toolkit show shielded address test', async () => {
    const address = await toolkit.showAddress(seed, 'shielded');

    log.info(`Shielded address: ${address}`);

    expect(address).toMatch(/^mn_shield-addr_/);
  });

  test('mn-toolkit show unshielded address test', async () => {
    const address = await toolkit.showAddress(seed, 'unshielded');

    log.info(`Unshielded address: ${address}`);

    expect(address).toMatch(/^mn_addr_/);
  });

  test('mn-toolkit show viewing key test', async () => {
    const viewingKey = await toolkit.showViewingKey(seed);

    log.info(`Viewing key: ${viewingKey}`);

    expect(viewingKey).toMatch(/^mn_shield-esk_/);
  });
});
