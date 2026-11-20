// This file is part of midnightntwrk/midnight-indexer.
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

// Host resolution for the environment model. Kept free of import-time side
// effects (no logger, no file reads) so the unit project can test it directly.

export type HostConfig = {
  indexerHost: string;
  nodeHost: string;
};

export type HostOverrides = {
  indexerHost?: string;
  nodeHost?: string;
};

/**
 * Hosts of the local stack, the only environment this repository targets.
 * Every other valid `TARGET_ENV` value has no built-in hosts: whoever runs
 * against it supplies `INDEXER_HOST` and `NODE_HOST`.
 */
const builtInHosts: Readonly<Record<string, HostConfig>> = {
  undeployed: { indexerHost: 'localhost:8088', nodeHost: 'localhost:9944' },
};

function readOverride(name: string, raw: string | undefined): string | undefined {
  const value = raw?.trim();
  if (!value) return undefined;
  if (value.includes('://') || value.includes('/')) {
    throw new Error(
      `Invalid ${name}="${raw}": expected a bare host with an optional port ` +
        `(for example "indexer.example.org" or "localhost:8088"), without a scheme or path.`,
    );
  }
  return value;
}

/**
 * Resolves the indexer and node hosts for an environment.
 *
 * `INDEXER_HOST` and `NODE_HOST` each take precedence over the built-in host
 * of the same kind, so they also redirect a run on the local stack.
 * An environment with no built-in hosts needs both.
 */
export function resolveHosts(envName: string, overrides: HostOverrides = {}): HostConfig {
  const builtIn = builtInHosts[envName];
  const indexerHost = readOverride('INDEXER_HOST', overrides.indexerHost) ?? builtIn?.indexerHost;
  const nodeHost = readOverride('NODE_HOST', overrides.nodeHost) ?? builtIn?.nodeHost;

  if (indexerHost && nodeHost) {
    return { indexerHost, nodeHost };
  }

  const missing = [!indexerHost && 'INDEXER_HOST', !nodeHost && 'NODE_HOST'].filter(Boolean);
  throw new Error(
    `TARGET_ENV="${envName}" has no built-in hosts. Set ${missing.join(' and ')} ` +
      `to the host (and optional port) to test against.`,
  );
}
