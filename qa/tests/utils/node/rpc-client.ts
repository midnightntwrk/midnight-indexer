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

import { env } from 'environment/model';

const DEFAULT_TIMEOUT_MS = 10_000;

interface JsonRpcResponse<T> {
  result?: T;
  error?: { code: number; message: string };
}

interface BlockHeader {
  number: string;
}

/**
 * Minimal Substrate JSON-RPC client over HTTP.
 *
 * Only the handful of chain methods the QA suites need. Deliberately not a
 * polkadot-js instance: these are one-shot calls where a full API bootstrap
 * (metadata download, type registry) costs far more than the call itself.
 */
export class NodeRpcClient {
  private readonly url: string;

  constructor(url: string = env.getNodeHttpBaseURL()) {
    this.url = url;
  }

  /** The height of the current best block. */
  async getChainTip(): Promise<number> {
    const header = await this.call<BlockHeader>('chain_getHeader');
    return Number.parseInt(header.number, 16);
  }

  /** The block hash at the given height, or null if the height is beyond the tip. */
  async getBlockHash(height: number): Promise<string | null> {
    return await this.call<string | null>('chain_getBlockHash', [height]);
  }

  private async call<T>(
    method: string,
    params: unknown[] = [],
    timeoutMs: number = DEFAULT_TIMEOUT_MS,
  ): Promise<T> {
    const response = await fetch(this.url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', id: 1, method, params }),
      signal: AbortSignal.timeout(timeoutMs),
    });

    if (!response.ok) {
      throw new Error(`node RPC ${method} failed: HTTP ${response.status} from ${this.url}`);
    }

    const body = (await response.json()) as JsonRpcResponse<T>;
    if (body.error) {
      throw new Error(`node RPC ${method} failed: ${body.error.message} (${body.error.code})`);
    }
    if (body.result === undefined) {
      throw new Error(`node RPC ${method} returned no result`);
    }

    return body.result;
  }
}
