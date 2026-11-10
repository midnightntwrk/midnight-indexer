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

// Contract deploys, circuit calls and maintenance updates driven by midnight-js
// in-process, with moth supplying the wallet.
//
// LEDGER TRAIN. This module runs on midnight-js 5 (5.0.0), whose current
// era is ledger 9: transactions cross to the wallet provider as live
// `@midnightntwrk/ledger-v9` objects, and the compiled contract has to carry
// compact-runtime 0.21.0, which is what compactc 0.36.0 emits. A ledger-8
// chain is midnight-js 5's *retained* era: it needs the ledger-8 compact-runtime
// resolvable next to the artifacts, which this harness does not carry, so
// `mothHandlesContracts()` says no there and the ToolkitWrapper routes contract
// actions through the toolkit container instead — the 1.x toolkit images are
// public, unlike the 2.x ones.
//
// WHY NOT moth's own contract path. moth exports `deployContract` / `callCircuit`,
// but they run on midnight-js 4.1.1, which authors ledger-v8 transactions, and
// their wallet provider (`contract/wallet-provider.ts`, `makeContractWalletProvider`)
// refuses a chain that has handed over to ledger-v9 outright, because such a
// chain can never include a ledger-v8 transaction. moth's *transfer* path is
// ledger-agnostic: it goes balance -> signRecipe -> finalize through the facade,
// the same order a real wallet uses, with the ledger the wallets are acting at.
// This module's wallet provider delegates to that transfer-path balancer, so
// the contract path depends on neither moth's release train nor its
// midnight-js pin.
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
// ONE WASM COPY. `@midnightntwrk/ledger-v9` resolves to a single physical copy
// in node_modules. midnight-js 5.0.0 moves transactions across its providers as
// `LedgerTransaction` handles (bytes plus era and stage) and decodes them with
// its own ledger, so the hand-over does not trip `instanceof` on a second
// copy; but moth's wallet SDK, this harness (`@midnightntwrk/wallet-sdk/ledger/v9`)
// and midnight-js still have to agree on one ledger build, and moth's pinned
// snapshot asks for an older wallet SDK than midnight-js does. The `overrides`
// block in package.json pins the wallet SDK, ledger-v9 and onchain-runtime-v4
// to the releases, next to the ledger-8 pins moth's own transfer path still
// needs; do not remove any of them.

import { dirname, resolve } from 'path';
import {
  createContractMaintenanceTxInterface,
  createUnprovenCallTx,
  deployContract as deployViaMidnightJs,
  submitCallTx,
} from '@midnight-ntwrk/midnight-js-contracts';
import { Transaction, type FinalizedTransaction } from '@midnight-ntwrk/midnight-js-protocol/v9';
import {
  CompiledContract,
  type Awaitable,
  type CircuitContext,
  type CircuitResults,
  type ConstructorResult,
  type Contract,
} from '@midnight-ntwrk/midnight-js-protocol/v9/compact-js';
import {
  sampleSigningKey,
  type SigningKey,
} from '@midnight-ntwrk/midnight-js-protocol/v9/compact-runtime';
import {
  indexerPublicDataProvider,
  type IndexerPublicDataProvider,
} from '@midnight-ntwrk/midnight-js-indexer-public-data-provider';
import { httpClientProofProvider } from '@midnight-ntwrk/midnight-js-http-client-proof-provider';
import { NodeZkConfigProvider } from '@midnight-ntwrk/midnight-js-node-zk-config-provider';
import {
  createLedger9MidnightProvider,
  createLedger9WalletProvider,
  SucceedEntirely,
  type FinalizedTxRead,
  type MidnightProviders,
  type PrivateStateId,
  type PrivateStateProvider,
} from '@midnight-ntwrk/midnight-js-types';
import { Cause, Runtime, type Types } from 'effect';
import * as Rx from 'rxjs';
import log from '@utils/logging/logger';
import { env } from '../../environment/model';
import { ensureProofServer, proofServerMismatchHint } from './proof-server';
import { balanceTransaction } from '@shieldedtech/moth-wallet/sync/operations';
import { unwrapTransaction } from '@shieldedtech/moth-wallet/sync/ledger-routing';
import {
  assertSpendableDust,
  awaitDustRootsCurrent,
  openMothWallet,
  seedFingerprint,
} from './moth-backend';
import { mothHandlesContracts } from './ledger';

