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

// Contract deploys and circuit calls driven by midnight-js in-process, with
// moth supplying the wallet.
//
// WHY NOT moth's own contract path. moth exports `deployContract` / `callCircuit`,
// but they build a wallet provider whose `balanceTx` hand-rolls
// `signTransactionIntents` over a WASM wrapper that `finalizeRecipe` never sees
// (moth issue #119, surfacing as node error 192). The fix is merged but
// unreleased. moth's *transfer* path does not do this — it goes
// balance -> signRecipe -> finalize, the same order a real wallet uses. This
// module builds a midnight-js `walletProvider` in that same order, so the
// contract path does not depend on moth's release train at all.
//
// This is the shape proven on preprod on 2026-09-14 by the segment-probe
// investigation (`segment-probe/src/moth-wallet.ts`), ported here.
//
// WHY NOT the DApp connector. midnight-js only asks for an object with
// `getCoinPublicKey` / `getEncryptionPublicKey` / `balanceTx` / `submitTx`. In a
// browser the connector implements that by messaging a wallet extension; in
// Node we implement it directly against moth's facade. There is no connector
// and no IPC involved.
//
// ONE WASM COPY. `@midnight-ntwrk/ledger-v8` and `onchain-runtime-v3` must each
// resolve to a single physical copy in node_modules, or objects built by one
// copy fail `instanceof` in the other (separate WASM linear memories). The
// `overrides` block in package.json pins both; do not remove it.

import { dirname, resolve } from 'path';
import { createKeystore } from '@midnightntwrk/wallet-sdk/unshielded';
import {
  createUnprovenCallTxFromInitialStates,
  deployContract as deployViaMidnightJs,
  getPublicStates,
  submitTx,
} from '@midnight-ntwrk/midnight-js-contracts';
import { CompiledContract } from '@midnight-ntwrk/midnight-js-protocol/compact-js';
import { sampleSigningKey } from '@midnight-ntwrk/midnight-js-protocol/compact-runtime';
import { indexerPublicDataProvider } from '@midnight-ntwrk/midnight-js-indexer-public-data-provider';
import { httpClientProofProvider } from '@midnight-ntwrk/midnight-js-http-client-proof-provider';
import { NodeZkConfigProvider } from '@midnight-ntwrk/midnight-js-node-zk-config-provider';
import { setNetworkId } from '@midnight-ntwrk/midnight-js-network-id';
import * as Rx from 'rxjs';
import log from '@utils/logging/logger';
import { env } from '../../environment/model';
import { ensureProofServer, proofServerMismatchHint } from './proof-server';
import { openMothWallet } from './moth-backend';

/**
 * The compiled contract this suite deploys: `counter.compact`, whose single
 * circuit `increment()` is the call key `callContract()` already defaults to.
 *
 * Built with compactc 0.31.1, which emits `checkRuntimeVersion('0.16.0')` to
 * match the `@midnight-ntwrk/compact-runtime` 0.16.0 that midnight-js 4.1.1
 * pins. A 0.30.0 build emits 0.15.0 and will not load here.
 */
export const DEFAULT_ARTIFACT_DIR = resolve(__dirname, '../../contracts/counter/managed');

/** The only circuit `counter.compact` exposes. */
export const DEFAULT_CIRCUIT_ID = 'increment';

/** Shape returned by a deploy, mirroring the toolkit's `DeployContractResult` keys. */
export interface MothDeployResult {
  readonly contractAddress: string;
  readonly txHash: string;
}

/**
 * midnight-js providers backed by a moth-synced wallet, plus the loaded
 * contract binding. Opened once per (seed, artifact) pair — building the
 * providers is cheap, but `openMothWallet` behind it is not, so it is shared.
 */
interface ContractContext {
  readonly providers: any;
  readonly compiledContract: any;
  readonly coinPublicKey: string;
  readonly encryptionPublicKey: string;
}

const contexts = new Map<string, Promise<ContractContext>>();

/**
 * Load the compiled contract module out of a `managed/` directory and wrap it
 * as a midnight-js `CompiledContract`.
 *
 * The directory is an input rather than a constant so the suite is not tied to
 * one contract: any `compact compile` output with the circuits under test works.
 * `withVacantWitnesses` is correct only for a contract with no witnesses; a
 * contract that has them needs its own binding.
 */
