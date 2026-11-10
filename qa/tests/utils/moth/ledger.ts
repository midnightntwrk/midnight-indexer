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

// Which ledger train the target chain runs.
//
// WHY. The wallet SDK moth runs on (2.x) follows a chain across its ledger-8 ->
// ledger-9 hard fork on its own: every wallet runs ledger-v8 below the chain's
// fork version and ledger-v9 from it, so nothing has to be loaded per train
// before a key is derived. What still depends on the train is outside the
// wallet: the proof server image that can prove for the node
// (`utils/moth/proof-server.ts`) and whether the harness's midnight-js 5
// contract path can author transactions the chain accepts
// (`utils/moth/moth-contracts.ts`).
//
// HOW. The indexer reports the chain's protocol version on every block, and
// the wallet SDK's fork schedule says where ledger-v9 begins (2_000_000 on
// every Midnight network). `isLedgerV9` is moth's own reading of that
// schedule, so the harness and the wallets agree on one number. Nothing is
// inferred from NODE_TAG or package.json: the chain is the only authority, and
// a chain that does not answer is an error, not a default.

import type { LedgerVersion } from '@shieldedtech/moth-wallet';
import { forks, isLedgerV9 } from '@shieldedtech/moth-wallet/sync/ledger-routing';
import { ProtocolVersion } from '@midnightntwrk/wallet-sdk';
import log from '@utils/logging/logger';
import { IndexerHttpClient } from '../indexer/http-client';
import { env } from '../../environment/model';

let loading: Promise<LedgerVersion> | undefined;

const detect = async (): Promise<LedgerVersion> => {
  const indexerUrl = env.getIndexerGraphqlHttpURL();
  const response = await new IndexerHttpClient().getLatestBlock();
  const observed = response.data?.block?.protocolVersion;
  if (typeof observed !== 'number' || !Number.isFinite(observed) || observed <= 0) {
    const errors = response.errors?.map((e) => e.message).join('; ');
    throw new Error(
      `The indexer at ${indexerUrl} did not report a protocol version on its latest block` +
        `${errors ? ` (${errors})` : ''}, so the ledger train ${env.getNetworkId()} runs is unknown. ` +
        'The moth backend does not guess: check the indexer is up and has indexed at least one block.',
    );
  }
  const version = isLedgerV9(ProtocolVersion.ProtocolVersion(BigInt(observed))) ? 'v9' : 'v8';
  log.info(
    `moth ledger: ${version} (protocol version ${observed} reported by ${indexerUrl}; ` +
      `ledger-v9 from ${forks.v9})`,
  );
  return version;
};

/**
 * The ledger train the target chain runs, as the indexer reports it. Detected
 * once per process; every moth entry point that depends on the train (proof
 * server choice, contract routing) awaits this. The wallets themselves need
 * nothing from it: the SDK's forking wallets read the same fork schedule.
 */
export const loadLedger = (): Promise<LedgerVersion> => {
  loading ??= detect().catch((error: unknown) => {
    loading = undefined;
    throw error;
  });
  return loading;
};

/**
 * Whether moth can build contract transactions on this chain. Its contract
 * path (`utils/moth/moth-contracts.ts`) runs on midnight-js 5, whose current
 * era is ledger 9, against artifacts compiled with compactc 0.36.0. A ledger-8
 * chain is that train's retained era and needs the ledger-8 compact-runtime
 * resolvable next to the artifacts, which the harness does not carry, so
 * contract deploys, calls and updates go through the toolkit there (its 1.x
 * images are public, unlike the 2.x ones). Transfers are unaffected: they go
 * through the wallet SDK, which follows the chain across the fork itself.
 */
export const mothHandlesContracts = async (): Promise<boolean> => (await loadLedger()) === 'v9';
