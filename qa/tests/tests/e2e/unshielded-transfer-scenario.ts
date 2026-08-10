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

// The unshielded transfer scenario, shared by the two unshielded e2e suites:
// night-transactions.test.ts (the native NIGHT token) and
// unshielded-transactions.test.ts (a contract-minted custom token). Both suites
// exercise the same transfer and the same indexer surfaces; the token type is the
// only parameter, so the coverage cannot silently drift apart between them.
//
// Not a *.test.ts file, so it is never collected on its own — it only defines
// tests when a suite calls defineUnshieldedTransferTests.

import type { TestContext } from 'vitest';
import log from '@utils/logging/logger';
import '@utils/logging/test-logging-hooks';
import { retry } from '@utils/retry-helper';
import dataProvider from '@utils/testdata-provider';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { IndexerWsClient, UnshieldedTxSubscriptionResponse } from '@utils/indexer/websocket-client';
import { ToolkitWrapper, ToolkitTransactionResult } from '@utils/toolkit/toolkit-wrapper';
import {
  Transaction,
  UnshieldedTransaction,
  UnshieldedTransactionEvent,
  UnshieldedTransactionsProgress,
  UnshieldedUtxo,
} from '@utils/indexer/indexer-types';
import {
  getBlockByHashWithRetry,
  resolveBlockHash,
  setupWalletEventSubscriptions,
} from './test-utils';

/** Timeout for the suite and for the transfer that sets it up. */
export const UNSHIELDED_TRANSFER_TIMEOUT = 200_000;

/**
 * The progress subscription backs off while idle (since indexer 4.4.0: 30s doubling
 * up to 240s, ±20% jitter), so a subscription opened well before the transaction may
 * only report the change after the full backed-off gap — about five minutes worst
 * case. Hence the wide window on the two progress tests.
 */
const PROGRESS_TIMEOUT = 400_000;

/** Identifies a shared test, so a suite can attach its Xray key to the right one. */
export type UnshieldedTransferTestId =
  | 'blockQueryByHash'
  | 'transactionQueryByHash'
  | 'sourceTransactionEvent'
  | 'destinationTransactionEvent'
  | 'transferredAmount'
  | 'tokenTypeOnOutputs'
  | 'sourceProgressUpdate'
  | 'destinationProgressUpdate';

/** The unshielded token a suite exercises the scenario with. */
export interface UnshieldedTokenUnderTest {
  /** Short name used in test titles and in each test's `labels`, e.g. `NIGHT`. */
  label: string;
  /**
   * Hex token type to transfer, and the type every UTXO of the transfer must carry.
   * A suite that picks its token at runtime may replace it from `prepare`, which runs
   * before the transfer is submitted.
   */
  tokenType: string;
  /** Amount to transfer, with the unit name used in test titles (e.g. `1` / `STAR`). */
  amount: number;
  unit: string;
  /**
   * Seed of the receiving wallet, which must be distinct from every other e2e suite's
   * destination. The e2e files run concurrently against one chain, so a shared
   * destination lets one suite's transfer show up in the other's event stream — and
   * where a suite asserts that a wallet received no transaction event at all
   * (wallet-subscriptions.test.ts does), that turns into a false failure over there.
   */
  destinationSeed: string;
  /**
   * Seeds of further wallets to subscribe alongside the destination, exposed as
   * `wallet.destinations[1..]`. A suite adds them when it also asserts on how the
   * indexer routes a transfer between several subscribed recipients.
   */
  extraDestinationSeeds?: string[];
  /** Xray test keys by test id. Omitted by suites with no Xray coverage. */
  testKeys?: Partial<Record<UnshieldedTransferTestId, string>>;
  /**
   * Runs once the wallets are subscribed and before the transfer is submitted —
   * the place to capture pre-transfer baselines, and the place to decide whether
   * the environment can run the scenario at all.
   *
   * @returns a reason to skip every test, or null when the scenario can run.
   */
  prepare?: (scenario: UnshieldedTransferScenario) => Promise<string | null>;
}

/** Shared state of one transfer, populated by the scenario's `beforeAll`. */
export interface UnshieldedTransferScenario {
  token: UnshieldedTokenUnderTest;
  httpClient: IndexerHttpClient;
  wsClient: IndexerWsClient;
  toolkit: ToolkitWrapper;
  wallet: Awaited<ReturnType<typeof setupWalletEventSubscriptions>>;
  transactionResult: ToolkitTransactionResult;
  /** Non-null when the environment cannot run the scenario; every test skips with it. */
  skipReason: string | null;
}