const loadCompiledContract = async (artifactDir: string): Promise<any> => {
  const modulePath = resolve(artifactDir, 'contract', 'index.js');
  const contractModule = await import(modulePath);
  const name = dirname(artifactDir).split('/').pop() ?? 'contract';
  return CompiledContract.make(name, contractModule.Contract).pipe(
    CompiledContract.withVacantWitnesses,
    CompiledContract.withCompiledFileAssets(artifactDir),
  );
};

/**
 * Build the midnight-js provider set on top of moth's synced facade.
 *
 * `balanceTx` deliberately runs balance -> signRecipe -> finalize. That is the
 * order moth's transfer path and testkit both use; moth's own contract path
 * does not, and that is moth issue #119 (node error 192).
 */
const openContractContext = (seed: string, artifactDir: string): Promise<ContractContext> => {
  // The seed prefix, not the seed: two different seeds against one artifact
  // must not share a context, but the key is never secret enough to hold more.
  const key = `${artifactDir}::${seed.slice(0, 8)}`;
  const existing = contexts.get(key);
  if (existing) return existing;

  const opening = (async (): Promise<ContractContext> => {
    const networkId = env.getNetworkId();
    setNetworkId(networkId as any);

    const wallet = await openMothWallet(seed);
    const facade = wallet.synced.facade as any;

    const state = await Rx.firstValueFrom(facade.state() as Rx.Observable<any>);
    const coinPublicKey = state.shielded.coinPublicKey.toHexString() as string;
    const encryptionPublicKey = state.shielded.encryptionPublicKey.toHexString() as string;

    const keystore = createKeystore((wallet.keys as any).nightExternalKey, networkId);
    const walletProvider = {
      getCoinPublicKey: () => coinPublicKey,
      getEncryptionPublicKey: () => encryptionPublicKey,
      async balanceTx(tx: any, ttl?: Date) {
        const recipe = await facade.balanceUnboundTransaction(
          tx,
          {
            shieldedSecretKeys: (wallet.keys as any).shieldedSecretKeys,
            dustSecretKey: (wallet.keys as any).dustSecretKey,
          },
          { ttl: ttl ?? new Date(Date.now() + 60 * 60_000) },
        );
        const signed = await facade.signRecipe(recipe, (payload: Uint8Array) =>
          keystore.signData(payload),
        );
        return facade.finalizeRecipe(signed);
      },
      submitTx: (tx: any) => facade.submitTransaction(tx),
    };

    const zkConfigProvider = new NodeZkConfigProvider<string>(artifactDir);
    const providers = {
      publicDataProvider: indexerPublicDataProvider(
        env.getIndexerGraphqlHttpURL(),
        env.getIndexerGraphqlWsURL(),
      ),
      zkConfigProvider,
      proofProvider: httpClientProofProvider(await ensureProofServer(), zkConfigProvider as any),
      walletProvider,
      midnightProvider: walletProvider,
      // Required even for a contract with no private state: `submitDeployTx`
      // calls `setContractAddress()` on it unconditionally. In-memory, because
      // nothing here needs to outlive the process and a LevelDB provider would
      // leave a directory behind per run.
      privateStateProvider: inMemoryPrivateStateProvider(),
    };

    const compiledContract = await loadCompiledContract(artifactDir);
    return { providers, compiledContract, coinPublicKey, encryptionPublicKey };
  })();

  contexts.set(key, opening);
  opening.catch(() => contexts.delete(key));
  return opening;
};

/**
 * The smallest private-state provider `submitDeployTx` will accept. midnight-js
 * ships a LevelDB one; this suite has no private state to keep, so an in-memory
 * map avoids leaving a database directory behind on every run.
 */
