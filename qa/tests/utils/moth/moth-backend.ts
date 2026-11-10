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
  decodeBech32mAddress,
  deriveWalletKeys,
  designateForDustWithKeys,
  NIGHT_TOKEN_ID,
  sendTokensWithKeys,
  startWalletSync,
  type NetworkConfig,
  type SendRequest,
  type SyncedWallet,
  type WalletKeys,
} from '@shieldedtech/moth-wallet';
import type { FacadeState, UtxoWithMeta } from '@midnightntwrk/wallet-sdk/facade';
import type { DustWalletState } from '@midnightntwrk/wallet-sdk/dust';
import {
  coreStateOf as coreStateOfV1,
  type Dust,
  type DustFullInfo,
} from '@midnightntwrk/wallet-sdk/dust/v1';
import { coreStateOf as coreStateOfV2 } from '@midnightntwrk/wallet-sdk/dust/v2';
import { Option } from 'effect';
import * as Rx from 'rxjs';
import log from '@utils/logging/logger';
import { env } from '../../environment/model';
import { IndexerHttpClient } from '../indexer/http-client';
import { rootHexLe, sameRoot } from './dust-root';
import { ensureProofServer, proofServerMismatchHint } from './proof-server';
import type { AddressType, ToolkitTransactionResult } from '../toolkit/toolkit-wrapper';
import type {
  DustBalance,
  DustOutput,
  PrivateWalletState,
  PublicWalletState,
  Utxo,
} from '../toolkit/schemas';

/** Minutes to allow for a cold sync. A warm one takes seconds. */
const SYNC_TIMEOUT_MS = Number(process.env.MN_SYNC_TIMEOUT_MS ?? 6 * 60 * 60 * 1000);

/**
 * How long to wait for a dust registration submitted by {@link assertSpendableDust}
 * to show up as DUST the wallet can spend. Registration is confirmed within a
 * block or two; the wait covers the indexer and the wallet's sync behind it.
 */
const REGISTRATION_SETTLE_MS = 5 * 60 * 1000;

/**
 * A "complete" sync whose applied and target indices are both zero means the
 * subscription delivered nothing — seen on preprod when a websocket is dead
 * (a withdrawn blue/green host 404s) or rate-limited. The SDK reports
 * isSynced=true on that dead stream, so a run would silently proceed against an
 * empty wallet. Reject it if it persists this long.
 */
const ZERO_TOTAL_GRACE_MS = 120_000;

/** A moth-synced wallet, opened once and reused for the life of the process. */
export interface MothWallet {
  readonly synced: SyncedWallet;
  readonly keys: WalletKeys;
  readonly networkId: string;
  /** moth's cache/wallet name for the seed, for messages; never the seed itself. */
  readonly name: string;
}

// One synced wallet per cache name, keyed so the restore is paid once per
// process rather than once per transaction.
const openWallets = new Map<string, Promise<MothWallet>>();

/**
 * A one-way fingerprint of a seed, for use wherever a seed must identify a
 * wallet without being stored or logged. The seed is secret; its SHA-256
 * prefix is not the seed, and unlike a prefix of the seed itself it is
 * distinct for seeds that share leading bytes (zero-padded seeds do).
 */
export const seedFingerprint = (seed: string): string =>
  createHash('sha256').update(seed).digest('hex').slice(0, 12);

/**
 * A cache/wallet name derived from the seed fingerprint, safe to use as a
 * directory name. An explicit `TX_BACKEND_MOTH_WALLET` overrides it (useful
 * to keep a dedicated light wallet warm — the restore cost is per-wallet).
 */
const cacheNameFor = (seed: string): string =>
  process.env.TX_BACKEND_MOTH_WALLET?.trim() || `qa-${seedFingerprint(seed)}`;

