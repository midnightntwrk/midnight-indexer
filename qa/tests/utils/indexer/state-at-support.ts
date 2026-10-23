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

import log from '@utils/logging/logger';
import { env } from 'environment/model';
import { retry } from '@utils/retry-helper';

/**
 * Probes whether the deployed indexer exposes the `stateAt` field on contract actions. The field
 * ships with indexer 4.4 and is not yet on every environment, so the stateAt tests gate on this
 * rather than a hardcoded environment list.
 *
 * As for `contractEventsSurfacePresent`, a healthy schema response that lacks the field returns
 * `false` (the tests skip), while a probe that cannot reach a healthy answer throws after retries,
 * so a transient blip is never mistaken for "field absent".
 *
 * @returns true when `ContractAction.stateAt` is in the schema, false when a healthy response
 *          reports it absent.
 * @throws if the field's presence cannot be determined after retries.
 */
export async function stateAtFieldPresent(): Promise<boolean> {
  const query = `query { __type(name: "ContractAction") { fields { name } } }`;
  return retry(
    async () => {
      const response = await fetch(env.getIndexerHttpBaseURL() + '/api/v4/graphql', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ query }),
        signal: AbortSignal.timeout(15_000),
      });
      if (!response.ok) {
        throw new Error(`stateAt field probe got HTTP ${response.status}`);
      }
      const json = (await response.json()) as {
        data?: { __type: { fields: { name: string }[] } | null };
      };
      const present = json.data?.__type?.fields.some((field) => field.name === 'stateAt') ?? false;
      log.debug(`stateAt field present on ${env.getCurrentEnvironmentName()}: ${present}`);
      return present;
    },
    { maxRetries: 2, delayMs: 1000, retryLabel: 'stateAt field probe' },
  );
}