/**
 * The compiled contract this suite deploys: `counter.compact`, whose single
 * circuit `increment()` is the call key `callContract()` already defaults to.
 *
 * Built with compactc 0.36.0, which emits `checkRuntimeVersion('0.21.0')` to
 * match the `@midnight-ntwrk/compact-runtime` 0.21.0 that midnight-js 5.0.0
 * pins, and ships the integrity manifest the ZK config provider verifies the
 * keys against. An older build (0.16.0 from compactc 0.31.1) will not load here.
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
 * The 30 minutes moth gives the intent it adds while balancing. midnight-js
 * builds the transaction with the same lifetime so the two never disagree about
 * when it expires.
 */
const TX_TTL_SECONDS = 30 * 60;

/**
 * The shape of contract this module drives: no private state, no witnesses,
 * and neither the constructor nor any circuit takes arguments. That is what
 * `callCircuitViaMoth(seed, address, circuitId)` can express — it passes a
 * circuit id and nothing else — and it is what selects midnight-js's
 * argument-free, private-state-free overloads, so no option object needs a
 * `privateStateId` or an `args` the harness could not fill in.
 */
interface ArgumentlessContract extends Contract<undefined> {
  readonly provableCircuits: Record<
    string,
    (context: CircuitContext) => Awaitable<CircuitResults<unknown>>
  >;
  initialState(context: CircuitContext): Awaitable<ConstructorResult<undefined>>;
}

/** The compiled binding of an {@link ArgumentlessContract}, with its witnesses and assets bound. */
type MothCompiledContract = CompiledContract.CompiledContract<ArgumentlessContract, undefined>;

/**
 * The provider set the contract functions below hand to midnight-js. The
 * public data provider is the indexer one rather than the abstract interface
 * because teardown has to reach its `dispose()`.
 */
interface MothContractProviders extends MidnightProviders<string, PrivateStateId, unknown> {
  readonly publicDataProvider: IndexerPublicDataProvider;
}

/**
 * midnight-js providers backed by a moth-synced wallet, plus the loaded
 * contract binding. Opened once per (seed, artifact) pair — building the
 * providers is cheap, but `openMothWallet` behind it is not, so it is shared.
 */
