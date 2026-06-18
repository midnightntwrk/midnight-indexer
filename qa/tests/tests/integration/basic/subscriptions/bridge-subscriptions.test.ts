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

// Integration tests for c2m-bridge GraphQL subscriptions: bridgeEvents and
// bridgeBalance (#942).
//
// Subscription mechanics: subscriptions backfill-then-live-tail — they replay
// matching historical events from the optional `from` cursor (an event id) and
// then continue streaming new ones. There is no progress sentinel and no
// toBlock bound. bridgeBalance(address) emits the current balance immediately on
// connect, then re-emits on every relevant event for that address. There is no
// bridgeClaims subscription — a claim is a BridgeClaimTransaction surfaced via
// the unshieldedTransactions query.
//
// These are skeletons enumerating the intended cases:
//   test.todo → needs bridge schema + event data in the test environment. When
//               implemented, follow the framework conventions
//               (how-to-write-a-qa-indexer-test): use IndexerWsClient with the
//               beforeEach/afterEach connect lifecycle, define response types in
//               utils/indexer/websocket-client.ts, gate on bridge availability,
//               and set ctx.task labels.
//   test.skip → blocked on a specific in-flight feature (noted inline).
//
// Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/942

describe('bridge subscriptions', () => {
  describe('bridgeEvents', () => {
    /**
     * @given the with-data chain has bridge events with known ids
     * @when we subscribe with from = <firstEventId - 1>
     * @then the known events are delivered in id-ascending order before live mode
     */
    test.todo('should replay historical events from the given cursor id');

    /**
     * @given the with-data chain has events for multiple recipients
     * @when we subscribe with recipient = <knownAddress>
     * @then only events with the matching recipient are delivered
     */
    test.todo('should only deliver events matching the recipient filter');

    /**
     * @given the with-data chain has multiple event variants
     * @when we subscribe with variant = USER_TRANSFER
     * @then only BridgeUserTransfer events are delivered
     */
    test.todo('should only deliver events matching the variant filter');

    /**
     * @given we know the id of the last event received in a previous subscription
     * @when we reconnect with from = <lastId>
     * @then we receive events with id > lastId with no gap or duplication
     */
    test.todo('should resume from cursor without gap or duplication on reconnection');

    // Blocked: UnapprovedTransfer is unreachable until the approval governance
    // logic lands on the node (ApprovedTransactions storage + governance
    // extrinsic). Re-enable once UnapprovedTransfer events can be produced.
    // Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/940
    test.skip('should deliver BridgeUnapprovedTransfer events via subscription', () => {});
  });

  describe('claims via unshieldedTransactions', () => {
    // There is no bridgeClaims subscription. A bridge claim is a
    // BridgeClaimTransaction (a ClaimRewards transaction with ClaimKind
    // CardanoBridge) surfaced via the unshieldedTransactions query, so claim
    // coverage belongs with the unshielded-transaction tests.
    /**
     * @given the with-data chain contains a CardanoBridge claim for a known recipient
     * @when we observe unshieldedTransactions for that recipient
     * @then the claim appears as a BridgeClaimTransaction
     */
    test.todo(
      'should observe claims as BridgeClaimTransaction via the unshieldedTransactions query',
    );
  });

  describe('bridgeBalance', () => {
    /**
     * @given a 32-byte all-zeros address with no bridge activity
     * @when we subscribe to bridgeBalance(address: <zeros>)
     * @then the first frame has deposited, claimed and balance all zero
     */
    test.todo('should emit zero balance immediately for an address with no bridge activity');

    /**
     * @given we are subscribed to bridgeBalance(address: <knownAddress>)
     * @when a UserTransfer for that address is indexed
     * @then the subscription emits a new BridgeBalance with deposited > 0
     */
    test.todo('should push an updated BridgeBalance when a relevant UserTransfer is indexed');

    /**
     * @given the address has a prior UserTransfer balance and we are subscribed
     * @when a CardanoBridge claim for that address is indexed
     * @then balance reflects the ledger's net remaining-claimable, reaching zero once fully claimed
     */
    test.todo('should push an updated BridgeBalance when a claim reduces the balance');
  });
});
