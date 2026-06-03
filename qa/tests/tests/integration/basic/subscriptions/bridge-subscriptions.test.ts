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

// Integration tests for c2m-bridge GraphQL subscriptions.
//
// Covers: bridgeEvents, bridgeClaims, bridgeBalance subscriptions (#942).
//
// ── Status of tests ──────────────────────────────────────────────────────────
//
//   it.todo   → Requires bridge schema + event data in the test environment.
//               See the "bridge data availability" section in PR #1219 (the
//               #941 test plan PR) for the investigation required before these
//               can be activated.
//               Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/942
//
//   it.skip   → Blocked on a specific in-flight feature (noted inline).
//
// ── Subscription mechanics ───────────────────────────────────────────────────
//
// All bridge subscriptions follow the existing indexer pattern:
//   1. Connect via WebSocket (graphql-ws protocol).
//   2. Optionally provide a `from` cursor (event id or block height) for
//      reconnection / historical backfill.
//   3. Optionally provide `toBlock` to bound the stream (replay mode).
//   4. Receive typed events until the subscription ends or the client disconnects.
//
// The `bridgeBalance(address)` subscription differs: it computes and emits the
// current BridgeBalance immediately, then re-emits on every event for that address.
//
// Reconnection and backfill tests require known event ids from a stable fixture
// chain, so they are also blocked on Q1 (data availability).
//
// ── Types ─────────────────────────────────────────────────────────────────────
//
// Types are defined inline as stubs. Move to indexer-types.ts and
// websocket-client.ts once #942 is merged and field names are confirmed.

import log from '@utils/logging/logger';
import '@utils/logging/test-logging-hooks';
import { IndexerWsClient } from '@utils/indexer/websocket-client';
import type { GraphQLResponse } from '@utils/indexer/indexer-types';

// ── Stub types (move to indexer-types.ts / websocket-client.ts once #942 lands) ──

interface BlockReference {
  blockHeight: number;
  blockHash: string;
}

interface BridgeEventBase {
  id: number;
  midnightTxHash: string;
  indexedAt: BlockReference;
}

interface BridgeUserTransfer extends BridgeEventBase {
  __typename: 'BridgeUserTransfer';
  cardanoTxHash: string;
  amount: string;
  recipient: string;
}

interface BridgeReserveTransfer extends BridgeEventBase {
  __typename: 'BridgeReserveTransfer';
  cardanoTxHash: string;
  amount: string;
}

interface BridgeInvalidTransfer extends BridgeEventBase {
  __typename: 'BridgeInvalidTransfer';
  cardanoTxHash: string;
  amount: string;
}

interface BridgeUnapprovedTransfer extends BridgeEventBase {
  __typename: 'BridgeUnapprovedTransfer';
  cardanoTxHash: string;
  amount: string;
  recipient: string;
}

interface BridgeSubminimalFlushTransfer extends BridgeEventBase {
  __typename: 'BridgeSubminimalFlushTransfer';
  amount: string;
  count: number;
}

type BridgeEvent =
  | BridgeUserTransfer
  | BridgeReserveTransfer
  | BridgeInvalidTransfer
  | BridgeUnapprovedTransfer
  | BridgeSubminimalFlushTransfer;

interface BridgeProgress {
  __typename: 'BridgeProgress';
  highestBlockHeight: number;
  finished: boolean;
}

type BridgeEventOrProgress = BridgeEvent | BridgeProgress;

interface BridgeClaim {
  id: number;
  transactionHash: string;
  recipient: string;
  amount: string;
  indexedAt: BlockReference;
}

type BridgeClaimOrProgress = BridgeClaim | BridgeProgress;

interface BridgeBalance {
  address: string;
  deposited: string;
  claimed: string;
  balance: string;
}

// ── GraphQL subscription strings ─────────────────────────────────────────────

