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
// These are skeletons enumerating the intended cases:
//   test.todo → needs bridge event/claim data in the test environment. When
//               implemented, follow the framework conventions
//               (how-to-write-a-qa-indexer-test): issue the query via an
//               IndexerHttpClient method, define response types in
//               utils/indexer/indexer-types.ts, gate on bridge availability,
//               assert with toBeSuccess(), and set ctx.task labels.
//   test.skip → blocked on a specific in-flight feature (noted inline).
//
// Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/941

describe('bridge queries', () => {
  describe('bridgeEvents', () => {
    /**
     * @given a 32-byte all-zeros recipient address with no bridge activity
     * @when we query bridgeEvents with that recipient
     * @then the response is successful and bridgeEvents is an empty array
     */
    test.todo('should return an empty list for a recipient address with no bridge activity');

    /**
     * @given the with-data chain contains indexed UserTransfer events
     * @when we query bridgeEvents(variant: USER_TRANSFER)
     * @then every event in the response has __typename BridgeUserTransfer
     */
    test.todo('should return only UserTransfer events when filtered by variant USER_TRANSFER');

    /**
     * @given the with-data chain contains UserTransfer events for a known recipient
     * @when we query bridgeEvents(recipient: <knownAddress>)
     * @then every returned event has recipient equal to <knownAddress>
     */
    test.todo('should return only events for the specified recipient address');

    /**
     * @given the with-data chain contains all 5 bridge pallet event variants
     * @when we query bridgeEvents with no filters
     * @then the response contains events of each __typename
     */
    test.todo('should return events of all 5 variant types when present');

    /**
     * @given the with-data chain contains at least 3 indexed bridge events
     * @when we query bridgeEvents(limit: 2, offset: 0) and (limit: 2, offset: 1)
     * @then the two pages overlap by exactly 1 event and ids are ascending
     */
    test.todo('should paginate results with offset and limit');

    /**
     * @given the with-data chain contains at least one UserTransfer event
     * @when we query bridgeEvents(variant: USER_TRANSFER, limit: 1)
     * @then the event exposes id, blockHeight, midnightTxHash, cardanoTxHash, amount, recipient
     * @and all hex-encoded fields are non-empty and blockHeight is a positive integer
     */
    test.todo('should return events with the correct shape for BridgeUserTransfer');
  });

  describe('claims via unshieldedTransactions', () => {
    // A claim is surfaced as a BridgeClaimTransaction (a ClaimRewards transaction
    // with ClaimKind CardanoBridge) via the existing unshieldedTransactions query,
    // not a bridge-specific query — hence it is covered here rather than as a
    // bridgeClaims query.
    /**
     * @given the with-data chain contains a CardanoBridge claim for a known recipient
     * @when we subscribe to unshieldedTransactions for that recipient
     * @then the matched transaction is a BridgeClaimTransaction with the bridged recipient and amount
     */
    test.todo(
      'should surface a BridgeClaimTransaction (recipient, amount) via unshieldedTransactions for a claim recipient',
    );
  });

  describe('bridgeBalance', () => {
    /**
     * @given a 32-byte all-zeros address with no bridge activity
     * @when we query bridgeBalance(address: <zeros>)
     * @then deposited, claimed and balance are all the zero-value hex string
     */
    test.todo('should return zero balance for an address with no bridge activity');

    /**
     * @given the with-data chain contains UserTransfer events for a known recipient
     * @when we query bridgeBalance(address: <knownAddress>)
     * @then deposited equals the sum of UserTransfer.amount values for that address
     */
    test.todo('should set deposited to the sum of UserTransfer amounts for the address');

    /**
     * @given an address with a UserTransfer and a subsequent fully-claimed claim
     * @when we query bridgeBalance(address: <address>)
     * @then balance is the zero-value hex string (read from the ledger's remaining-claimable map)
     */
    test.todo('should return zero balance once an address has fully claimed');

    /**
     * @given an address with a UserTransfer and a subsequent partial claim
     * @when we query bridgeBalance(address: <address>)
     * @then balance is a non-zero hex string reflecting the remaining-claimable amount
     */
    test.todo('should reflect a non-zero remaining-claimable balance after a partial claim');
  });

  describe('bridgeDeposits', () => {
    /**
     * @given a 32-byte all-zeros address with no deposits
     * @when we query bridgeDeposits(recipient: <zeros>)
     * @then the response is an empty array
     */
    test.todo('should return an empty list for an address with no deposits');

    /**
     * @given the with-data chain has both UserTransfer and UnapprovedTransfer for the same address
     * @when we query bridgeDeposits(recipient: <address>) without includeUnapproved
     * @then only BridgeUserTransfer events are returned (no BridgeUnapprovedTransfer)
     */
    test.todo('should return only UserTransfer events by default (excludes UnapprovedTransfer)');

    // Blocked: UnapprovedTransfer is emitted only after the approval governance
    // logic lands on the node. The variant is defined but unreachable until the
    // ApprovedTransactions storage and governance extrinsic land.
    // Re-enable once UnapprovedTransfer events can be produced in a test env.
    // Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/940
    test.skip('should include UnapprovedTransfer events when includeUnapproved=true', () => {});
  });
});
