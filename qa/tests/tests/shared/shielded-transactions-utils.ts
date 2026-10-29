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
import {
  IndexerWsClient,
  type ShieldedTxSubscriptionResponse,
} from '@utils/indexer/websocket-client';
import { SHIELDED_TRANSACTION_SUBSCRIPTION_BY_SESSION_ID_AND_INDEX } from '@utils/indexer/graphql/subscriptions';
import {
  RelevantTransactionWithIndicesSchema,
  ShieldedTransactionsProgressSchema,
} from '@utils/indexer/graphql/schema';
import type { ShieldedTransactionsProgress } from '@utils/indexer/indexer-types';

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/** A RelevantTransaction as streamed, reduced to the keys the server pages and filters by. */
export interface StreamedRelevantTransaction {
  id: number;
  hash: string;
  zswapStartIndex: number;
  zswapEndIndex: number;
  hasCollapsedUpdate: boolean;
}

/**
 * Keeps a `shieldedTransactions` subscription open and records what it streams,
 * so a test can wait for a condition, let the stream settle and inspect the
 * whole sequence afterwards. Progress events are recorded apart from relevant
 * transactions, because the stream never ends: progress keeps arriving while
 * relevant transactions, if the server behaves, stop.
 */
export class RelevantTransactionCollector {
  readonly transactions: StreamedRelevantTransaction[] = [];
  readonly progress: ShieldedTransactionsProgress[] = [];
  readonly errors: string[] = [];

  private unsubscribe: (() => void) | undefined;
  private lastTransactionAtMs = 0;

  /**
   * @param index - the subscription's `index` argument; the server defaults a missing one to 0.
   */
  constructor(
    private readonly ws: IndexerWsClient,
    private readonly sessionId: string,
    private readonly index?: number,
  ) {}

  start(): this {
    if (this.unsubscribe) {
      throw new Error('collector already started');
    }
    this.lastTransactionAtMs = Date.now();
    this.unsubscribe = this.ws.subscribeToShieldedTransactionEvents(
      {
        next: (payload: ShieldedTxSubscriptionResponse) => this.onPayload(payload),
        error: (error) => {
          this.errors.push(error.message);
        },
      },
      this.sessionId,
      SHIELDED_TRANSACTION_SUBSCRIPTION_BY_SESSION_ID_AND_INDEX,
      { INDEX: this.index ?? 0 },
    );
    return this;
  }

  stop(): void {
    this.unsubscribe?.();
    this.unsubscribe = undefined;
  }

  /** Resolve once `predicate` holds, or reject after `timeoutMs` naming `what`. */
  async waitFor(predicate: () => boolean, timeoutMs: number, what: string): Promise<void> {
    const start = Date.now();
    while (!predicate()) {
      if (this.errors.length > 0) {
        throw new Error(`subscription error while waiting for ${what}: ${this.errors.join('; ')}`);
      }
      if (Date.now() - start > timeoutMs) {
        throw new Error(
          `Timed out after ${timeoutMs}ms waiting for ${what}; ` +
            `streamed transaction IDs so far: [${this.transactions.map((t) => t.id).join(', ')}]`,
        );
      }
      await sleep(100);
    }
  }

  /**
   * Resolve `true` once no relevant transaction has arrived for `idleMs`, or
   * `false` when `maxMs` elapses or more than `maxTransactions` were streamed
   * first. A stream that repeats a transaction forever never goes idle, so the
   * caps are what make the wait finite on a server with that defect.
   */
  async waitUntilIdle(options: {
    idleMs: number;
    maxMs: number;
    maxTransactions?: number;
  }): Promise<boolean> {
    const { idleMs, maxMs, maxTransactions = Number.POSITIVE_INFINITY } = options;
    const start = Date.now();
    for (;;) {
      if (this.transactions.length > maxTransactions) {
        return false;
      }
      if (Date.now() - this.lastTransactionAtMs >= idleMs) {
        return true;
      }
      if (Date.now() - start >= maxMs) {
        return false;
      }
      await sleep(100);
    }
  }

  private onPayload(payload: ShieldedTxSubscriptionResponse): void {
    if (payload.errors?.length) {
      this.errors.push(...payload.errors.map((error) => error.message));
      return;
    }
    const event = payload.data?.shieldedTransactions as { __typename?: string } | undefined;
    switch (event?.__typename) {
      case 'RelevantTransaction': {
        const parsed = RelevantTransactionWithIndicesSchema.safeParse(event);
        if (!parsed.success) {
          this.errors.push(
            `RelevantTransaction schema: ${JSON.stringify(parsed.error.format(), null, 2)}`,
          );
          return;
        }
        const { transaction, zswapCollapsedUpdate } = parsed.data;
        this.transactions.push({
          id: transaction.id,
          hash: transaction.hash,
          zswapStartIndex: transaction.zswapStartIndex,
          zswapEndIndex: transaction.zswapEndIndex,
          hasCollapsedUpdate: zswapCollapsedUpdate !== null,
        });
        this.lastTransactionAtMs = Date.now();
        return;
      }
      case 'ShieldedTransactionsProgress': {
        const parsed = ShieldedTransactionsProgressSchema.safeParse(event);
        if (parsed.success) {
          this.progress.push(parsed.data);
        } else {
          this.errors.push(
            `ShieldedTransactionsProgress schema: ${JSON.stringify(parsed.error.format(), null, 2)}`,
          );
        }
        return;
      }
      default:
        log.debug(`Ignoring shielded transactions event: ${JSON.stringify(payload)}`);
    }
  }
}

/**
 * Subscribe from `index`, let the stream settle and return what it streamed.
 * `settled` is false when the stream never went idle within `maxMs` or exceeded
 * `maxTransactions`, i.e. when the server kept streaming.
 */
export async function collectRelevantTransactions(
  ws: IndexerWsClient,
  sessionId: string,
  options: { index?: number; idleMs?: number; maxMs?: number; maxTransactions?: number } = {},
): Promise<{
  transactions: StreamedRelevantTransaction[];
  progress: ShieldedTransactionsProgress[];
  settled: boolean;
}> {
  const { index, idleMs = 3_000, maxMs = 40_000, maxTransactions } = options;
  const collector = new RelevantTransactionCollector(ws, sessionId, index).start();
  try {
    const settled = await collector.waitUntilIdle({ idleMs, maxMs, maxTransactions });
    if (collector.errors.length > 0) {
      throw new Error(`shielded transactions subscription failed: ${collector.errors.join('; ')}`);
    }
    return { transactions: collector.transactions, progress: collector.progress, settled };
  } finally {
    collector.stop();
  }
}

/**
 * The first ShieldedTransactionsProgress of a fresh subscription, which the
 * server emits immediately on subscribe. Its `highestZswapEndIndex` is the
 * chain's next free zswap index.
 */
export async function awaitFirstProgress(
  ws: IndexerWsClient,
  sessionId: string,
  timeoutMs = 15_000,
): Promise<ShieldedTransactionsProgress> {
  const collector = new RelevantTransactionCollector(ws, sessionId).start();
  try {
    await collector.waitFor(
      () => collector.progress.length > 0,
      timeoutMs,
      'the first ShieldedTransactionsProgress event',
    );
    return collector.progress[0];
  } finally {
    collector.stop();
  }
}
