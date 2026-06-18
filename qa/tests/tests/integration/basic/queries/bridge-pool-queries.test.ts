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

// Integration tests for c2m-bridge pool observability queries: bridgePoolSummary,
// bridgeReserveInflows, bridgeTreasuryInflows (#944).
//
// Pool observability tracks where NIGHT flows when bridge transactions are
// processed: the Reserve pool (ReserveTransfer) and the Treasury (Invalid,
// Unapproved, SubminimalFlush). Per-address balance (UserTransfer/claims) is
// covered by #941. bridgePoolSummary returns cumulative totals per category plus
// a count of subminimum txs aggregated into flush events.
//
// These are skeletons enumerating the intended cases:
//   test.todo → needs bridge event data in the test environment. When
//               implemented, follow the framework conventions
//               (how-to-write-a-qa-indexer-test): issue the query via an
//               IndexerHttpClient method, define response types in
//               utils/indexer/indexer-types.ts, gate on bridge availability,
//               assert with toBeSuccess(), and set ctx.task labels.
//   test.skip → blocked on an in-flight feature: UNAPPROVED on approval
//               governance (#940); SUBMINIMAL_FLUSH on confirming whether the
//               flush threshold is a configurable genesis parameter (node team)
//               so a flush can be triggered deterministically in a test env.
//
// Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/944

describe('bridge pool queries', () => {
  describe('bridgePoolSummary', () => {
    /**
     * @given no bridge events have been indexed (or atBlock before the first bridge block)
     * @when we query bridgePoolSummary
     * @then reserveTotal is zero, every treasuryByReason total is zero,
     *       subminimumTxCount is 0 and lastEventBlockHeight is null
     */
    test.todo('should return all-zero totals when no bridge events have been indexed');

    /**
     * @given the with-data chain has N ReserveTransfer events with known amounts
     * @when we query bridgePoolSummary
     * @then reserveTotal equals the sum of all ReserveTransfer.amount values
     */
    test.todo('should set reserveTotal to the cumulative sum of ReserveTransfer amounts');

    /**
     * @given the with-data chain has InvalidTransfer and UnapprovedTransfer events
     * @when we query bridgePoolSummary
     * @then treasuryByReason contains INVALID and UNAPPROVED entries with correct totals
     */
    test.todo('should aggregate treasury inflows separately by INVALID and UNAPPROVED reason');

    /**
     * @given bridge events exist up to a known block height H
     * @when we query bridgePoolSummary
     * @then lastEventBlockHeight equals H
     */
    test.todo('should set lastEventBlockHeight to the most recently indexed bridge event block');

    /**
     * @given bridge events exist at blocks B1 and B2 (B1 < B2)
     * @when we query bridgePoolSummary(atBlock: B1) and (atBlock: B2)
     * @then only events up to and including the given block contribute, and B2 totals exceed B1
     */
    test.todo('should return a point-in-time snapshot when atBlock is given');

    // Blocked: requires crossing the subminimum accumulation threshold to produce
    // a SubminimalFlushTransfer. Pending confirmation of whether the threshold is
    // a configurable genesis parameter (node team) so a flush can be triggered.
    // Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/944
    test.skip('should count SubminimalFlushTransfer.count in subminimumTxCount', () => {});

    // Blocked: approval governance not landed yet, so UnapprovedTransfer cannot
    // be produced. Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/940
    test.skip('should aggregate UnapprovedTransfer amounts under UNAPPROVED treasury reason', () => {});
  });

  describe('bridgeReserveInflows', () => {
    /**
     * @given no ReserveTransfer events have been indexed
     * @when we query bridgeReserveInflows
     * @then the response is successful and returns an empty array
     */
    test.todo('should return an empty list when no ReserveTransfer events are indexed');

    /**
     * @given the with-data chain has ReserveTransfer events across multiple blocks
     * @when we query bridgeReserveInflows(blockHeightFrom: B1, blockHeightTo: B2)
     * @then only events with blockHeight in [B1, B2] are returned
     */
    test.todo('should return ReserveTransfer events within the specified block range');

    /**
     * @given at least one ReserveTransfer is indexed
     * @when we query bridgeReserveInflows(limit: 1)
     * @then the event exposes id, blockHeight, midnightTxHash, cardanoTxHash, amount as non-empty
     */
    test.todo('should return events with correct BridgeReserveTransfer field shape');

    /**
     * @given at least 3 ReserveTransfer events are indexed
     * @when we query with limit=2 offset=0 and limit=2 offset=1
     * @then results are consistent and ids are in ascending order
     */
    test.todo('should paginate ReserveTransfer events with offset and limit');
  });

  describe('bridgeTreasuryInflows', () => {
    /**
     * @given no treasury-redirected events (Invalid, Unapproved, SubminimalFlush) are indexed
     * @when we query bridgeTreasuryInflows
     * @then the response is successful and returns an empty array
     */
    test.todo('should return an empty list when no treasury-redirected events are indexed');

    /**
     * @given the with-data chain has InvalidTransfer events
     * @when we query bridgeTreasuryInflows with no reason filter
     * @then the results include BridgeInvalidTransfer events
     */
    test.todo('should return all treasury event types when no reason filter is given');

    /**
     * @given the with-data chain has both Invalid and Reserve events
     * @when we query bridgeTreasuryInflows(reason: INVALID)
     * @then every returned event has __typename BridgeInvalidTransfer
     */
    test.todo('should return only BridgeInvalidTransfer when reason=INVALID');

    /**
     * @given treasury events exist at blocks B1 and B3
     * @when we query bridgeTreasuryInflows(blockHeightFrom: B1, blockHeightTo: B2) where B2 < B3
     * @then only events at B1 are returned (B3 is outside the range)
     */
    test.todo('should respect blockHeightFrom and blockHeightTo filters for treasury inflows');

    // Blocked: approval governance not landed yet (#940).
    test.skip('should return only BridgeUnapprovedTransfer when reason=UNAPPROVED', () => {});

    // Blocked: SubminimalFlush threshold not yet confirmed (see header note, #944).
    test.skip('should return only BridgeSubminimalFlushTransfer when reason=SUBMINIMAL_FLUSH', () => {});
  });
});
