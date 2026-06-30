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

/**
 * Probes whether the deployed indexer exposes the public contract-events surface
 * (the `ContractEvent` GraphQL type). The surface ships behind PR #1185 and is
 * not yet on every environment, so the contract-event integration tests gate on
 * this rather than a hardcoded environment list: they assert where the surface
 * exists and skip where it does not, tracking the feature as it rolls out.
 *
 * @returns true when the `ContractEvent` type is present in the schema.
 */
export async function contractEventsSurfacePresent(): Promise<boolean> {
  const query = `query { __type(name: "ContractEvent") { name kind } }`;
  try {
    const response = await fetch(env.getIndexerHttpBaseURL() + '/api/v4/graphql', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ query }),
      signal: AbortSignal.timeout(15_000),
    });
    const json = (await response.json()) as { data?: { __type: { name: string } | null } };
    const present = json.data?.__type?.name === 'ContractEvent';
    log.debug(`Contract events surface present on ${env.getCurrentEnvironmentName()}: ${present}`);
    return present;
  } catch (error) {
    log.warn(`Contract events surface probe failed: ${(error as Error).message}`);
    return false;
  }
}