interface ContractContext {
  readonly providers: MothContractProviders;
  readonly compiledContract: MothCompiledContract;
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
const loadCompiledContract = async (artifactDir: string): Promise<MothCompiledContract> => {
  const modulePath = resolve(artifactDir, 'contract', 'index.js');
  // A dynamic import of a path built at run time is untyped; this is the one
  // member of a compactc module the binding needs.
  const contractModule = (await import(modulePath)) as {
    Contract: Types.Ctor<ArgumentlessContract>;
  };
  const name = dirname(artifactDir).split('/').pop() ?? 'contract';
  return CompiledContract.make(name, contractModule.Contract).pipe(
    CompiledContract.withVacantWitnesses,
    CompiledContract.withCompiledFileAssets(artifactDir),
  );
};

/**
 * Build the midnight-js provider set on top of moth's synced facade.
 *
 * `balanceTx` hands the unbound transaction to moth's `balanceTransaction`,
 * the balance -> signRecipe -> finalize sequence its transfer path uses. moth's
 * own contract path does not, and that is moth issue #119 (node error 192).
 */
const openContractContext = (seed: string, artifactDir: string): Promise<ContractContext> => {
  // The seed's hash, not the seed: the key must never hold the secret, and a
  // raw prefix collides for zero-padded seeds, which would sign with the
  // wrong wallet.
  const key = `${artifactDir}::${seedFingerprint(seed)}`;
  const existing = contexts.get(key);
  if (existing) return existing;

  const opening = (async (): Promise<ContractContext> => {
    if (!(await mothHandlesContracts())) {
      throw new Error(
        'moth builds contract transactions through midnight-js 5, whose current era is ledger 9, ' +
          'and this chain runs ledger v8. Route contract actions through the toolkit ' +
          '(ToolkitWrapper does so on its own when TX_BACKEND=moth meets a ledger-8 chain).',
      );
    }
    const networkId = env.getNetworkId();

    const wallet = await openMothWallet(seed);
    await assertSpendableDust(wallet, 'build contract transactions');
    const facade = wallet.synced.facade;

    const state = await Rx.firstValueFrom(facade.state());
    const coinPublicKey = state.shielded.coinPublicKey.toHexString();
    const encryptionPublicKey = state.shielded.encryptionPublicKey.toHexString();

    // midnight-js 5 hands over the unbound transaction as a ledger-9 handle and
    // takes back either bytes or a ledger object it can serialize, which the
    // ledger-9 adapter seals into the finalized handle the submit step wants.
    // moth's balancer speaks bytes, so the handle is serialized on the way in;
    // what comes back is a `WalletTransaction` handle sealed at the version the
    // wallets act at, and the ledger object inside it is unwrapped for
    // midnight-js. moth picks the TTL itself, so the one midnight-js offers is
    // not forwarded.
    //
    // Balancing adds the DUST fee spend, proven against the wallet's dust
    // trees, so those must carry the chain's current roots first: right after
    // a spend of its own (a deploy, then the call that follows it) the wallet
    // still holds the tree it spent against until its next projections pass,
    // and the node refuses a proof on that root (`awaitDustRootsCurrent`).
    const walletProvider = createLedger9WalletProvider({
      getCoinPublicKey: () => coinPublicKey,
      getEncryptionPublicKey: () => encryptionPublicKey,
      balanceTx: async (tx) => {
        await awaitDustRootsCurrent(wallet, 'balance the contract transaction');
        return unwrapTransaction<FinalizedTransaction>(
          await balanceTransaction(facade, wallet.keys, networkId, tx.serialize(), false),
        );
      },
    });
    // The facade only accepts WalletTransaction handles stamped with a
    // protocol version; handed bare transaction bytes it refuses them as
    // "built for protocol version undefined" before anything reaches the node.
    // `adoptTransaction` reads the bytes of the midnight-js handle with the
    // ledger the wallets are acting at and seals them.
    //
    // What `submitTx` resolves to matters: midnight-js waits for the
    // transaction by polling the indexer with `transactions(offset:
    // {identifier})`, so it must be the facade's intent identifier. moth's own
    // `submitFinalizedTransaction` resolves to the transaction *hash* instead,
    // which that query never matches, and the deploy then sits in the wait
    // until the suite's hook times out.
    const midnightProvider = createLedger9MidnightProvider(async (tx) => {
      try {
        return String(
          await facade.submitTransaction(facade.adoptTransaction(tx.serialize(), 'Finalized')),
        );
      } catch (err) {
        // midnight-js redacts the provider's failure to "submitTx rejected the
        // transaction", so the node's own reason (a `1010 Custom error: N`
        // ledger code, a protocol-version mismatch) is only visible here.
        log.error(`moth facade refused to submit the transaction: ${errorChain(err)}`);
        throw err;
      }
    });

    const zkConfigProvider = new NodeZkConfigProvider<string>(artifactDir);
    const publicDataProvider = indexerPublicDataProvider({
      queryURL: env.getIndexerGraphqlHttpURL(),
      subscriptionURL: env.getIndexerGraphqlWsURL(),
    });
    const providers: MothContractProviders = {
      publicDataProvider,
      zkConfigProvider,
      proofProvider: httpClientProofProvider({ url: await ensureProofServer(), zkConfigProvider }),
      walletProvider,
      midnightProvider,
      // Required even for a contract with no private state: the deploy stores
      // the contract's signing key in it, and the maintenance update reads it
      // back. In-memory, because nothing here needs to outlive the process and
      // a LevelDB provider would leave a directory behind per run.
      privateStateProvider: inMemoryPrivateStateProvider(),
      // midnight-js 5 reads the network off the provider set rather than a
      // process-wide `setNetworkId`.
      config: { networkId, ttlSeconds: TX_TTL_SECONDS },
    };

    const compiledContract = await loadCompiledContract(artifactDir);
    return { providers, compiledContract, coinPublicKey, encryptionPublicKey };
  })();

  contexts.set(key, opening);
  opening.catch(() => contexts.delete(key));
  return opening;
};

/**
 * The smallest private-state provider midnight-js 5 will accept. It ships a
 * LevelDB one; this suite has no private state to keep, so an in-memory map
 * avoids leaving a database directory behind on every run. Private states are
 * reached through a per-contract view (`forContract`), signing keys sit on the
 * provider itself, and both are keyed on the lower-cased address, as the
 * LevelDB provider compares them. The export/import surface exists for wallet
 * migrations and is never reached from here, so it refuses rather than
 * pretending.
 */
const inMemoryPrivateStateProvider = (): PrivateStateProvider<PrivateStateId, unknown> => {
  const statesByContract = new Map<string, Map<string, unknown>>();
  const signingKeys = new Map<string, SigningKey>();
  const unsupported = (what: string) => async () => {
    throw new Error(`The in-memory private state provider does not ${what}.`);
  };
  const statesOf = (address: string): Map<string, unknown> => {
    const key = address.toLowerCase();
    const existing = statesByContract.get(key);
    if (existing) return existing;
    const created = new Map<string, unknown>();
    statesByContract.set(key, created);
    return created;
  };
  return {
    forContract: (address: string) => {
      const states = statesOf(address);
      return {
        contractAddress: address.toLowerCase(),
        set: async (key: string, state: unknown) => void states.set(key, state),
        get: async (key: string) => states.get(key) ?? null,
        remove: async (key: string) => void states.delete(key),
        clear: async () => void states.clear(),
        exportPrivateStates: unsupported('export private states'),
        importPrivateStates: unsupported('import private states'),
      };
    },
    clearAll: async () => void statesByContract.clear(),
    setSigningKey: async (address: string, signingKey: SigningKey) =>
      void signingKeys.set(address.toLowerCase(), signingKey),
    getSigningKey: async (address: string) => signingKeys.get(address.toLowerCase()) ?? null,
    removeSigningKey: async (address: string) => void signingKeys.delete(address.toLowerCase()),
    clearSigningKeys: async () => void signingKeys.clear(),
    exportSigningKeys: unsupported('export signing keys'),
    importSigningKeys: unsupported('import signing keys'),
  };
};

/**
 * An error and every `cause` beneath it, one message per link, for a log line.
 * The wallet SDK nests the node's rejection two or three causes down, and
 * midnight-js hides all of them.
 */
const errorChain = (err: unknown): string => {
  const links: string[] = [];
  let current: unknown = err;
  for (let depth = 0; current !== undefined && current !== null && depth < 8; depth++) {
    // An Effect `FiberFailure` keeps its cause under a symbol, not `.cause`;
    // `squash` yields the error the fiber actually failed with.
    if (Runtime.isFiberFailure(current)) {
      current = Cause.squash(current[Runtime.FiberFailureCauseId]);
      continue;
    }
    const e = current as { name?: string; _tag?: string; message?: string; cause?: unknown };
    links.push(`${e.name ?? e._tag ?? typeof current}: ${e.message ?? String(current)}`);
    current = e.cause;
  }
  return links.join(' <- ');
};

/**
 * Wrap a build/submit failure so it can never be mistaken for the indexer
 * reporting the wrong thing. The suite tests the indexer; an ambiguous failure
 * from the transaction generator would point nowhere.
 */
const asBackendFailure = (what: string, err: unknown): Error => {
  const message = (err as Error).message;
  // Only for genuine proving failures. An earlier version matched /circuit/
  // too, which appended a ledger-train explanation to "Circuit 'store' is
  // undefined" — a plain contract/call-key mismatch that has nothing to do with
  // the proof server, and the wrong hint sends the reader down the wrong path.
  const hint = /prov(e|ing|er)|zk(ir|config)?\b/i.test(message)
    ? ` ${proofServerMismatchHint()}`
    : '';
  return new Error(
    `moth/midnight-js backend failed to ${what} (not an indexer failure): ${message}${hint}`,
  );
};

/**
 * midnight-js resolves a submit once the transaction is in a block, whether or
 * not it applied, so a rejected transaction still comes back with a hash. The
 * suite must not go on to assert the indexer against one of those. The status
 * fields sit on both era variants of the record, so no narrowing is needed.
 */
const ensureApplied = (what: string, finalized: FinalizedTxRead): string => {
  if (finalized.status !== SucceedEntirely) {
    throw new Error(`${what} landed with status ${finalized.status}`);
  }
  if (!finalized.txHash) {
    throw new Error(`${what} reported no transaction hash`);
  }
  return finalized.txHash;
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
    });
    const contractAddress = deployed.deployTxData.public.contractAddress;
    const txHash = ensureApplied('the deploy', deployed.deployTxData.public);
    log.info(`moth/midnight-js deployed contract ${contractAddress}`);
    return { contractAddress, txHash };
  } catch (err) {
    throw asBackendFailure('deploy the contract', err);
  }
};

