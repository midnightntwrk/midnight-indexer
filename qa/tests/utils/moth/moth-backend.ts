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

// An alternative transaction backend that builds and submits transactions with
// moth-wallet's sync engine in-process, instead of a `midnight-node-toolkit`
// container.
//
// WHY. The toolkit rebuilds full ledger state from raw blocks on every
// chain-touching call — measured on preprod at ~607 s and ~34.5 GB per call,
// and the cost grows with the chain. moth persists each sub-wallet's state and
// restores it, so a warm wallet catches up in ~1 s. This backend reuses that
// engine for transaction *submission* only.
//
// WHAT IT DELIBERATELY DOES NOT USE. moth's own contract path
// (`contract/deploy.ts`, `call.ts`) hand-rolls signing in a way that surfaces
// as node error 192 (moth issue #119). moth's *transfer* path
// (`sendTokensWithKeys`) does not — it balances, proves and submits through the
// wallet facade, the same path a real wallet uses. This module only wires that
// transfer path, so `generateSingleTx` gets a fast backend without the #119 trap.
//
// SCOPE. Transfers only, for now. Contract deploys and calls, chain forensics
// and chain setup all stay on the toolkit; this backend is selected only for
// single-transfer submission when TX_BACKEND=moth.

import { createHash } from 'crypto';
import {
  deriveWalletKeys,
  NIGHT_TOKEN_ID,
  sendTokensWithKeys,
  startWalletSync,
  type NetworkConfig,
  type SendRequest,
  type SyncedWallet,
  type WalletKeys,
} from '@shieldedtech/moth-wallet';
import * as Rx from 'rxjs';
import log from '@utils/logging/logger';
import { env } from '../../environment/model';
import type { AddressType, ToolkitTransactionResult } from '../toolkit/toolkit-wrapper';

/** Minutes to allow for a cold sync. A warm one takes seconds. */
const SYNC_TIMEOUT_MS = Number(process.env.MN_SYNC_TIMEOUT_MS ?? 6 * 60 * 60 * 1000);

/**
 * A "complete" sync whose applied and target indices are both zero means the
 * subscription delivered nothing — seen on preprod when a websocket is dead
 * (a withdrawn blue/green host 404s) or rate-limited. The SDK reports
 * isSynced=true on that dead stream, so a run would silently proceed against an
 * empty wallet. Reject it if it persists this long.
 */
const ZERO_TOTAL_GRACE_MS = 120_000;

/** A moth-synced wallet, opened once and reused for the life of the process. */
interface MothWallet {
  readonly synced: SyncedWallet;
  readonly keys: WalletKeys;
  readonly networkId: string;
}

// One synced wallet per cache name, keyed so the restore is paid once per
// process rather than once per transaction.
const openWallets = new Map<string, Promise<MothWallet>>();

/**
 * A cache/wallet name derived from the seed by a one-way hash. The seed is
 * secret and must never appear in a path or a log; its SHA-256 prefix is not
 * the seed and is safe to use as a directory name. An explicit
 * `TX_BACKEND_MOTH_WALLET` overrides it (useful to keep a dedicated light
 * wallet warm — the restore cost is per-wallet).
 */
const cacheNameFor = (seed: string): string =>
  process.env.TX_BACKEND_MOTH_WALLET?.trim() ||
  `qa-${createHash('sha256').update(seed).digest('hex').slice(0, 12)}`;

const networkConfig = (): NetworkConfig => ({
  id: env.getNetworkId(),
  nodeUrl: env.getNodeWebsocketBaseURL(),
  indexerUrl: env.getIndexerGraphqlHttpURL(),
  proofServerUrl: env.getProofServerURL(),
});

/**
 * Wait for a fully-synced facade whose DUST balance has stopped moving, and
 * reject a dead-stream fake sync (see ZERO_TOTAL_GRACE_MS). Building against a
 * stale DUST Merkle root is rejected by the node as error 170, so the settle
 * step holds until the dust balance is stable across two emissions.
 */
