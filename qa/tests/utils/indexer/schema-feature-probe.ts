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

import { env } from 'environment/model';

const BLOCK_FIELD_DESCRIPTIONS_QUERY = `{
  __type(name: "Block") {
    fields {
      name
      description
    }
  }
}`;

interface IntrospectedField {
  name: string;
  description: string | null;
}

/**
 * Introspects the deployed schema and returns the description of a `Block` field,
 * or null when the field does not exist.
 *
 * Uses native fetch (pattern of http-compression-probe) because the typed
 * IndexerHttpClient methods are bound to domain queries, not introspection.
 */
export async function fetchBlockFieldDescription(fieldName: string): Promise<string | null> {
  const apiVersion = process.env.INDEXER_API_VERSION?.trim() || 'v4';
  const url = `${env.getIndexerHttpBaseURL()}/api/${apiVersion}/graphql`;

  const response = await fetch(url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ query: BLOCK_FIELD_DESCRIPTIONS_QUERY }),
    signal: AbortSignal.timeout(30_000),
  });

  const body = (await response.json()) as {
    data?: { __type?: { fields?: IntrospectedField[] } };
  };
  const field = body.data?.__type?.fields?.find((f) => f.name === fieldName);
  return field?.description ?? null;
}

/**
 * Whether the deployed indexer serves per-block dust Merkle tree roots.
 *
 * Up to and including 4.3.3 the `Block.dust*MerkleTreeRoot` fields are documented
 * (and resolved) "at the latest indexed state" — the tip's roots for every block.
 * Since the per-block change the deployed schema documents them "at this block".
 * The description is the only observable version marker: field name and type are
 * identical on both sides.
 */
export async function isPerBlockDustRootsSupported(): Promise<boolean> {
  const description = await fetchBlockFieldDescription('dustGenerationMerkleTreeRoot');
  return description !== null && description.includes('at this block');
}