/**
 * Call one circuit on a deployed contract and return the submitted tx hash.
 *
 * midnight-js reads the contract's current public state from the indexer,
 * runs the circuit against it and submits once — unlike the segment-probe,
 * which deliberately builds twice from one snapshot to force a stale call. Here
 * a plain, applied call is what the indexer assertions want.
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
    const finalized = await submitCallTx(ctx.providers, {
      compiledContract: ctx.compiledContract,
      contractAddress,
      circuitId,
    });
    const txHash = ensureApplied(`the ${circuitId} call`, finalized.public);
    log.info(`moth/midnight-js called ${circuitId} on ${contractAddress}`);
    return txHash;
  } catch (err) {
    throw asBackendFailure(`call circuit ${circuitId}`, err);
  }
};

/**
 * Build one circuit call against the contract's current on-chain state without
 * proving or submitting it, and return the unproven transaction as the ledger
 * prints it.
 *
 * The moth counterpart of building a call with the toolkit and decoding it with
 * `show-transaction`: a suite can check a fixture really produces the effect
 * under test from the transaction itself, independently of the indexer. The
 * call is built against live state, so build it only once the previous call is
 * in a block, and expect {@link callCircuitViaMoth} to build its own copy.
 *
 * @param seed - Funding seed for the calling wallet. Never logged.
 * @param contractAddress - Address returned by {@link deployContractViaMoth}.
 * @param circuitId - Circuit to call.
 * @param artifactDir - The same `managed/` directory the contract was deployed from.
 */