const awaitSettled = async (facade: SyncedWallet['facade'], timeoutMs: number): Promise<void> => {
  const started = Date.now();
  let lastReport = 0;
  let zeroTotalSince: number | undefined;

  await Rx.firstValueFrom(
    (facade.state() as Rx.Observable<any>).pipe(
      Rx.tap((s: any) => {
        const now = Date.now();
        if (now - lastReport < 15_000) return;
        lastReport = now;
        const one = (name: string, p: any) => {
          if (!p) return `${name}=?`;
          const applied = p.appliedIndex ?? 0n;
          const target = p.highestRelevantWalletIndex ?? p.highestIndex ?? 0n;
          const pct =
            target > 0n ? ` ${((Number(applied) / Number(target)) * 100).toFixed(1)}%` : '';
          return `${name}=${applied}/${target}${pct}`;
        };
        log.info(
          `moth syncing (${((now - started) / 60_000).toFixed(1)}m)  ` +
            `${one('shielded', s.shielded?.state?.progress)}  ` +
            `${one('unshielded', s.unshielded?.progress)}  ` +
            `${one('dust', s.dust?.state?.progress)}`,
        );
      }),
      Rx.filter((s: any) => {
        try {
          const complete =
            s.shielded?.state?.progress?.isStrictlyComplete?.() === true &&
            s.unshielded?.progress?.isStrictlyComplete?.() === true &&
            s.dust?.state?.progress?.isStrictlyComplete?.() === true;
          if (!complete) return false;
          const idle = (p: any) =>
            Number(p?.appliedIndex ?? 0n) === 0 &&
            Number(p?.highestRelevantWalletIndex ?? p?.highestIndex ?? 0n) === 0;
          if (idle(s.shielded?.state?.progress) && idle(s.dust?.state?.progress)) {
            zeroTotalSince ??= Date.now();
            if (Date.now() - zeroTotalSince > ZERO_TOTAL_GRACE_MS) {
              throw new Error(
                'moth reports synced but the indexer delivered no ledger events ' +
                  `(applied and target both 0 on shielded and dust for ${ZERO_TOTAL_GRACE_MS / 1000}s). ` +
                  'The subscription most likely failed to open (a withdrawn or rate-limited ' +
                  'indexer websocket). Check the target indexer is routed and caught up.',
              );
            }
            return false;
          }
          return true;
        } catch (err) {
          if (err instanceof Error && err.message.startsWith('moth reports synced')) throw err;
          return false;
        }
      }),
      Rx.bufferCount(2, 1),
      Rx.filter(([a, b]: any[]) => {
        try {
          return (a.dust?.balance?.(new Date()) ?? 0n) === (b.dust?.balance?.(new Date()) ?? 0n);
        } catch {
          return true;
        }
      }),
      Rx.map(() => undefined),
      Rx.timeout({
        each: timeoutMs,
        with: () =>
          Rx.throwError(
            () =>
              new Error(
                `moth sync produced no progress for ${(timeoutMs / 60_000).toFixed(0)} minutes. ` +
                  'Raise MN_SYNC_TIMEOUT_MS if the progress lines were still advancing.',
              ),
          ),
      }),
    ),
  );
};

/**
 * Open a moth-synced wallet for a seed, or return the one already open for it.
 * The seed is used only to derive keys and a hashed cache name; it is never
 * logged. On a first sync this is minutes; warm, it is seconds.
 */
const openMothWallet = (seed: string): Promise<MothWallet> => {
  const name = cacheNameFor(seed);
  const existing = openWallets.get(name);
  if (existing) return existing;

  const opening = (async (): Promise<MothWallet> => {
    const keys = deriveWalletKeys(seed);
    const network = networkConfig();
    log.info(`moth wallet: ${name} (cache ~/.moth/sync/${network.id}/${name}/)`);
    const synced = await startWalletSync(
      keys,
      network,
      (message) => log.debug(`moth: ${message}`),
      name,
      false,
    );
    await awaitSettled(synced.facade, SYNC_TIMEOUT_MS);
    log.info(`moth wallet ${name} synced`);
    return { synced, keys, networkId: network.id };
  })();

  openWallets.set(name, opening);
  opening.catch(() => openWallets.delete(name));
  return opening;
};

/**
 * Build and submit a single transfer through moth, returning the same shape the
 * toolkit backend returns. Status is `sent`: the tx hash is known at submit,
 * the block hash is not — the caller resolves it from the indexer, exactly as
 * it does for a toolkit `sent` result.
 */
export const generateSingleTxViaMoth = async (
  sourceSeed: string,
  addressType: AddressType,
  destinationAddress: string,
  amount: number,
  tokenType?: string,
): Promise<ToolkitTransactionResult> => {
  const wallet = await openMothWallet(sourceSeed);
  const request: SendRequest = {
    type: addressType,
    tokenId: tokenType ?? NIGHT_TOKEN_ID,
    amount: BigInt(amount),
    to: destinationAddress,
  };

  let txHash: string;
  try {
    txHash = await sendTokensWithKeys(
      wallet.synced.facade,
      wallet.keys,
      wallet.networkId,
      [request],
      (stage) => log.debug(`moth transfer: ${stage}`),
    );
  } catch (err) {
    // Keep this distinct from an indexer assertion failure: this is the
    // transaction generator failing to build or submit, not the indexer
    // reporting the wrong thing. The suite tests the indexer; an ambiguous
    // failure here would point nowhere.
    throw new Error(
      `moth backend failed to build or submit the transaction (not an indexer failure): ${
        (err as Error).message
      }`,
    );
  }

  return {
    txHash,
    blockHash: '',
    status: 'sent',
    rawOutput: `moth backend submitted ${txHash}`,
  };
};

/**
 * Stop every moth wallet opened this process and release its sync subscription.
 * Call from the transaction backend's teardown; an unstopped wallet keeps a
 * websocket open.
 */
export const closeMothWallets = async (): Promise<void> => {
  const opened = [...openWallets.values()];
  openWallets.clear();
  await Promise.all(
    opened.map(async (p) => {
      try {
        const w = await p;
        await w.synced.stop();
      } catch {
        // Already failed to open, or already stopped — nothing to release.
      }
    }),
  );
};
