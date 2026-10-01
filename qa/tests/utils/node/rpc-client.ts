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
 * What the node says about a contract address: either its hex-encoded state, or the
 * RPC error it answered with when it holds no such contract.
 */
export interface ContractStateResult {
  present: boolean;
  state?: string;
  /** What the node actually said, for failure messages. */
  nodeAnswer: string;
}

// How the node says "I hold no such contract". Node 2.x raises an RPC error carrying
// StateRpcError::ContractNotPresent's text; node 1.0.x answers successfully with an empty
// state. Both measured on 2026-10-01 against a real deploy and a rolled-back one. Any other
// error is a different failure and must not be read as absence — see getContractState.
const CONTRACT_ABSENT_MESSAGE = /contract not present/i;

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

  /** The height of the highest finalized block. */
  async getFinalizedHeight(): Promise<number> {
    const hash = await this.call<string>('chain_getFinalizedHead');
    return await this.getBlockHeight(hash);
  }

  /** The version string the node reports for itself. */
  async getSystemVersion(): Promise<string> {
    return await this.call<string>('system_version');
  }

  /** The height of the block with the given hash. */
  async getBlockHeight(hash: string): Promise<number> {
    const header = await this.call<BlockHeader>('chain_getHeader', [hash]);
    return Number.parseInt(header.number, 16);
  }

  /**
   * The state the node holds for a contract address.
   *
   * Only the two known absence answers are reported as absent. An unrecognised RPC error — a
   * renamed method, a rejected parameter, a node that cannot read its own state — is thrown,
   * so a broken oracle fails the run instead of quietly agreeing that nothing is there. A
   * transport failure is thrown for the same reason.
   */
  async getContractState(contractAddress: string): Promise<ContractStateResult> {
    const body = await this.request<string>('midnight_contractState', [contractAddress]);
    if (body.error) {
      if (!CONTRACT_ABSENT_MESSAGE.test(body.error.message)) {
        throw new Error(
          `node RPC midnight_contractState failed: ${body.error.message} (${body.error.code})`,
        );
      }
      return { present: false, nodeAnswer: body.error.message };
    }
    if (body.result === undefined) {
      throw new Error('node RPC midnight_contractState returned no result');
    }
    const state = body.result.replace(/^0x/, '');
    if (state === '') {
      return { present: false, nodeAnswer: 'an empty contract state' };
    }
    return { present: true, state, nodeAnswer: `${state.length} characters of state` };
  }

  private async call<T>(
    method: string,
    params: unknown[] = [],
    timeoutMs: number = DEFAULT_TIMEOUT_MS,
  ): Promise<T> {
    const body = await this.request<T>(method, params, timeoutMs);
    if (body.error) {
      throw new Error(`node RPC ${method} failed: ${body.error.message} (${body.error.code})`);
    }
    if (body.result === undefined) {
      throw new Error(`node RPC ${method} returned no result`);
    }
    return body.result;
  }

  private async request<T>(
    method: string,
    params: unknown[] = [],
    timeoutMs: number = DEFAULT_TIMEOUT_MS,
  ): Promise<JsonRpcResponse<T>> {
    const response = await fetch(this.url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', id: 1, method, params }),
      signal: AbortSignal.timeout(timeoutMs),
    });

    if (!response.ok) {
      throw new Error(`node RPC ${method} failed: HTTP ${response.status} from ${this.url}`);
    }

    return (await response.json()) as JsonRpcResponse<T>;
  }
}
