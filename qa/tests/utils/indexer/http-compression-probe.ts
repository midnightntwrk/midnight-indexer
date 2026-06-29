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

export interface CompressionProbeResult {
  status: number;
  // The raw Content-Encoding header value, or null if the server sent none.
  contentEncoding: string | null;
  data: unknown;
}

// Large enough to exceed tower-http's default minimum-size threshold for
// compression (~1 KiB). A minimal domain query (e.g. `{ block { hash } }`)
// can return a body smaller than the threshold, causing the server to skip
// compression even when the client advertises Accept-Encoding — which would
// make the gzip/br/zstd assertions below fail spuriously.
const INTROSPECTION_QUERY = `{
  __schema {
    types {
      name
      fields {
        name
        type { name kind ofType { name kind } }
      }
    }
  }
}`;

/**
 * Sends a raw GraphQL POST and returns the HTTP status, Content-Encoding
 * header, and parsed body.
 *
 * Uses native fetch rather than graphql-request so that response headers
 * remain accessible — graphql-request parses and discards them before
 * surfacing the response to callers.
 *
 * Note on Node.js fetch (undici): undici automatically decompresses the
 * response body when Content-Encoding is present, so `response.json()` works
 * transparently regardless of the encoding. The Content-Encoding header is
 * still preserved in `response.headers` and reflects what the server actually
 * sent.
 *
 * Note on identity responses: undici unconditionally appends its own
 * `Accept-Encoding` (gzip, deflate, br) to every outgoing request. To test
 * the server's identity (uncompressed) path, pass `Accept-Encoding: identity`
 * explicitly — that overrides undici's default and instructs the server not
 * to compress.
 */
export async function probeGraphQLCompression(
  acceptEncoding?: string,
  query: string = INTROSPECTION_QUERY,
): Promise<CompressionProbeResult> {
  const apiVersion = process.env.INDEXER_API_VERSION?.trim() || 'v4';
  const url = `${env.getIndexerHttpBaseURL()}/api/${apiVersion}/graphql`;

  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
  };
  if (acceptEncoding !== undefined) {
    headers['Accept-Encoding'] = acceptEncoding;
  }

  const response = await fetch(url, {
    method: 'POST',
    headers,
    body: JSON.stringify({ query }),
    signal: AbortSignal.timeout(30_000),
  });

  const data = await response.json();

  return {
    status: response.status,
    contentEncoding: response.headers.get('content-encoding'),
    data,
  };
}