/**
 * Registers the scenario's `beforeAll`/`afterAll` hooks: connect, start the toolkit,
 * subscribe both wallets, then transfer `amount` of the token under test from the
 * environment's funding wallet to the destination wallet.
 *
 * @param token - The token to exercise the scenario with.
 * @returns The scenario state, readable from test bodies once `beforeAll` has run.
 */
export function setupUnshieldedTransferScenario(
  token: UnshieldedTokenUnderTest,
): UnshieldedTransferScenario {
  // Every other field is assigned by the beforeAll below, before any test body reads it.
  const scenario = { token, skipReason: null } as UnshieldedTransferScenario;

  beforeAll(async () => {
    scenario.httpClient = new IndexerHttpClient();
    scenario.wsClient = new IndexerWsClient();
    await scenario.wsClient.connectionInit();

    scenario.toolkit = new ToolkitWrapper({});
    await scenario.toolkit.start();

    const sourceSeed = dataProvider.getFundingSeed();
    scenario.wallet = await setupWalletEventSubscriptions(
      scenario.toolkit,
      scenario.wsClient,
      sourceSeed,
      [token.destinationSeed, ...(token.extraDestinationSeeds ?? [])],
    );

    scenario.skipReason = (await token.prepare?.(scenario)) ?? null;
    if (scenario.skipReason !== null) {
      log.warn(`Skipping the ${token.label} transfer scenario: ${scenario.skipReason}`);
      return;
    }

    scenario.transactionResult = await scenario.toolkit.generateSingleTx(
      sourceSeed,
      'unshielded',
      scenario.wallet.destinations[0].destinationAddress,
      token.amount,
      token.tokenType,
    );
    await resolveBlockHash(scenario.transactionResult);
  }, UNSHIELDED_TRANSFER_TIMEOUT);

  afterAll(async () => {
    scenario.wallet?.source.unsubscribe();
    scenario.wallet?.destinations.forEach((destination) => destination.unsubscribe());
    await Promise.all([scenario.toolkit?.stop(), scenario.wsClient?.connectionClose()]);
  });

  return scenario;
}

/**
 * Starts a shared test: attaches its labels and its suite's Xray key, then skips it
 * when the environment could not be prepared.
 */
function startTest(
  scenario: UnshieldedTransferScenario,
  ctx: TestContext,
  id: UnshieldedTransferTestId,
  labels: string[],
): void {
  const testKey = scenario.token.testKeys?.[id];
  ctx.task!.meta.custom = {
    labels: [...labels, 'UnshieldedTokens', scenario.token.label],
    ...(testKey ? { testKey } : {}),
  };
  ctx.skip?.(scenario.skipReason !== null, scenario.skipReason ?? '');
}

/** Skips a test whose subject is the confirmed transaction itself. */
function skipUnlessConfirmed(scenario: UnshieldedTransferScenario, ctx: TestContext): void {
  ctx.skip?.(
    scenario.transactionResult.status !== 'confirmed',
    "Toolkit transaction hasn't been confirmed",
  );
}

/** Returns the transfer's UTXOs that carry the token type under test. */
function utxosOfTokenUnderTest(
  scenario: UnshieldedTransferScenario,
  transaction: Transaction | undefined,
  side: 'unshieldedCreatedOutputs' | 'unshieldedSpentOutputs',
): UnshieldedUtxo[] {
  return (transaction?.[side] ?? []).filter(
    (utxo: UnshieldedUtxo) => utxo.tokenType === scenario.token.tokenType,
  );
}

/**
 * Finds a progress update event reporting a transaction id past the baseline.
 * Used by the source and destination progress tests through `retry`.
 *
 * @param events - The events array to search.
 * @param baselineTransactionId - The transaction ID to compare against.
 * @param addressLabel - Label for error messages ('source' or 'destination').
 * @returns The found event.
 * @throws Error if no matching event is found.
 */
function findProgressUpdateEvent(
  events: UnshieldedTxSubscriptionResponse[],
  baselineTransactionId: number,
  addressLabel: string,
): UnshieldedTxSubscriptionResponse {
  const event = events.find((event) => {
    const txEvent = event.data?.unshieldedTransactions as UnshieldedTransactionEvent;

    log.debug(`waiting for UnshieldedTransactionsProgress event`);
    if (txEvent.__typename === 'UnshieldedTransactionsProgress') {
      const progressUpdate = txEvent;
      log.debug(`progressUpdate received: ${JSON.stringify(progressUpdate, null, 2)}`);
      if (progressUpdate.highestTransactionId > baselineTransactionId) {
        return true;
      }
    }
  });
  if (!event) {
    throw new Error(`${addressLabel} address progress update event not found yet`);
  }
  return event;
}