const BRIDGE_EVENTS_SUBSCRIPTION = `
  subscription BridgeEvents(
    $from: Int
    $toBlock: Int
    $recipient: HexEncoded
    $variant: BridgePalletEventVariant
  ) {
    bridgeEvents(from: $from, toBlock: $toBlock, recipient: $recipient, variant: $variant) {
      ... on BridgeUserTransfer {
        __typename id midnightTxHash cardanoTxHash amount recipient
        indexedAt { blockHeight blockHash }
      }
      ... on BridgeReserveTransfer {
        __typename id midnightTxHash cardanoTxHash amount
        indexedAt { blockHeight blockHash }
      }
      ... on BridgeInvalidTransfer {
        __typename id midnightTxHash cardanoTxHash amount
        indexedAt { blockHeight blockHash }
      }
      ... on BridgeUnapprovedTransfer {
        __typename id midnightTxHash cardanoTxHash amount recipient
        indexedAt { blockHeight blockHash }
      }
      ... on BridgeSubminimalFlushTransfer {
        __typename id midnightTxHash amount count
        indexedAt { blockHeight blockHash }
      }
      ... on BridgeProgress {
        __typename highestBlockHeight finished
      }
    }
  }
`;

const BRIDGE_CLAIMS_SUBSCRIPTION = `
  subscription BridgeClaims($from: Int, $toBlock: Int, $recipient: HexEncoded) {
    bridgeClaims(from: $from, toBlock: $toBlock, recipient: $recipient) {
      ... on BridgeClaim {
        __typename id transactionHash recipient amount
        indexedAt { blockHeight blockHash }
      }
      ... on BridgeProgress {
        __typename highestBlockHeight finished
      }
    }
  }
`;

const BRIDGE_BALANCE_SUBSCRIPTION = `
  subscription BridgeBalance($address: HexEncoded!) {
    bridgeBalance(address: $address) {
      address
      deposited
      claimed
      balance
    }
  }
`;

// A 32-byte all-zeros hex string with deterministically no bridge activity.
const UNKNOWN_ADDRESS = '0'.repeat(64);

// ── Tests ────────────────────────────────────────────────────────────────────

describe('bridge subscriptions — bridgeEvents', () => {
  let wsClient: IndexerWsClient;

  beforeEach(async () => {
    wsClient = new IndexerWsClient();
    await wsClient.connectionInit();
  }, 30_000);

  afterEach(async () => {
    await wsClient.connectionClose();
  });

  /**
   * bridgeEvents subscription with no cursor and no data emits a BridgeProgress
   * immediately indicating the current state (finished=false, live mode).
   *
   * This test does not require pre-existing bridge events — it only verifies
   * that the subscription endpoint is live and emits a progress frame.
   *
   * @given no from cursor (subscribe from current head)
   * @when we subscribe to bridgeEvents
   * @then we receive at least one BridgeProgress frame with highestBlockHeight >= 0
   */
  it.todo('should emit a BridgeProgress frame immediately when no from cursor is given');

  /**
   * bridgeEvents subscription replays historical events when a from cursor is given.
   *
   * @given the with-data chain has bridge events with known ids
   * @when we subscribe with from = <firstEventId - 1>
   * @then we receive the known events in id-ascending order before switching to live mode
   */
  it.todo('should replay historical events from the given cursor id');

  /**
   * bridgeEvents subscription with toBlock emits a final BridgeProgress(finished=true)
   * when the bounded replay reaches the target block.
   *
   * @given the with-data chain has bridge events up to a known block height H
   * @when we subscribe with from=0 and toBlock=H
   * @then we receive all events up to block H and then a BridgeProgress with finished=true
   */
  it.todo('should finish replay at toBlock and emit BridgeProgress(finished=true)');

  /**
   * bridgeEvents subscription with a recipient filter only streams events for that address.
   *
   * @given the with-data chain has events for multiple recipients
   * @when we subscribe with recipient=<knownAddress>
   * @then only events with matching recipient are delivered
   */
  it.todo('should only deliver events matching the recipient filter');

  /**
   * bridgeEvents subscription with a variant filter only streams that event variant.
   *
   * @given the with-data chain has multiple event variants
   * @when we subscribe with variant=USER_TRANSFER
   * @then only BridgeUserTransfer events are delivered
   */
  it.todo('should only deliver events matching the variant filter');

  /**
   * bridgeEvents subscription reconnection: subscribing again from a known cursor
   * picks up exactly where the previous subscription left off with no gap or duplicate.
   *
   * @given we know the id of the last event received in a previous subscription
   * @when we reconnect with from=<lastId>
   * @then we receive events starting at id > lastId with no duplicates
   */
  it.todo('should resume from cursor without gap or duplication on reconnection');

  // Skipped: UnapprovedTransfer is unreachable until approval governance logic lands.
  // The variant is defined in the union but never emitted until `ApprovedTransactions`
  // storage and the governance extrinsic land on `c-to-m-subminimal-transfers-accumulation`.
  // Re-enable once UnapprovedTransfer events appear in a test environment.
  // Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/940
  test.skip('should deliver BridgeUnapprovedTransfer events via subscription', () => {
    // TODO: implement once UnapprovedTransfer can be generated in test env.
    // Subscribe to bridgeEvents (no variant filter or variant=UNAPPROVED_TRANSFER).
    // Trigger an unapproved transfer and verify the event arrives.
  });
});