const inMemoryPrivateStateProvider = () => {
  const states = new Map<string, unknown>();
  const addresses = new Map<string, string>();
  return {
    set: async (key: string, state: unknown) => void states.set(key, state),
    get: async (key: string) => states.get(key) ?? null,
    remove: async (key: string) => void states.delete(key),
    clear: async () => void states.clear(),
    setSigningKey: async (address: string, signingKey: string) =>
      void addresses.set(address, signingKey),
    getSigningKey: async (address: string) => addresses.get(address) ?? null,
    removeSigningKey: async (address: string) => void addresses.delete(address),
    clearSigningKeys: async () => void addresses.clear(),
    setContractAddress: async (key: string, address: string) => void addresses.set(key, address),
  };
};

/**
 * Wrap a build/submit failure so it can never be mistaken for the indexer
 * reporting the wrong thing. The suite tests the indexer; an ambiguous failure
 * from the transaction generator would point nowhere.
 */
const asBackendFailure = (what: string, err: unknown): Error => {
  const message = (err as Error).message;
  const hint = /prov(e|ing)|zk|circuit/i.test(message) ? ` ${proofServerMismatchHint()}` : '';
  return new Error(
    `moth/midnight-js backend failed to ${what} (not an indexer failure): ${message}${hint}`,
  );
};

/**
 * Deploy a compiled contract and return its address and deploy transaction hash.
 *
 * @param seed - Funding seed for the deploying wallet. Never logged.
 * @param artifactDir - A `compact compile` `managed/` directory. Defaults to the
 *                      counter contract committed under `qa/tests/contracts`.
 */
export const deployContractViaMoth = async (
  seed: string,
  artifactDir: string = DEFAULT_ARTIFACT_DIR,
): Promise<MothDeployResult> => {
  const ctx = await openContractContext(seed, artifactDir);
  try {
    const deployed = await deployViaMidnightJs(ctx.providers, {
      compiledContract: ctx.compiledContract,
      signingKey: sampleSigningKey(),
      initialPrivateState: undefined,
    } as any);
    const contractAddress = deployed.deployTxData.public.contractAddress as string;
    log.info(`moth/midnight-js deployed contract ${contractAddress}`);
    return { contractAddress, txHash: deployed.deployTxData.public.txHash as string };
  } catch (err) {
    throw asBackendFailure('deploy the contract', err);
  }
};

/**
 * Call one circuit on a deployed contract and return the submitted tx hash.
 *
 * Builds from the contract's current public state and submits once — unlike the
 * segment-probe, which deliberately builds twice from one snapshot to force a
 * stale call. Here a plain, applied call is what the indexer assertions want.
 *
 * @param seed - Funding seed for the calling wallet. Never logged.
 * @param contractAddress - Address returned by {@link deployContractViaMoth}.
 * @param circuitId - Circuit to call. Defaults to the counter's `increment`.
 * @param artifactDir - The same `managed/` directory the contract was deployed from.
 */
export const callCircuitViaMoth = async (
  seed: string,
  contractAddress: string,
  circuitId: string = DEFAULT_CIRCUIT_ID,
  artifactDir: string = DEFAULT_ARTIFACT_DIR,
): Promise<string> => {
  const ctx = await openContractContext(seed, artifactDir);
  try {
    const snapshot = await getPublicStates(ctx.providers.publicDataProvider, contractAddress);
    const call = await createUnprovenCallTxFromInitialStates(
      ctx.providers.zkConfigProvider,
      {
        compiledContract: ctx.compiledContract,
        contractAddress,
        circuitId,
        args: [],
        coinPublicKey: ctx.coinPublicKey,
        initialContractState: snapshot.contractState,
        initialZswapChainState: snapshot.zswapChainState,
        ledgerParameters: snapshot.ledgerParameters,
      } as any,
      ctx.encryptionPublicKey,
    );
    const submitted = await submitTx(ctx.providers, {
      unprovenTx: call.private.unprovenTx,
      circuitId,
    });
    log.info(`moth/midnight-js called ${circuitId} on ${contractAddress}`);
    return submitted.txHash as string;
  } catch (err) {
    throw asBackendFailure(`call circuit ${circuitId}`, err);
  }
};

/**
 * Drop the cached provider sets. The moth wallets behind them are owned by
 * `moth-backend`, which closes them in its own teardown — releasing them here
 * would pull a wallet out from under the transfer path.
 */
export const closeMothContractContexts = (): void => {
  contexts.clear();
};
