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

// Integration tests for c2m-bridge GraphQL queries.
//
// Covers: bridgeEvents, bridgeClaims, bridgeBalance, bridgeDeposits (#941).
//
// ── Status of tests ──────────────────────────────────────────────────────────
//
//   it.todo   → Requires bridge event data to be present in the test
//               environment. Current status: UNKNOWN — see Q1 in the test
//               plan. Investigation needed: does the with-data chain contain
//               any C2M bridge pallet events? If not, these tests must stay
//               as todo until (a) node branch `c-to-m-subminimal-transfers-
//               accumulation` merges, (b) the toolkit gains bridge tx support,
//               and (c) the with-data chain is refreshed.
//               Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/941
//
//   it.skip   → Blocked on a specific in-flight feature (noted inline).
//               These are intentionally skipped; do not remove the skip
//               without confirming the upstream blocker has landed.
//
// ── Types ─────────────────────────────────────────────────────────────────────
//
// Bridge GraphQL types are defined inline below as stubs. Move them to
// utils/indexer/indexer-types.ts once #941 is merged and the exact field
// names are confirmed against the live schema.

import log from '@utils/logging/logger';
import '@utils/logging/test-logging-hooks';
import type { TestContext } from 'vitest';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import type { GraphQLResponse } from '@utils/indexer/indexer-types';

// ── Stub types (move to indexer-types.ts once #941 lands) ──────────────────

export type BridgePalletEventVariant =
  | 'USER_TRANSFER'
  | 'RESERVE_TRANSFER'
  | 'INVALID_TRANSFER'
  | 'UNAPPROVED_TRANSFER'
  | 'SUBMINIMAL_FLUSH_TRANSFER';

export interface BlockReference {
  blockHeight: number;
  blockHash: string;
}

export interface BridgeEventBase {
  id: number;
  midnightTxHash: string;
  indexedAt: BlockReference;
}

export interface BridgeUserTransfer extends BridgeEventBase {
  __typename: 'BridgeUserTransfer';
  cardanoTxHash: string;
  amount: string;
  recipient: string;
}

export interface BridgeReserveTransfer extends BridgeEventBase {
  __typename: 'BridgeReserveTransfer';
  cardanoTxHash: string;
  amount: string;
}

export interface BridgeInvalidTransfer extends BridgeEventBase {
  __typename: 'BridgeInvalidTransfer';
  cardanoTxHash: string;
  amount: string;
}

export interface BridgeUnapprovedTransfer extends BridgeEventBase {
  __typename: 'BridgeUnapprovedTransfer';
  cardanoTxHash: string;
  amount: string;
  recipient: string;
}

export interface BridgeSubminimalFlushTransfer extends BridgeEventBase {
  __typename: 'BridgeSubminimalFlushTransfer';
  amount: string;
  count: number;
}

export type BridgeEvent =
  | BridgeUserTransfer
  | BridgeReserveTransfer
  | BridgeInvalidTransfer
  | BridgeUnapprovedTransfer
  | BridgeSubminimalFlushTransfer;

export interface BridgeClaim {
  id: number;
  transactionHash: string;
  recipient: string;
  amount: string;
  indexedAt: BlockReference;
}

export interface BridgeBalance {
  address: string;
  deposited: string;
  claimed: string;
  balance: string;
}

// ── GraphQL query strings ───────────────────────────────────────────────────

const BRIDGE_EVENT_FRAGMENT = `
  fragment BridgeEventFields on BridgeEvent {
    id
    midnightTxHash
    indexedAt { blockHeight blockHash }
    ... on BridgeUserTransfer {
      __typename cardanoTxHash amount recipient
    }
    ... on BridgeReserveTransfer {
      __typename cardanoTxHash amount
    }
    ... on BridgeInvalidTransfer {
      __typename cardanoTxHash amount
    }
    ... on BridgeUnapprovedTransfer {
      __typename cardanoTxHash amount recipient
    }
    ... on BridgeSubminimalFlushTransfer {
      __typename amount count
    }
  }
`;

const BRIDGE_EVENTS_QUERY = `
  ${BRIDGE_EVENT_FRAGMENT}
  query BridgeEvents(
    $blockHeight: Int
    $transactionId: Int
    $recipient: HexEncoded
    $variant: BridgePalletEventVariant
    $offset: Int
    $limit: Int
  ) {
    bridgeEvents(
      blockHeight: $blockHeight
      transactionId: $transactionId
      recipient: $recipient
      variant: $variant
      offset: $offset
      limit: $limit
    ) {
      ...BridgeEventFields
    }
  }
`;