/** Asserts a progress update past the baseline arrives for one of the two addresses. */
async function expectProgressUpdate(
  events: UnshieldedTxSubscriptionResponse[],
  historicalEvents: UnshieldedTxSubscriptionResponse[],
  addressLabel: string,
): Promise<void> {
  const isProgress = (event: UnshieldedTxSubscriptionResponse) =>
    event.data?.unshieldedTransactions.__typename === 'UnshieldedTransactionsProgress';

  // A wallet that has never transacted still gets an immediate progress update reporting
  // 0, so 0 is the baseline when no historical update was captured.
  const highestTransactionIdBefore =
    (
      historicalEvents.filter(isProgress).at(-1)?.data?.unshieldedTransactions as
        UnshieldedTransactionsProgress | undefined
    )?.highestTransactionId ?? 0;
  log.info(
    `Highest ${addressLabel} transaction ID before transaction: ${highestTransactionIdBefore}`,
  );

  const event = await retry(
    async () => findProgressUpdateEvent(events, highestTransactionIdBefore, addressLabel),
    {
      maxRetries: 60,
      delayMs: 5000,
      retryLabel: `find ${addressLabel} address progress update event`,
    },
  );

  expect(event).toBeDefined();
  const highestTransactionIdAfter = (
    event.data?.unshieldedTransactions as UnshieldedTransactionsProgress
  ).highestTransactionId;
  log.info(
    `Highest ${addressLabel} transaction ID after transaction: ${highestTransactionIdAfter}`,
  );
  expect(highestTransactionIdAfter).toBeGreaterThan(highestTransactionIdBefore);
}

/**
 * Defines the tests every unshielded token type must pass: the transfer is reported
 * by block query, transaction query, both wallets' transaction subscriptions and both
 * wallets' progress updates, with the right amount and the right token type.
 *
 * @param scenario - The scenario returned by `setupUnshieldedTransferScenario`.
 */
