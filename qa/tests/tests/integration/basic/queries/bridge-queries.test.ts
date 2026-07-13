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

// Integration tests for c2m-bridge GraphQL queries: bridgeEvents, bridgeBalance,
// bridgeDeposits, and claims surfaced as BridgeClaimTransaction (#941).
//
// Test-data reality (2026-07): the bridge is a cross-chain feature — events
// originate from Cardano locks, so they cannot be produced by the indexer's own
// toolkit-driven node-data generation. Of the five BridgeEvent variants only
// BridgeUserTransfer has any on-chain data, and only on devnet. Cases that need
// only the surface (empty results, zero balances) run wherever the surface is
// deployed; cases that need a real UserTransfer run where such data exists and
// ctx.skip otherwise; cases that need the other four variants or non-trivial
// pool state stay test.todo until a data source exists.
//   test.todo → needs bridge data not yet producible in any test environment.
//   test.skip → blocked on a specific in-flight feature (noted inline).
//
// Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/941

import log from '@utils/logging/logger';
import { env } from 'environment/model';
import type { TestContext } from 'vitest';
import '@utils/logging/test-logging-hooks';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import type { BridgeUserTransfer } from '@utils/indexer/indexer-types';

const httpClient = new IndexerHttpClient();

// 32-byte all-zeros recipient (HexEncoded) — a valid address with no bridge activity.
const EMPTY_RECIPIENT = '0'.repeat(64);
// 16-byte u128 zero value, as returned by the bridge amount/balance fields.
const ZERO_U128 = '0'.repeat(32);

// Probed once against the target environment: whether the bridge surface exists,
// a real UserTransfer to assert shape against, and a fully-claimed address (a
// recipient whose deposited > 0 but remaining balance is zero).
let surfacePresent = false;
let sampleUserTransfer: BridgeUserTransfer | null = null;
let fullyClaimedAddress: string | null = null;