const BRIDGE_CLAIMS_QUERY = `
  query BridgeClaims($recipient: HexEncoded, $offset: Int, $limit: Int) {
    bridgeClaims(recipient: $recipient, offset: $offset, limit: $limit) {
      id
      transactionHash
      recipient
      amount
      indexedAt { blockHeight blockHash }
    }
  }
`;

const BRIDGE_BALANCE_QUERY = `
  query BridgeBalance($address: HexEncoded!) {
    bridgeBalance(address: $address) {
      address
      deposited
      claimed
      balance
    }
  }
`;

const BRIDGE_DEPOSITS_QUERY = `
  ${BRIDGE_EVENT_FRAGMENT}
  query BridgeDeposits($recipient: HexEncoded!, $includeUnapproved: Boolean) {
    bridgeDeposits(recipient: $recipient, includeUnapproved: $includeUnapproved) {
      ...BridgeEventFields
    }
  }
`;

// ── Helpers ─────────────────────────────────────────────────────────────────

const httpClient = new IndexerHttpClient();

function rawRequest<T>(
  query: string,
  variables?: Record<string, unknown>,
): Promise<GraphQLResponse<T>> {
  return (
    httpClient as unknown as {
      client: {
        rawRequest: (q: string, vars?: Record<string, unknown>) => Promise<GraphQLResponse<T>>;
      };
    }
  ).client.rawRequest(query, variables);
}

// A 32-byte all-zeros hex string: deterministically has no bridge activity.
const UNKNOWN_RECIPIENT = '0'.repeat(64);

// ── Tests ───────────────────────────────────────────────────────────────────

describe('bridge queries — bridgeEvents', () => {
  /**
   * bridgeEvents for an unknown recipient returns an empty list (not an error).
   * This test is deterministic and does not require bridge event data.
   *
   * @given a 32-byte all-zeros hex address that has no bridge activity
   * @when we query bridgeEvents with recipient=<zeros>
   * @then the response is successful and bridgeEvents is an empty array
   */
  it.todo('should return an empty list for a recipient address with no bridge activity');

  /**
   * bridgeEvents with a valid variant filter for USER_TRANSFER returns only UserTransfer events.
   *
   * @given the with-data chain contains indexed UserTransfer events
   * @when we query bridgeEvents(variant: USER_TRANSFER)
   * @then every event in the response has __typename BridgeUserTransfer
   */
  it.todo('should return only UserTransfer events when filtered by variant USER_TRANSFER');

  /**
   * bridgeEvents with a recipient filter returns only events matching that recipient.
   *
   * @given the with-data chain contains UserTransfer events for a known recipient address
   * @when we query bridgeEvents(recipient: <knownAddress>)
   * @then every returned event has recipient equal to <knownAddress>
   */
  it.todo('should return only events for the specified recipient address');

  /**
   * bridgeEvents without filters returns all 5 variant types if they are present.
   *
   * @given the with-data chain contains all 5 bridge pallet event variants
   * @when we query bridgeEvents with no filters
   * @then the response contains events of each __typename
   */
  it.todo('should return events of all 5 variant types when present');

  /**
   * bridgeEvents respects offset and limit pagination.
   *
   * @given the with-data chain contains at least 3 indexed bridge events
   * @when we query bridgeEvents(limit: 2, offset: 0) and bridgeEvents(limit: 2, offset: 1)
   * @then the two pages share exactly 1 event (offset overlap) and ids are in ascending order
   */
  it.todo('should paginate results with offset and limit');

  /**
   * bridgeEvents response shape matches the BridgeEvent interface and concrete type fields.
   * Validates all required fields are present and non-null where expected.
   *
   * @given the with-data chain contains at least one UserTransfer event
   * @when we query bridgeEvents(variant: USER_TRANSFER, limit: 1)
   * @then the single event has id, midnightTxHash, indexedAt, cardanoTxHash, amount, recipient
   * @and all hex-encoded fields are non-empty strings
   * @and indexedAt.blockHeight is a positive integer
   */
  it.todo('should return events with the correct shape for BridgeUserTransfer');
});