export function defineUnshieldedTransferTests(scenario: UnshieldedTransferScenario): void {
  const { amount, unit } = scenario.token;

  describe(`a successful unshielded transaction transferring ${amount} ${unit} between two addresses`, () => {
    /**
     * Once an unshielded transaction has been submitted to node and confirmed, the indexer should
     * report that transaction in the block through a block query by hash, using the block hash
     * reported by the toolkit.
     *
     * @given a confirmed unshielded transaction between two wallets
     * @when the block is queried by the block hash the toolkit reported
     * @then the block should contain the transaction with outputs for both addresses
     */
    test('should be reported by the indexer through a block query by hash', async (ctx: TestContext) => {
      startTest(scenario, ctx, 'blockQueryByHash', ['Query', 'Block', 'ByHash']);
      skipUnlessConfirmed(scenario, ctx);

      // The expected block might take a bit more to show up by indexer, so we retry a few times
      const blockResponse = await getBlockByHashWithRetry(scenario.transactionResult.blockHash);

      expect(blockResponse?.data?.block?.transactions).toBeDefined();
      expect(blockResponse?.data?.block?.transactions?.length).toBeGreaterThan(0);

      const sourceAddresInTx = blockResponse.data?.block?.transactions?.find((tx: Transaction) =>
        tx.unshieldedCreatedOutputs?.find(
          (output: UnshieldedUtxo) => output.owner === scenario.wallet.source.address,
        ),
      );

      const destAddresInTx = blockResponse.data?.block?.transactions?.find((tx: Transaction) =>
        tx.unshieldedCreatedOutputs?.find(
          (output: UnshieldedUtxo) =>
            output.owner === scenario.wallet.destinations[0].destinationAddress,
        ),
      );

      expect(sourceAddresInTx).toBeDefined();
      expect(destAddresInTx).toBeDefined();
    });

    /**
     * Once an unshielded transaction has been submitted to node and confirmed, the indexer should
     * report that transaction through a query by transaction hash, using the transaction hash
     * reported by the toolkit.
     *
     * @given a confirmed unshielded transaction between two wallets
     * @when transactions are queried by the transaction hash
     * @then the returned transactions should include outputs for both addresses involved
     */
    test('should be reported by the indexer through a transaction query by hash', async (ctx: TestContext) => {
      startTest(scenario, ctx, 'transactionQueryByHash', ['Query', 'Transaction', 'ByHash']);
      skipUnlessConfirmed(scenario, ctx);

      // The expected transaction might take a bit more to show up by indexer, so we retry a few times
      const transactionResponse = await scenario.httpClient.getTransactionByOffset({
        hash: scenario.transactionResult.txHash,
      });

      expect(transactionResponse?.data?.transactions).toBeDefined();
      expect(
        transactionResponse?.data?.transactions?.length,
        'No transactions found',
      ).toBeGreaterThan(0);

      const sourceAddresInTx = transactionResponse.data?.transactions?.find((tx: Transaction) =>
        tx.unshieldedCreatedOutputs?.find(
          (output: UnshieldedUtxo) => output.owner === scenario.wallet.source.address,
        ),
      );
      expect(sourceAddresInTx).toBeDefined();

      const destAddresInTx = transactionResponse.data?.transactions?.find((tx: Transaction) =>
        tx.unshieldedCreatedOutputs?.find(
          (output: UnshieldedUtxo) =>
            output.owner === scenario.wallet.destinations[0].destinationAddress,
        ),
      );
      expect(destAddresInTx).toBeDefined();
    });

    /**
     * Once an unshielded transaction has been submitted to node and confirmed, the indexer should
     * report that transaction through an unshielded transaction event for the source address.
     *
     * @given a subscription to unshielded transaction events for the source address
     * @when an unshielded transaction is submitted to node
     * @then a transaction event including created and spent UTXOs for the source address is received
     */
    test('should be reported by the indexer through an unshielded transaction event for the source address', async (ctx: TestContext) => {
      startTest(scenario, ctx, 'sourceTransactionEvent', ['Subscription', 'Transaction']);
      skipUnlessConfirmed(scenario, ctx);

      // The event arrives asynchronously through the subscription, so we retry a few times.
      const sourceAddressEvent = await retry(
        async () => {
          const event = scenario.wallet.source.events.find((event) => {
            const txEvent = event.data?.unshieldedTransactions as UnshieldedTransaction;
            return (
              txEvent.__typename === 'UnshieldedTransaction' &&
              txEvent.createdUtxos?.some(
                (utxo: UnshieldedUtxo) => utxo.owner === scenario.wallet.source.address,
              ) &&
              txEvent.spentUtxos?.some(
                (utxo: UnshieldedUtxo) => utxo.owner === scenario.wallet.source.address,
              )
            );
          });
          if (!event) {
            throw new Error('Source address transaction event not found yet');
          }
          return event;
        },
        {
          maxRetries: 10,
          delayMs: 3000,
          retryLabel: 'find source address transaction event',
        },
      );
      expect(sourceAddressEvent).toBeDefined();
    });

    /**
     * Once an unshielded transaction has been submitted to node and confirmed, the indexer should
     * report that transaction through an unshielded transaction event for the destination address.
     *
     * @given a subscription to unshielded transaction events for the destination address
     * @when an unshielded transaction is submitted to node
     * @then a transaction event including a created UTXO for the destination is received
     */
    test('should be reported by the indexer through an unshielded transaction event for the destination address', async (ctx: TestContext) => {
      startTest(scenario, ctx, 'destinationTransactionEvent', ['Subscription', 'Transaction']);
      skipUnlessConfirmed(scenario, ctx);

      // The event arrives asynchronously through the subscription, so we retry a few times.
      const destinationAddressEvent = await retry(
        async () => {
          const event = scenario.wallet.destinations[0].events.find((event) => {
            const txEvent = event.data?.unshieldedTransactions as UnshieldedTransaction;
            return (
              txEvent.__typename === 'UnshieldedTransaction' &&
              txEvent.createdUtxos?.some(
                (utxo: UnshieldedUtxo) =>
                  utxo.owner === scenario.wallet.destinations[0].destinationAddress,
              )
            );
          });

          if (!event) {
            throw new Error('Destination address transaction event not found yet');
          }
          return event;
        },
        {
          maxRetries: 10,
          delayMs: 3000,
          retryLabel: 'find destination address transaction event',
        },
      );
      expect(destinationAddressEvent).toBeDefined();
    });

    /**
     * A transfer of one unshielded token splits the spent UTXO into the destination's
     * amount and the source's change, so the indexer should report two created outputs
     * and one spent output of that token type.
     *
     * @given a confirmed unshielded transaction between two wallets
     * @when the containing block is inspected for outputs of the token under test
     * @then two created outputs and one spent output reflect the transfer of the amount
     *       sent (1 STAR for NIGHT), with the destination's output holding that amount
     */
    test(`should have transferred ${amount} ${unit} from the source to the destination address`, async (ctx: TestContext) => {
      startTest(scenario, ctx, 'transferredAmount', []);
      skipUnlessConfirmed(scenario, ctx);

      // The expected block might take a bit more to show up by indexer, so we retry a few times
      const blockResponse = await getBlockByHashWithRetry(scenario.transactionResult.blockHash);
      const unshieldedTx = blockResponse.data?.block?.transactions?.find((tx: Transaction) => {
        const hasCreated = tx.unshieldedCreatedOutputs && tx.unshieldedCreatedOutputs.length > 0;
        const hasSpent = tx.unshieldedSpentOutputs && tx.unshieldedSpentOutputs.length > 0;
        log.info(`Transaction ${tx.hash}: hasCreated=${hasCreated}, hasSpent=${hasSpent}`);
        return hasCreated || hasSpent;
      });

      expect(unshieldedTx).toBeDefined();

      const createdOutputs = utxosOfTokenUnderTest(
        scenario,
        unshieldedTx,
        'unshieldedCreatedOutputs',
      );
      expect(createdOutputs).toHaveLength(2);

      const sourceOutput = createdOutputs.find(
        (output) => output.owner === scenario.wallet.source.address,
      );
      const destOutput = createdOutputs.find(
        (output) => output.owner === scenario.wallet.destinations[0].destinationAddress,
      );

      expect(sourceOutput).toBeDefined();
      expect(destOutput).toBeDefined();
      expect(destOutput?.value).toBe(String(amount));

      const spentOutputs = utxosOfTokenUnderTest(scenario, unshieldedTx, 'unshieldedSpentOutputs');
      expect(spentOutputs).toHaveLength(1);
      expect(spentOutputs[0]?.owner).toBe(scenario.wallet.source.address);
    });

    /**
     * A transfer moves one unshielded token only, so every UTXO it creates or spends must
     * carry the token type the transfer was made in — the type the suite asked the toolkit
     * for, never a value read back from the response under test.
     *
     * @given a confirmed unshielded transaction between two wallets
     * @when the created and spent outputs of that transaction are inspected
     * @then every one of them carries the token type under test (0x00…00 for NIGHT), and
     *       no other token type appears
     */
    test('should report the token type under test on every created and spent output', async (ctx: TestContext) => {
      startTest(scenario, ctx, 'tokenTypeOnOutputs', ['Query', 'Block', 'ByHash']);
      skipUnlessConfirmed(scenario, ctx);

      const blockResponse = await getBlockByHashWithRetry(scenario.transactionResult.blockHash);
      const unshieldedTx = blockResponse.data?.block?.transactions?.find(
        (tx: Transaction) => tx.hash === scenario.transactionResult.txHash,
      );
      expect(unshieldedTx).toBeDefined();

      const utxos = [
        ...(unshieldedTx?.unshieldedCreatedOutputs ?? []),
        ...(unshieldedTx?.unshieldedSpentOutputs ?? []),
      ];
      expect(utxos.length).toBeGreaterThan(0);
      expect(new Set(utxos.map((utxo: UnshieldedUtxo) => utxo.tokenType))).toEqual(
        new Set([scenario.token.tokenType]),
      );
    });

    /**
     * Once an unshielded transaction has been submitted to node and confirmed, the indexer should
     * report that transaction through a progress update event for the source address.
     *
     * @given a subscription to unshielded transaction events for the source address
     * @when an unshielded transaction is submitted to node
     * @then a progress update event is received
     * @and its highest transaction ID is greater than the one seen before the transaction
     */
    test(
      'should be reported by the indexer through a progress update event for the source address',
      { timeout: PROGRESS_TIMEOUT },
      async (ctx: TestContext) => {
        startTest(scenario, ctx, 'sourceProgressUpdate', [
          'Subscription',
          'Transaction',
          'Progress',
        ]);

        await expectProgressUpdate(
          scenario.wallet.source.events,
          scenario.wallet.source.historicalEvents,
          'source',
        );
      },
    );

    /**
     * Once an unshielded transaction has been submitted to node and confirmed, the indexer should
     * report that transaction through a progress update event for the destination address.
     *
     * @given a subscription to unshielded transaction events for the destination address
     * @when an unshielded transaction is submitted to node
     * @then a progress update event is received
     * @and its highest transaction ID is greater than the one seen before the transaction
     */
    test(
      'should be reported by the indexer through a progress update event for the destination address',
      { timeout: PROGRESS_TIMEOUT },
      async (ctx: TestContext) => {
        startTest(scenario, ctx, 'destinationProgressUpdate', [
          'Subscription',
          'Transaction',
          'Progress',
        ]);

        await expectProgressUpdate(
          scenario.wallet.destinations[0].events,
          scenario.wallet.destinations[0].historicalDestinationEvents,
          'destination',
        );
      },
    );
  });
}
