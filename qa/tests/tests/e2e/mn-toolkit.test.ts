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
import '@utils/logging/test-logging-hooks';

import { showAddress } from '@utils/toolkit/src/show-address';

// To run: yarn test e2e
describe('mn-toolkit', () => {
  test('mn-toolkit test', async () => {
    const seed = '0000000000000000000000000000000000000000000000000000000000000001';

    const shielded = await showAddress({
      chain: 'undeployed',
      addressType: 'shielded',
      seed,
    });

    console.log('Shielded address:', shielded);
  });
});