describe('bridge queries — bridgeClaims', () => {
  /**
   * bridgeClaims for an unknown recipient returns an empty list.
   *
   * @given a 32-byte all-zeros hex address with no claims
   * @when we query bridgeClaims(recipient: <zeros>)
   * @then the response is successful and bridgeClaims is an empty array
   */
  it.todo('should return an empty list for an address with no claims');

  /**
   * bridgeClaims for a known claimer returns their claim records.
   *
   * @given the with-data chain contains a CardanoBridge claim for a known address
   * @when we query bridgeClaims(recipient: <claimerAddress>)
   * @then each claim has id, transactionHash, recipient, amount, indexedAt
   * @and the recipient field matches the queried address
   */
  it.todo('should return claims for a known claimer address');

  /**
   * bridgeClaims respects offset and limit pagination.
   *
   * @given the with-data chain contains at least 3 bridge claims
   * @when we paginate with limit 2 and offset 0, then offset 1
   * @then results are consistent and ids are in ascending order
   */
  it.todo('should paginate claims with offset and limit');
});

describe('bridge queries — bridgeBalance', () => {
  /**
   * bridgeBalance for an address with no bridge activity returns a zero balance.
   * The zero value is HexEncoded big-endian u128, which is "00" * 16.
   *
   * @given a 32-byte all-zeros hex address with no bridge activity
   * @when we query bridgeBalance(address: <zeros>)
   * @then the response is successful
   * @and deposited, claimed, and balance are all zero-value hex strings
   */
  it.todo('should return zero balance for an address with no bridge activity');

  /**
   * bridgeBalance.deposited reflects the sum of UserTransfer amounts for the address.
   *
   * @given the with-data chain contains UserTransfer events for a known recipient
   * @when we query bridgeBalance(address: <knownAddress>)
   * @then deposited equals the sum of UserTransfer.amount values for that address
   */
  it.todo('should set deposited to the sum of UserTransfer amounts for the address');

  /**
   * bridgeBalance.balance = deposited - claimed, clamped to zero.
   *
   * @given the with-data chain has UserTransfer and a subsequent BridgeClaim for the same address
   * @when we query bridgeBalance(address: <address>)
   * @then balance = deposited - claimed
   * @and balance is never negative (clamped to zero)
   */
  it.todo('should return balance equal to deposited minus claimed');

  /**
   * bridgeBalance supports partial claims (claimed < deposited).
   *
   * @given an address with a UserTransfer of amount A and a BridgeClaim of amount B (B < A)
   * @when we query bridgeBalance
   * @then balance = A - B (partial claim leaves remaining claimable balance)
   */
  it.todo('should reflect partial claims correctly in balance');
});

describe('bridge queries — bridgeDeposits', () => {
  /**
   * bridgeDeposits for an unknown address returns an empty list by default.
   *
   * @given a 32-byte all-zeros hex address with no deposits
   * @when we query bridgeDeposits(recipient: <zeros>)
   * @then the response is an empty array
   */
  it.todo('should return an empty list for an address with no deposits');

  /**
   * bridgeDeposits without includeUnapproved flag returns only UserTransfer events.
   *
   * @given the with-data chain has both UserTransfer and UnapprovedTransfer for the same address
   * @when we query bridgeDeposits(recipient: <address>) without includeUnapproved
   * @then only BridgeUserTransfer events are returned (no BridgeUnapprovedTransfer)
   */
  it.todo('should return only UserTransfer events by default (excludes UnapprovedTransfer)');

  // Skipped: UnapprovedTransfer is emitted only after the approval governance logic lands
  // on node branch `c-to-m-subminimal-transfers-accumulation`. The `UnapprovedTransfer`
  // variant is defined but unreachable until `ApprovedTransactions` storage and the
  // governance extrinsic land (see #940 notes, commit 03bda8f4, 29 Apr 2026).
  // Re-enable this test once UnapprovedTransfer events appear in a test environment.
  // Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/940
  test.skip('should include UnapprovedTransfer events when includeUnapproved=true', async (_ctx: TestContext) => {
    // TODO: implement once UnapprovedTransfer can be generated in test env
    // Query bridgeDeposits(recipient: <address>, includeUnapproved: true)
    // Expect response to contain both BridgeUserTransfer and BridgeUnapprovedTransfer
  });
});

// Keep this at the bottom so unused imports are not flagged by the linter
// until the test bodies are fleshed out.
void BRIDGE_EVENTS_QUERY;
void BRIDGE_CLAIMS_QUERY;
void BRIDGE_BALANCE_QUERY;
void BRIDGE_DEPOSITS_QUERY;
void UNKNOWN_RECIPIENT;
void rawRequest;
void log;
