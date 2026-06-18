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

// Integration tests for the c2m-bridge pool observability subscription:
// bridgePoolUpdates (#944).
//
// bridgePoolUpdates takes no arguments. It emits the current BridgePoolSummary
// immediately on connect (with newEvent null), then live-tails a BridgePoolUpdate
// frame on every newly indexed pool-affecting event (Reserve or Treasury). Each
// frame carries the triggering event (newEvent) and the recomputed pool snapshot
// (pool). There is no cursor / historical-replay parameter.
//
// These are skeletons enumerating the intended cases:
//   test.todo → needs bridge schema + live event generation in the test
//               environment. When implemented, follow the framework conventions
//               (how-to-write-a-qa-indexer-test): use IndexerWsClient with the
//               beforeEach/afterEach connect lifecycle, define response types in
//               utils/indexer/websocket-client.ts, gate on bridge availability,
//               and set ctx.task labels.
//   test.skip → blocked on an in-flight feature: UNAPPROVED on approval
//               governance (#940); SUBMINIMAL_FLUSH on confirming the flush
//               threshold with the node team so a flush can be triggered.
//
// Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/944

describe('bridge pool subscription', () => {
  describe('bridgePoolUpdates', () => {
    /**
     * @given no pool-affecting bridge events have been indexed
     * @when we subscribe to bridgePoolUpdates
     * @then the first frame has all-zero pool totals, lastEventBlockHeight null and newEvent null
     */
    test.todo('should emit current pool summary immediately on subscribe');

    /**
     * @given we are subscribed to bridgePoolUpdates in live mode
     * @when a new ReserveTransfer event is indexed
     * @then we receive a frame with newEvent BridgeReserveTransfer and an updated pool.reserveTotal
     */
    test.todo('should push an update when a new ReserveTransfer is indexed');

    /**
     * @given we are subscribed to bridgePoolUpdates in live mode
     * @when a new InvalidTransfer event is indexed
     * @then we receive a frame with newEvent BridgeInvalidTransfer and an increased INVALID treasury aggregate
     */
    test.todo('should push an update when a new InvalidTransfer is indexed');

    /**
     * @given we are subscribed to bridgePoolUpdates in live mode
     * @when a UserTransfer event is indexed
     * @then no frame is pushed (a user transfer affects a user address, not a pool)
     */
    test.todo('should not push a pool update for UserTransfer events (user address, not pool)');

    /**
     * @given 3 ReserveTransfer events with known amounts A1, A2, A3 are indexed while subscribed
     * @when we receive a frame per pool-affecting event
     * @then pool.reserveTotal is A1, then A1+A2, then A1+A2+A3 across successive frames
     */
    test.todo('should carry a running pool total in each frame (cumulative consistency)');

    // Blocked: approval governance not landed yet (#940), so UnapprovedTransfer
    // cannot be produced.
    test.skip('should push an update when an UnapprovedTransfer is indexed', () => {});

    // Blocked: SubminimalFlush threshold not yet confirmed (see header note, #944).
    test.skip('should push an update and increment subminimumTxCount when a flush is indexed', () => {});
  });
});