export const describeUnprovenCallViaMoth = async (
  seed: string,
  contractAddress: string,
  circuitId: string,
  artifactDir: string = DEFAULT_ARTIFACT_DIR,
): Promise<string> => {
  const ctx = await openContractContext(seed, artifactDir);
  try {
    const unsubmitted = await createUnprovenCallTx(ctx.providers, {
      compiledContract: ctx.compiledContract,
      contractAddress,
      circuitId,
    });
    // `unprovenTx` is a handle; decode it with the ledger-9 copy to print it.
    return unsubmitted.private.unprovenTx.deserialize({ v9: Transaction }).toString();
  } catch (err) {
    throw asBackendFailure(`build circuit ${circuitId}`, err);
  }
};

/**
 * Replace a contract's maintenance authority — the toolkit's `maintenance`
 * command, and the only contract action of the three that changes who may
 * maintain the contract rather than its state.
 *
 * The deploy stored its signing key in the private-state provider, which is
 * in-memory, so this only works in the same process that deployed the contract.
 * That matches how the suite uses it: deploy, call and update run in one
 * sequential describe block.
 *
 * @param seed - Funding seed for the wallet paying for the update. Never logged.
 * @param contractAddress - Address returned by {@link deployContractViaMoth}.
 * @param newAuthority - The signing key to hand authority to. Defaults to a
 *                       fresh sample key; the suite only checks that the update
 *                       is indexed, not who ends up holding it.
 * @param artifactDir - The same `managed/` directory the contract was deployed from.
 */
export const updateContractViaMoth = async (
  seed: string,
  contractAddress: string,
  newAuthority: SigningKey = sampleSigningKey(),
  artifactDir: string = DEFAULT_ARTIFACT_DIR,
): Promise<string> => {
  const ctx = await openContractContext(seed, artifactDir);
  try {
    const maintenance = createContractMaintenanceTxInterface(
      ctx.providers,
      ctx.compiledContract,
      contractAddress,
    );
    const finalized = await maintenance.replaceAuthority(newAuthority);
    const txHash = ensureApplied('the authority update', finalized.public);
    log.info(`moth/midnight-js replaced the authority on ${contractAddress}`);
    return txHash;
  } catch (err) {
    throw asBackendFailure('replace the contract maintenance authority', err);
  }
};

/**
 * Drop the cached provider sets so no caller can reuse one that wraps a
 * stopped facade, and release each set's indexer websocket. Call it right
 * before `closeMothWallets()`, as `ToolkitWrapper.stop()` and the moth-only
 * suites do; the wallets themselves stay owned by `moth-backend`, which is why
 * this does not stop them.
 */
export const closeMothContractContexts = (): void => {
  for (const opening of contexts.values()) {
    // Best effort: a context that never finished opening has nothing to release,
    // and a websocket that is already gone is not worth failing teardown over.
    void opening.then((ctx) => ctx.providers.publicDataProvider.dispose()).catch(() => undefined);
  }
  contexts.clear();
};
