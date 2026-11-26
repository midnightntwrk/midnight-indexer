// This file is part of midnightntwrk/midnight-indexer
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

import { BaseSequencer, type TestSpecification } from 'vitest/node';

// Test files that must run after every other file of their run, in this order.
//
// contract-state-history-after-gc.test.ts checks the contract history the other e2e files
// leave on the chain once gc passes have run past it. Run first, it would find none and have to
// make its own, then wait minutes for the gc passes; run last, the rest of the run has already
// paid for them.
const RUN_LAST = ['tests/e2e/contract-state-history-after-gc.test.ts'];

function runLastRank(spec: TestSpecification): number {
  const path = spec.moduleId.replaceAll('\\', '/');
  return RUN_LAST.findIndex((suffix) => path.endsWith(`/${suffix}`));
}

/**
 * Vitest's default order (failed first, then slowest first), with the RUN_LAST files moved to
 * the end. The order only holds between files that run one at a time, as e2e files do.
 */
export default class RunLastSequencer extends BaseSequencer {
  override async sort(files: TestSpecification[]): Promise<TestSpecification[]> {
    const sorted = await super.sort(files);
    const last = sorted
      .filter((spec) => runLastRank(spec) >= 0)
      .sort((a, b) => runLastRank(a) - runLastRank(b));
    return [...sorted.filter((spec) => runLastRank(spec) < 0), ...last];
  }
}