/**
 * Build moth's network config.
 *
 * The proof server is started on demand, or taken from PROOF_SERVER_URL.
 * Proving happens in this process, so the server is a dependency of the wallet,
 * not of the chain stack — deployed runs need it just as much as undeployed
 * ones do, which is why it is not in `docker-compose.yaml`.
 *
 * It is required even to *sync*, not only to submit: moth builds its proving
 * service during `startWalletSync`, so an empty URL fails there with
 * `TypeError: Invalid URL` (observed on preview, 2026-09-17). Global-setup
 * warm-up therefore needs one too.
 */
const networkConfig = async (): Promise<NetworkConfig> => ({
  id: env.getNetworkId(),
  nodeUrl: env.getNodeWebsocketBaseURL(),
  indexerUrl: env.getIndexerGraphqlHttpURL(),
  proofServerUrl: await ensureProofServer(),
});

/**
 * Wait for a fully-synced facade whose DUST balance has stopped moving, and
 * reject a dead-stream fake sync (see ZERO_TOTAL_GRACE_MS). Building against a
 * stale DUST Merkle root is rejected by the node as error 170, so the settle
 * step holds until the dust balance is stable across two emissions.
 */
/**
 * Elapsed time as `12m 04s`. Decimal minutes read as a clock time to anyone
 * skimming — `12.4m` looks like 12:04 but means 12m 24s — and these lines are
 * read while waiting on a sync that can run for hours.
 */
const elapsed = (ms: number): string => {
  const total = Math.max(0, Math.round(ms / 1000));
  const minutes = Math.floor(total / 60);
  const seconds = total % 60;
  return `${minutes}m ${String(seconds).padStart(2, '0')}s`;
};

/**
 * The sync progress of the shielded and dust sub-wallets, counted in ledger
 * events, with the wallet's own share of them known.
 */
type EventProgress = FacadeState['shielded']['progress'];

/**
 * The sync progress of the unshielded sub-wallet, counted in transactions.
 * It does not know which transactions concern this wallet, so it has no
 * per-wallet target.
 */
type TransactionProgress = FacadeState['unshielded']['progress'];

type SubWallet = 'shielded' | 'unshielded' | 'dust';

const ALL_SUB_WALLETS: readonly SubWallet[] = ['shielded', 'unshielded', 'dust'];

/**
 * Whether every named sub-wallet of a facade state reports a strictly complete
 * sync: the one bar every read of the wallet's state waits behind, so a
 * partially synced wallet cannot report a short balance.
 */
const isStrictlyComplete = (s: FacadeState, subWallets: readonly SubWallet[]): boolean =>
  subWallets.every((name) => s[name].progress.isStrictlyComplete());

/**
 * Wait until every named sub-wallet of the moth facade reports a strictly
 * complete sync, and return that facade state.
 *
 * @param purpose - What cannot happen without it, for the timeout message.
 */
const awaitStrictlyComplete = (
  wallet: MothWallet,
  subWallets: readonly SubWallet[],
  purpose: string,
): Promise<FacadeState> =>
  Rx.firstValueFrom(
    wallet.synced.facade.state().pipe(
      Rx.filter((s) => isStrictlyComplete(s, subWallets)),
      Rx.timeout({
        each: SYNC_TIMEOUT_MS,
        with: () =>
          Rx.throwError(
            () =>
              new Error(
                `moth did not reach a strictly-complete ${subWallets.join('/')} state in ` +
                  `${(SYNC_TIMEOUT_MS / 60_000).toFixed(0)} minutes, so ${purpose}.`,
              ),
          ),
      }),
    ),
  );