// Bridge events cannot be produced on the undeployed environment (no Cardano
// side), so the whole surface is skipped there.
describe.skipIf(env.isUndeployedEnv())('bridge queries', () => {
  beforeAll(async () => {
    const probe = await httpClient.getBridgeEvents({ variant: 'USER_TRANSFER', limit: 1 });
    // A GraphQL error here means the deployed indexer predates the bridge surface
    // (pre-4.4). Leave surfacePresent false so every case skips rather than fails.
    if (probe.errors || !probe.data) {
      log.warn(`Bridge surface not present on ${env.getCurrentEnvironmentName()}; skipping`);
      return;
    }
    surfacePresent = true;

    const first = probe.data.bridgeEvents.find(
      (e): e is BridgeUserTransfer => e.__typename === 'BridgeUserTransfer',
    );
    sampleUserTransfer = first ?? null;

    if (sampleUserTransfer) {
      const balance = await httpClient.getBridgeBalance(sampleUserTransfer.recipient);
      const b = balance.data?.bridgeBalance;
      if (b && b.deposited !== ZERO_U128 && b.balance === ZERO_U128) {
        fullyClaimedAddress = sampleUserTransfer.recipient;
      }
    }
  }, 30_000);

  describe('bridgeEvents', () => {
    /**
     * @given a 32-byte all-zeros recipient address with no bridge activity
     * @when bridgeEvents is queried for that recipient
     * @then the response is successful and bridgeEvents is an empty array
     */
    test('should return an empty list for a recipient address with no bridge activity', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Bridge', 'Negative'] };
      if (!surfacePresent) return ctx.skip();

      const response = await httpClient.getBridgeEvents({ recipient: EMPTY_RECIPIENT });

      expect(response).toBeSuccess();
      expect(response.data?.bridgeEvents).toEqual([]);
    });

    /**
     * @given an environment with at least one indexed BridgeUserTransfer
     * @when bridgeEvents(variant: USER_TRANSFER, limit: 1) is queried
     * @then the event exposes id, blockHeight, midnightTxHash, cardanoTxHash, amount, recipient
     * @and the hex fields are non-empty and blockHeight is a positive integer
     */
    test('should return events with the correct shape for BridgeUserTransfer', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Bridge', 'UserTransfer'] };
      if (!surfacePresent) return ctx.skip();
      if (!sampleUserTransfer) return ctx.skip(true, 'no BridgeUserTransfer data on this env');

      const event = sampleUserTransfer;
      expect(event.__typename).toBe('BridgeUserTransfer');
      expect(Number.isInteger(event.id)).toBe(true);
      expect(event.blockHeight).toBeGreaterThan(0);
      expect(event.midnightTxHash).toMatch(/^[0-9a-f]+$/);
      expect(event.cardanoTxHash).toMatch(/^[0-9a-f]+$/);
      expect(event.amount).toMatch(/^[0-9a-f]+$/);
      expect(event.recipient).toMatch(/^[0-9a-f]+$/);
    });

    /**
     * @given an environment whose chain has UserTransfer events for a known recipient
     * @when bridgeEvents(recipient: <knownAddress>) is queried
     * @then every returned event has recipient equal to <knownAddress>
     */
    test.todo('should return only events for the specified recipient address');

    /**
     * @given a chain containing all 5 bridge pallet event variants
     * @when bridgeEvents is queried with no filters
     * @then the response contains events of each __typename
     */
    test.todo('should return events of all 5 variant types when present');

    /**
     * @given a chain with at least 3 indexed bridge events
     * @when bridgeEvents(limit: 2, offset: 0) and (limit: 2, offset: 1) are queried
     * @then the two pages overlap by exactly 1 event and ids are ascending
     */
    test.todo('should paginate results with offset and limit');

    /**
     * @given a chain containing indexed UserTransfer events
     * @when bridgeEvents(variant: USER_TRANSFER) is queried
     * @then every event has __typename BridgeUserTransfer
     */
    test.todo('should return only UserTransfer events when filtered by variant USER_TRANSFER');
  });

  describe('claims via unshieldedTransactions', () => {
    // A claim is surfaced as a BridgeClaimTransaction (a ClaimRewards transaction
    // with ClaimKind CardanoBridge) via the existing unshieldedTransactions query,
    // not a bridge-specific query.
    /**
     * @given a chain containing a CardanoBridge claim for a known recipient
     * @when unshieldedTransactions is queried for that recipient
     * @then the matched transaction is a BridgeClaimTransaction with the bridged recipient and amount
     */
    test.todo(
      'should surface a BridgeClaimTransaction (recipient, amount) via unshieldedTransactions for a claim recipient',
    );
  });

  describe('bridgeBalance', () => {
    /**
     * @given a 32-byte all-zeros address with no bridge activity
     * @when bridgeBalance(address: <zeros>) is queried
     * @then deposited, claimed and balance are all the zero-value hex string
     */
    test('should return zero balance for an address with no bridge activity', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Bridge', 'Balance', 'Negative'] };
      if (!surfacePresent) return ctx.skip();

      const response = await httpClient.getBridgeBalance(EMPTY_RECIPIENT);

      expect(response).toBeSuccess();
      expect(response.data?.bridgeBalance).toEqual({
        deposited: ZERO_U128,
        claimed: ZERO_U128,
        balance: ZERO_U128,
      });
    });

    /**
     * @given an address with a UserTransfer deposit that has been fully claimed
     * @when bridgeBalance(address: <address>) is queried
     * @then deposited and claimed are non-zero and balance is the zero-value hex string
     *
     * The remaining-claimable balance is read from the ledger's bridge_receiving map.
     */
    test('should return zero balance once an address has fully claimed', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Bridge', 'Balance'] };
      if (!surfacePresent) return ctx.skip();
      if (!fullyClaimedAddress)
        return ctx.skip(true, 'no fully-claimed bridge address on this env');

      const response = await httpClient.getBridgeBalance(fullyClaimedAddress);

      expect(response).toBeSuccess();
      const b = response.data!.bridgeBalance;
      expect(b.deposited).not.toBe(ZERO_U128);
      expect(b.claimed).not.toBe(ZERO_U128);
      expect(b.balance).toBe(ZERO_U128);
    });

    /**
     * @given an address with UserTransfer events
     * @when bridgeBalance(address: <knownAddress>) is queried
     * @then deposited equals the sum of UserTransfer.amount values for that address
     */
    test.todo('should set deposited to the sum of UserTransfer amounts for the address');

    /**
     * @given an address with a UserTransfer and a subsequent partial claim
     * @when bridgeBalance(address: <address>) is queried
     * @then balance is a non-zero hex string reflecting the remaining-claimable amount
     */
    test.todo('should reflect a non-zero remaining-claimable balance after a partial claim');
  });

  describe('bridgeDeposits', () => {
    /**
     * @given a 32-byte all-zeros address with no deposits
     * @when bridgeDeposits(recipient: <zeros>) is queried
     * @then the response is an empty array
     */
    test('should return an empty list for an address with no deposits', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Bridge', 'Deposits', 'Negative'] };
      if (!surfacePresent) return ctx.skip();

      const response = await httpClient.getBridgeDeposits(EMPTY_RECIPIENT);

      expect(response).toBeSuccess();
      expect(response.data?.bridgeDeposits).toEqual([]);
    });

    /**
     * @given a chain with both UserTransfer and UnapprovedTransfer for the same address
     * @when bridgeDeposits(recipient: <address>) is queried without includeUnapproved
     * @then only BridgeUserTransfer events are returned (no BridgeUnapprovedTransfer)
     */
    test.todo('should return only UserTransfer events by default (excludes UnapprovedTransfer)');

    // Blocked: UnapprovedTransfer is emitted only after the approval governance
    // logic lands on the node. The variant is defined but unreachable until the
    // ApprovedTransactions storage and governance extrinsic land.
    // Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/940
    test.skip('should include UnapprovedTransfer events when includeUnapproved=true', () => {});
  });
});