describe('bridge subscriptions — bridgeClaims', () => {
  let wsClient: IndexerWsClient;

  beforeEach(async () => {
    wsClient = new IndexerWsClient();
    await wsClient.connectionInit();
  }, 30_000);

  afterEach(async () => {
    await wsClient.connectionClose();
  });

  /**
   * bridgeClaims subscription replays historical claims from the given cursor.
   *
   * @given the with-data chain has at least one CardanoBridge claim with a known id
   * @when we subscribe with from=<claimId - 1>
   * @then we receive the known claim as a BridgeClaim frame
   */
  it.todo('should replay a historical claim from the given cursor');

  /**
   * bridgeClaims subscription with a recipient filter delivers only claims for that address.
   *
   * @given the with-data chain has claims for multiple addresses
   * @when we subscribe with recipient=<knownAddress>
   * @then only claims where recipient matches are delivered
   */
  it.todo('should filter bridgeClaims by recipient');

  /**
   * bridgeClaims subscription with toBlock finishes and emits BridgeProgress(finished=true).
   *
   * @given the with-data chain has claims up to a known block
   * @when we subscribe with from=0 and toBlock=<knownBlock>
   * @then all claims up to that block are delivered and a Progress(finished=true) frame follows
   */
  it.todo('should finish replay at toBlock for bridgeClaims');
});

describe('bridge subscriptions — bridgeBalance', () => {
  let wsClient: IndexerWsClient;

  beforeEach(async () => {
    wsClient = new IndexerWsClient();
    await wsClient.connectionInit();
  }, 30_000);

  afterEach(async () => {
    await wsClient.connectionClose();
  });

  /**
   * bridgeBalance subscription emits the current balance immediately on connect.
   * For an address with no bridge activity, the initial emission is zero balance.
   *
   * @given a 32-byte all-zeros address with no bridge activity
   * @when we subscribe to bridgeBalance(address: <zeros>)
   * @then the first received frame has deposited=0, claimed=0, balance=0
   */
  it.todo('should emit zero balance immediately for an address with no bridge activity');

  /**
   * bridgeBalance subscription re-emits an updated balance when a new UserTransfer
   * for the subscribed address is indexed.
   *
   * @given we subscribe to bridgeBalance(address: <knownAddress>)
   * @and a UserTransfer for that address is then indexed
   * @when the chain-indexer processes the block
   * @then the subscription emits a new BridgeBalance with deposited > 0
   */
  it.todo('should push an updated BridgeBalance when a relevant UserTransfer is indexed');

  /**
   * bridgeBalance subscription re-emits when a claim for the subscribed address is indexed.
   *
   * @given the address has a prior UserTransfer balance
   * @and a CardanoBridge claim for that address is indexed
   * @when the subscription receives the claim event
   * @then balance = deposited - claimedAmount
   */
  it.todo('should push an updated BridgeBalance when a BridgeClaim reduces the balance');
});

// Suppress unused import warnings until test bodies are implemented.
void BRIDGE_EVENTS_SUBSCRIPTION;
void BRIDGE_CLAIMS_SUBSCRIPTION;
void BRIDGE_BALANCE_SUBSCRIPTION;
void UNKNOWN_ADDRESS;
void log;

type _SuppressUnused =
  | BridgeEventOrProgress
  | BridgeClaimOrProgress
  | BridgeBalance
  | GraphQLResponse<unknown>;
void (undefined as unknown as _SuppressUnused);