const awaitSettled = async (
  facade: SyncedWallet['facade'],
  timeoutMs: number,
  echoToConsole = false,
): Promise<void> => {
  const started = Date.now();
  let lastReport = 0;
  let lastEmission = Date.now();
  let zeroTotalSince: number | undefined;

  // A heartbeat, not a progress line. The progress lines below only print when
  // the facade emits, so a wallet that emits nothing at all — the shape a
  // stale cache restored against a re-genesised chain produces — stays
  // completely silent and looks identical to one that is simply slow. This
  // says out loud how long it has been quiet.
  const heartbeat = echoToConsole
    ? setInterval(() => {
        const quietMs = Date.now() - lastEmission;
        if (quietMs < 60_000) return;
        console.log(
          `[SETUP] moth wallet has emitted no state for ${elapsed(quietMs)} ` +
            `(${elapsed(Date.now() - started)} elapsed). A first sync is slow but ` +
            'should still emit; a permanently quiet wallet usually means a cache restored ' +
            'against a different chain — purge ~/.moth/sync/<network>/<wallet>/ and retry.',
        );
      }, 60_000)
    : undefined;
  heartbeat?.unref?.();

  try {
    await Rx.firstValueFrom(
      facade.state().pipe(
        Rx.tap((s) => {
          const now = Date.now();
          lastEmission = now;
          if (now - lastReport < 15_000) return;
          lastReport = now;
          // Grouped with thousands separators: these run to seven figures on a
          // deployed chain, and an ungrouped 1534959 is not readable at a glance.
          const n = (v: bigint | number) => Number(v).toLocaleString('en-US');
          const one = (name: string, p: EventProgress | TransactionProgress) => {
            // Once a sub-wallet is done, say so instead of printing indices that
            // no longer change. It also gives the only sensible reading for a
            // wallet whose counters are 0/0 — complete with nothing to apply,
            // which "0/0" alone makes look stuck.
            if (p.isStrictlyComplete()) return `${name}=synced 100.0%`;
            // `highestRelevantWalletIndex` counts only the events that concern
            // THIS wallet, which is what "synced" is judged against. The
            // unshielded sub-wallet counts transactions and has no such
            // per-wallet target, so its chain position is the target.
            const [applied, target, chain] =
              'appliedIndex' in p
                ? [p.appliedIndex, p.highestRelevantWalletIndex, p.highestIndex]
                : [p.appliedId, p.highestTransactionId, p.highestTransactionId];
            const pct =
              target > 0n ? ` ${((Number(applied) / Number(target)) * 100).toFixed(1)}%` : '';
            // The chain-wide stream position, shown only when it differs from
            // the per-wallet target. moth's TUI reports this larger number, so
            // without it the two disagree and look like a bug.
            const ofChain = chain > target ? ` of ${n(chain)}` : '';
            return `${name}=${n(applied)}/${n(target)}${pct}${ofChain}`;
          };
          // Each sub-wallet state exposes its `progress` directly; what sits
          // under `.state` is the producing variant's sealed ledger state.
          const line =
            `moth syncing (${elapsed(now - started)})  ` +
            `${one('shielded', s.shielded.progress)}  ` +
            `${one('unshielded', s.unshielded.progress)}  ` +
            `${one('dust', s.dust.progress)}`;
          log.info(line);
          // Global setup has no test log of its own and can sit here for well
          // over an hour on a first sync. Without a line on stdout a healthy slow
          // sync and a hung one look identical — which has already cost a run.
          if (echoToConsole) console.log(`[SETUP] ${line}`);
        }),
        Rx.filter((s) => {
          try {
            if (!isStrictlyComplete(s, ALL_SUB_WALLETS)) return false;
            // From the fork the dust wallet syncs from the indexer's projections
            // and its indices are block heights rather than event ids; they are
            // still non-zero on any chain that has a block, so the dead-stream
            // reading holds for it too.
            const idle = (p: EventProgress) =>
              p.appliedIndex === 0n && p.highestRelevantWalletIndex === 0n;
            if (idle(s.shielded.progress) && idle(s.dust.progress)) {
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
        Rx.filter(([a, b]) => {
          try {
            return a.dust.balance(new Date()) === b.dust.balance(new Date());
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
  } finally {
    if (heartbeat) clearInterval(heartbeat);
  }
};

/**
 * Open a moth-synced wallet for a seed, or return the one already open for it.
 * The seed is used only to derive keys and a hashed cache name; it is never
 * logged. On a first sync this is minutes; warm, it is seconds.
 */
export const openMothWallet = (seed: string, echoProgress = false): Promise<MothWallet> => {
  const name = cacheNameFor(seed);
  const existing = openWallets.get(name);
  if (existing) return existing;

  const opening = (async (): Promise<MothWallet> => {
    const network = await networkConfig();
    // Per-role seeds, not ledger key objects: the SDK's forking wallets derive
    // each ledger version's keys from them as the chain crosses the fork, and
    // the caches they write carry which sync produced them, so nothing has to
    // be loaded or set aside per ledger train here.
    const keys = deriveWalletKeys(seed);
    log.info(`moth wallet: ${name} (cache ~/.moth/sync/${network.id}/${name}/)`);
    const synced = await startWalletSync(
      keys,
      network,
      (message) => log.debug(`moth: ${message}`),
      name,
      false,
    );
    // A failed settle (timeout, dead stream, zero-total) never reaches
    // openWallets as a usable wallet, so closeMothWallets() cannot stop it:
    // stop the sync here or its websocket and sync engine stay alive.
    try {
      await awaitSettled(synced.facade, SYNC_TIMEOUT_MS, echoProgress);
    } catch (error) {
      await synced
        .stop()
        .catch((stopError: unknown) =>
          log.warn(`moth wallet ${name}: stop after failed sync also failed: ${stopError}`),
        );
      throw error;
    }
    log.info(`moth wallet ${name} synced`);
    return { synced, keys, networkId: network.id, name };
  })();

  openWallets.set(name, opening);
  opening.catch(() => openWallets.delete(name));
  return opening;
};

/**
 * Refuse to spend from a wallet that holds no DUST, saying why, before the SDK
 * fails the transaction with `Insufficient Funds: could not balance dust`.
 *
 * WHY. That SDK message reads as "fund the wallet", and on a chain that has
 * just hard-forked the wallet is funded: a ledger hard fork can rebuild the
 * dust generation tree from the Cardano-side registrations only, so a wallet
 * whose NIGHT was registered before the fork keeps its NIGHT but has no DUST
 * and no registration. The fix is to register the NIGHT again. Registration is
 * self-funding (the fee is paid from the DUST the NIGHT would have generated
 * had it been registered all along), so it is done here when
 * `MOTH_REGISTER_DUST=1`, and named in the error otherwise.
 *
 * @param wallet - An open, settled moth wallet.
 * @param purpose - What the caller is about to build, for the message.
 */
export const assertSpendableDust = async (wallet: MothWallet, purpose: string): Promise<void> => {
  const read = async () => {
    const s = await Rx.firstValueFrom(wallet.synced.facade.state());
    const night = s.unshielded.availableCoins.filter((c) => c.utxo.type === NIGHT_TOKEN_ID);
    return {
      dustCoins: s.dust.availableCoins.length,
      balance: s.dust.balance(new Date()),
      night: night.length,
      registered: night.filter((c) => c.meta.registeredForDustGeneration).length,
    };
  };

  let state = await read();
  if (state.balance > 0n) return;

  if (state.dustCoins === 0 && state.night > 0 && process.env.MOTH_REGISTER_DUST === '1') {
    log.warn(
      `moth wallet ${wallet.name}: no DUST behind ${state.night} NIGHT UTXO(s); ` +
        'registering them for DUST generation (MOTH_REGISTER_DUST=1)',
    );
    const txHash = await designateForDustWithKeys(
      wallet.synced.facade,
      wallet.keys,
      wallet.networkId,
      undefined,
      (stage) => log.debug(`moth dust registration: ${stage}`),
    );
    if (txHash) {
      log.info(`moth wallet ${wallet.name}: dust registration submitted (${txHash})`);
      const deadline = Date.now() + REGISTRATION_SETTLE_MS;
      while (state.balance === 0n && Date.now() < deadline) {
        await new Promise((resolve) => setTimeout(resolve, 5_000));
        state = await read();
      }
      if (state.balance > 0n) return;
    } else {
      log.warn(`moth wallet ${wallet.name}: moth found no unregistered NIGHT to register`);
    }
  }

  const detail =
    `${state.dustCoins} DUST UTXO(s), balance ${state.balance}, ${state.night} NIGHT UTXO(s) ` +
    `of which ${state.registered} flagged registered for DUST generation`;
  if (state.night === 0) {
    throw new Error(
      `moth wallet ${wallet.name} holds no DUST and no NIGHT (${detail}), so it cannot ${purpose}. ` +
        `Fund it on ${wallet.networkId}.`,
    );
  }
  if (state.dustCoins === 0) {
    throw new Error(
      `moth wallet ${wallet.name} holds NIGHT but no DUST (${detail}), so it cannot ${purpose}. ` +
        'Its NIGHT is not registered for DUST generation on this chain: after a ledger hard fork ' +
        'pre-fork registrations are not carried over. Register it again by ' +
        "rerunning with MOTH_REGISTER_DUST=1, or with moth's designateForDust for the seed. If the " +
        'NIGHT is flagged registered but the chain shows no DUST for it, compare the cached dust ' +
        `state with a fresh replay before trusting either; a cache under ~/.moth/sync/${wallet.networkId}/` +
        `${wallet.name}/ written by an older moth build can be set aside to force a rebuild.`,
    );
  }
  throw new Error(
    `moth wallet ${wallet.name} holds DUST UTXOs that have generated nothing yet (${detail}), ` +
      `so it cannot ${purpose}. Retry in a few minutes.`,
  );
};

/**
 * How long to wait for the dust wallet's Merkle trees to catch up with the
 * chain tip. A projections pass runs every few seconds; this covers a slow
 * indexer and a few other dust transactions landing meanwhile.
 */
const DUST_ROOT_CATCH_UP_MS = 3 * 60 * 1000;

/**
 * Hold until the dust wallet's commitment and generation trees carry the same
 * roots as the chain tip, so the next DUST spend is proven against state the
 * node agrees with.
 *
 * WHY. From the ledger-9 fork the dust wallet syncs from the indexer's
 * projections in passes rather than from a live event stream, so right after
 * a spend of its own it still holds the tree it spent against: the spent
 * output is gone, the change output and the chain's new root are not there
 * until the next pass. A second spend built in that window proves membership
 * against a root the node no longer has, and the node refuses it as
 * `InvalidDustSpendProof` (`1010: Invalid Transaction: Custom error: 170`),
 * which it also uses for genuinely bad proofs. The generation tree is held to
 * the same bar: a NIGHT spend of this wallet's changes it, and the dust proof
 * reads it too.
 *
 * The chain root moves whenever anyone's dust transaction lands, so both
 * sides are re-read on every check rather than compared against a snapshot.
 *
 * @param wallet - An open moth wallet.
 * @param purpose - What the caller is about to build, for the message.
 */
export const awaitDustRootsCurrent = async (wallet: MothWallet, purpose: string): Promise<void> => {
  const indexer = new IndexerHttpClient();
  const started = Date.now();
  let lastReport = 0;
  for (;;) {
    const state = await Rx.firstValueFrom(wallet.synced.facade.state());
    const local = dustLocalState(state.dust);
    const tip = (await indexer.getLatestBlock()).data?.block;
    const commitment = local.commitmentTreeRoot();
    const generation = local.generatingTreeRoot();
    if (
      sameRoot(commitment, tip?.dustCommitmentMerkleTreeRoot) &&
      sameRoot(generation, tip?.dustGenerationMerkleTreeRoot)
    ) {
      return;
    }
    const detail =
      `commitment root wallet ${rootHexLe(commitment) || 'none'} vs chain ` +
      `${tip?.dustCommitmentMerkleTreeRoot ?? 'unknown'}, generation root wallet ` +
      `${rootHexLe(generation) || 'none'} vs chain ${tip?.dustGenerationMerkleTreeRoot ?? 'unknown'} ` +
      `at block ${tip?.height ?? '?'}`;
    if (Date.now() - started > DUST_ROOT_CATCH_UP_MS) {
      throw new Error(
        `moth wallet ${wallet.name}: its DUST Merkle trees did not catch up with the chain in ` +
          `${DUST_ROOT_CATCH_UP_MS / 60_000} minutes, so it cannot ${purpose} (${detail}). A spend ` +
          'proven against a stale root is refused by the node as InvalidDustSpendProof ' +
          '(1010: Custom error: 170).',
      );
    }
    if (Date.now() - lastReport > 10_000) {
      lastReport = Date.now();
      log.debug(
        `moth wallet ${wallet.name}: waiting for its DUST trees to reach the tip (${detail})`,
      );
    }
    await new Promise((resolve) => setTimeout(resolve, 2_000));
  }
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
  await assertSpendableDust(wallet, 'build the transfer');
  await awaitDustRootsCurrent(wallet, 'build the transfer');
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
    const message = (err as Error).message;
    // Proving failures are almost always a ledger-train mismatch, and their own
    // message never says so. Add the sentence that explains it.
    const hint = /prov(e|ing)|zk|circuit/i.test(message) ? ` ${proofServerMismatchHint()}` : '';
    throw new Error(
      'moth backend failed to build or submit the transaction ' +
        `(not an indexer failure): ${message}${hint}`,
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
 * Sync a wallet and flush its state to moth's on-disk cache, then release it.
 *
 * Meant for global setup, which has no test timeout to burn: the first sync on a
 * fresh environment walks the chain and can take a long while. Test workers are
 * separate processes and cannot inherit this facade — what they inherit is the
 * cache on disk, which turns their cold sync into a much shorter restore. Keep
 * the wallet light (a low-traffic seed): restore cost tracks the wallet's state
 * size, not the chain length.
 */
export const warmMothWallet = async (seed: string): Promise<void> => {
  // Echo progress: this is the one call that can run for hours, and it runs in
  // global setup where nothing else prints.
  const wallet = await openMothWallet(seed, true);
  // moth writes the cache on stop(), so release it rather than leaving the
  // subscription open for the life of the setup process.
  await wallet.synced.stop();
  openWallets.delete(cacheNameFor(seed));
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
  // The proof server is NOT stopped here. It is a service owned by global
  // setup, which starts it once and stops it in teardown; a worker stopping it
  // would pull it out from under its siblings.
};

/**
 * List every unshielded token type the seed's wallet can currently spend, with
 * the total value held of each.
 *
 * This reads the wallet's own live balance, which is the only thing that tells
 * us what the wallet can actually *spend*. A chain-wide scan (the
 * `unshielded-token-types.jsonc` fixture) records what exists on the chain and
 * who held it at scan time — useful for query and streaming tests, but it
 * cannot establish spendable ownership for the wallet under test.
 *
 * Waits for the unshielded sub-wallet to be strictly complete, the same bar
 * moth's own `listNightUtxos` uses, so a partially synced wallet cannot report
 * a short balance.
 *
 * @param seed - The seed whose wallet to read. Never logged.
 * @returns Total spendable value per token type, keyed by the hex token type
 *          the indexer also reports as `tokenType`.
 */
export const listUnshieldedHoldingsViaMoth = async (seed: string): Promise<Map<string, bigint>> => {
  const wallet = await openMothWallet(seed);
  const state = await awaitStrictlyComplete(
    wallet,
    ['unshielded'],
    "the wallet's spendable token types cannot be listed",
  );

  const totals = new Map<string, bigint>();
  for (const coin of state.unshielded.availableCoins) {
    const tokenType = coin.utxo.type;
    totals.set(tokenType, (totals.get(tokenType) ?? 0n) + coin.utxo.value);
  }
  return totals;
};

/**
 * The ledger's "never" timestamp, `Timestamp::MAX`, in seconds. The toolkit
 * prints it as the `dtime` of a NIGHT UTXO that is still backing generation;
 * the SDK maps the same sentinel to `undefined`, so map it back.
 */
const LEDGER_TIMESTAMP_MAX_SECS = Number(2n ** 64n - 1n);

/** A field element or nonce as even-length lowercase hex, the shape the toolkit's keys take. */
const toHex = (value: bigint | string): string => {
  if (typeof value === 'string') return value.toLowerCase();
  const hex = value.toString(16);
  return hex.length % 2 === 0 ? hex : `0${hex}`;
};

const toSecs = (date: Date): number => Math.floor(date.getTime() / 1000);

/** A ledger `QualifiedDustOutput` in the toolkit's snake_case `DustOutput` shape. */
const toDustOutput = (token: Dust): DustOutput => ({
  initial_value: Number(token.initialValue),
  dust_public: toHex(token.owner),
  nonce: toHex(token.nonce),
  seq: token.seq,
  ctime: toSecs(token.ctime),
  backing_night: toHex(token.backingNight),
  mt_index: Number(token.mtIndex),
});

/**
 * An unshielded `UtxoWithMeta` in the toolkit's `Utxo` shape.
 *
 * `id` is `<intent hash>#<output number>`, which identifies the output on the
 * ledger. The SDK does not expose the UTXO's `initial_nonce` (the toolkit
 * derives it from ledger state), so it is left empty; nothing in the suite
 * reads it beyond the schema's string check.
 */
const toUtxo = (coin: UtxoWithMeta): Utxo => ({
  id: `${coin.utxo.intentHash}#${coin.utxo.outputNo}`,
  initial_nonce: '',
  value: Number(coin.utxo.value),
  user_address: coin.utxo.owner,
  token_type: coin.utxo.type,
  intent_hash: coin.utxo.intentHash,
  output_number: coin.utxo.outputNo,
});

/**
 * Read a seed's private wallet state from the moth-synced wallet, in the shape
 * the toolkit's `show-wallet --seed` command prints.
 *
 * WHY. Same as {@link getDustBalanceViaMoth}: `show-wallet` replays the whole
 * chain into the toolkit's fetch cache before printing anything, which never
 * finishes inside a test timeout on a deployed chain (qanet, 2026-10-09).
 *
 * MAPPING. `coins` holds the shielded sub-wallet's available coins keyed by
 * nullifier (the ledger's own key for a wallet's coins), `utxos` the unshielded
 * sub-wallet's available UTXOs, and `dust_utxos` the dust sub-wallet's
 * available outputs. Hex encodings follow {@link toHex}, so values are
 * schema-valid but not byte-identical to the toolkit's serialization.
 *
 * @param seed - The seed whose wallet to read. Never logged.
 */
export const showPrivateWalletStateViaMoth = async (seed: string): Promise<PrivateWalletState> => {
  const wallet = await openMothWallet(seed);
  const state = await awaitStrictlyComplete(
    wallet,
    ALL_SUB_WALLETS,
    'the private wallet state cannot be read',
  );

  const coins = Object.fromEntries(
    state.shielded.availableCoins.map(({ coin, nullifier }) => [
      toHex(nullifier),
      {
        nonce: toHex(coin.nonce),
        token_type: toHex(coin.type),
        value: Number(coin.value),
        mt_index: Number(coin.mt_index),
      },
    ]),
  );

  return {
    coins,
    utxos: state.unshielded.availableCoins.map(toUtxo),
    dust_utxos: state.dust.availableCoins.map((coin) => toDustOutput(coin.token)),
  };
};

/**
 * Read an unshielded address's public wallet state from the moth-synced wallet,
 * in the shape the toolkit's `show-wallet --address` command prints.
 *
 * moth opens wallets by seed, not address, so the caller passes the seed that
 * owns `address`; the address is checked against that wallet so a mismatched
 * pair fails loudly instead of returning another wallet's UTXOs. An address
 * alone reveals no shielded coins or dust outputs, so those fields are empty,
 * as in the toolkit's output.
 *
 * @param seed - The seed that owns `address`. Never logged.
 * @param address - The bech32m unshielded address to report.
 */
export const showPublicWalletStateViaMoth = async (
  seed: string,
  address: string,
): Promise<PublicWalletState> => {
  const wallet = await openMothWallet(seed);
  const state = await awaitStrictlyComplete(
    wallet,
    ['unshielded'],
    'the public wallet state cannot be read',
  );

  const requested = Buffer.from(decodeBech32mAddress(address).data).toString('hex');
  if (requested !== state.unshielded.address.hexString) {
    throw new Error(
      `Unshielded address ${address} does not belong to the supplied seed's wallet; ` +
        'moth can only report the public state of a wallet it holds the seed for.',
    );
  }

  return {
    coins: {},
    utxos: state.unshielded.availableCoins.map(toUtxo),
    dust_utxos: [],
  };
};

/**
 * The ledger's `DustLocalState` behind a dust sub-wallet state, which is where
 * the generation lookup lives.
 *
 * The facade seals each variant's ledger state, because it is made of one
 * ledger version's objects: the ledger-v8 variant produces it below the fork,
 * the ledger-v9 one from it, and only the producing variant can read it back.
 * Both ledgers expose `generationInfo` with the same shape, so whichever one
 * answers serves the mapping below.
 */
const dustLocalState = (dust: DustWalletState) => {
  const v2 = coreStateOfV2(dust);
  if (Option.isSome(v2)) return v2.value.state;
  const v1 = coreStateOfV1(dust);
  if (Option.isSome(v1)) return v1.value.state;
  throw new Error(
    `moth emitted a dust state at protocol version ${dust.protocolVersion} that neither ` +
      'the ledger-v8 nor the ledger-v9 dust variant can read, so its generation info cannot be read.',
  );
};

/**
 * Read a seed's DUST balance from the moth-synced wallet, in the same shape the
 * toolkit's `dust-balance` command prints.
 *
 * WHY. The toolkit answers this by replaying the whole chain from genesis on
 * every call. On a deployed chain that is millions of blocks and never finishes
 * inside a test timeout (qanet, 2026-10-09). moth restores the wallet from its
 * on-disk cache, which global setup has already warmed for the transfer
 * funding seed.
 *
 * MAPPING. Built from the same ledger state the toolkit reads: one
 * `generation_infos` entry per available DUST UTXO, `source` keyed by that
 * UTXO's nonce with its value at `now` (only UTXOs with generation info, as the
 * toolkit does), and `total` the wallet balance at `now`. Field elements are
 * hex-encoded big-endian, so keys are valid hex but not byte-identical to the
 * toolkit's ledger serialization; nothing in the schema or the suite compares
 * them across backends.
 *
 * @param seed - The seed whose wallet to read. Never logged.
 */
export const getDustBalanceViaMoth = async (seed: string): Promise<DustBalance> => {
  const wallet = await openMothWallet(seed);
  const { dust } = await awaitStrictlyComplete(wallet, ['dust'], 'the DUST balance cannot be read');

  // One clock for every figure below: `generatedNow` per UTXO and `total` must
  // agree, and the wallet's own `availableCoins` getter would value the coins
  // at its sync time instead.
  const now = new Date();

  // The available coins already exclude UTXOs without generation info (which
  // have no value the toolkit can report, and the schema's `generation_info` is
  // not nullable) and carry their `dtime`. The generation info is read again only
  // for the fields the full info lacks (value, owner, nonce); it exists for
  // every coin returned here.
  const coins: readonly DustFullInfo[] = dust.availableCoins;
  const local = dustLocalState(dust);
  const generationOf = (token: Dust) => local.generationInfo(token as never);

  const generation_infos = coins.map(({ token, dtime }) => {
    const generation = generationOf(token);
    if (!generation) {
      throw new Error(
        `moth returned an available DUST UTXO (nonce ${toHex(token.nonce)}) without generation info`,
      );
    }
    return {
      dust_output: toDustOutput(token),
      generation_info: {
        value: Number(generation.value),
        owner_dust_public_key: toHex(generation.owner),
        nonce: toHex(generation.nonce),
        dtime: dtime ? toSecs(dtime) : LEDGER_TIMESTAMP_MAX_SECS,
      },
    };
  });

  const source = Object.fromEntries(
    coins.map(({ token, generatedNow }) => [toHex(token.nonce), Number(generatedNow)]),
  );

  return {
    generation_infos,
    source,
    total: Number(dust.balance(now)),
  };
};
