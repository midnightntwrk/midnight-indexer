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

// Smoke-level schema presence checks for the c2m-bridge GraphQL surface.
//
// These tests verify that every bridge type, query, and subscription declared
// in #941 and #942 actually appears in the deployed schema. They run via
// introspection so they cost only one HTTP round-trip each, and they fail fast
// if a deployment is missing bridge support entirely.
//
// All tests are marked it.todo until the dev PRs (#938-#942) are merged and
// the schema-v4.graphql is regenerated. Un-todo them as part of the final
// QA sign-off on those PRs.
//
// Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/941
//           https://github.com/midnightntwrk/midnight-indexer/issues/942

import log from '@utils/logging/logger';
import '@utils/logging/test-logging-hooks';
import { IndexerHttpClient } from '@utils/indexer/http-client';

const INTROSPECT_TYPE = (name: string) => `
  query {
    __type(name: "${name}") {
      name
      kind
      fields { name }
      enumValues { name }
      possibleTypes { name }
    }
  }
`;

const INTROSPECT_QUERY_FIELDS = `
  query {
    __type(name: "Query") {
      fields { name }
    }
  }
`;

const INTROSPECT_SUBSCRIPTION_FIELDS = `
  query {
    __type(name: "Subscription") {
      fields { name }
    }
  }
`;

const httpClient = new IndexerHttpClient();

function rawRequest<T>(query: string): Promise<{ data: T | null }> {
  return (
    httpClient as unknown as {
      client: { rawRequest: (q: string) => Promise<{ data: T | null }> };
    }
  ).client.rawRequest(query);
}

// #941 — GraphQL types and queries for bridge events
describe('bridge GraphQL schema — types (#941)', () => {
  // BridgeEvent interface: shared fields across all 5 event variants.
  it.todo('should expose BridgeEvent interface');

  // Concrete types implementing BridgeEvent, one per pallet event variant.
  it.todo('should expose BridgeUserTransfer type');
  it.todo('should expose BridgeReserveTransfer type');
  it.todo('should expose BridgeInvalidTransfer type');
  it.todo('should expose BridgeUnapprovedTransfer type');
  it.todo('should expose BridgeSubminimalFlushTransfer type');

  // BridgeClaimTransaction: a ClaimRewards transaction with kind=CardanoBridge,
  // surfaced as its own Transaction type (no bridge-specific query).
  it.todo('should expose BridgeClaimTransaction type implementing Transaction');

  // BridgeBalance: deposited / claimed / balance summary per address.
  it.todo('should expose BridgeBalance type with deposited, claimed, balance fields');

  // Discriminator enum used by bridgeEvents(variant: ...) filter.
  it.todo('should expose BridgeEventVariant enum with 5 values');
});

describe('bridge GraphQL schema — queries (#941)', () => {
  // bridgeEvents: filterable list of BridgeEvent (by recipient, variant, block range).
  it.todo('should expose bridgeEvents query on Query type');

  // bridgeBalance: deposited / claimed / balance summary for an address.
  it.todo('should expose bridgeBalance query on Query type');

  // bridgeDeposits: convenience filter combining UserTransfer + optional UnapprovedTransfer.
  it.todo('should expose bridgeDeposits query on Query type');

  // Note: the bridge pool query surface (bridgeReserveInflows, bridgeTreasuryInflows,
  // bridgePoolSummary) is owned by the pool-observability work (#944) and is asserted
  // in that branch's tests, not here.
});

// #942 — GraphQL subscriptions for bridge events
describe('bridge GraphQL schema — subscriptions (#942)', () => {
  // Real-time push of new BridgeEvents; supports from-cursor reconnection.
  it.todo('should expose bridgeEvents subscription on Subscription type');

  // Live BridgeBalance recomputed on every matching event for an address.
  it.todo('should expose bridgeBalance subscription on Subscription type');

  // Note: the bridgePoolUpdates subscription is owned by the pool-observability
  // work (#944) and is asserted in that branch's tests, not here.
});

// Reference implementation for when it.todo is removed:
// Replace each it.todo block with a test body like this example.
//
// test('should expose BridgeEvent interface', async (ctx) => {
//   ctx.task!.meta.custom = { labels: ['Smoke', 'Bridge', 'Schema'] };
//   const response = await rawRequest<IntrospectionTypeResult>(INTROSPECT_TYPE('BridgeEvent'));
//   expect(response.data?.__type).not.toBeNull();
//   expect(response.data?.__type?.kind).toBe('INTERFACE');
//   expect(response.data?.__type?.fields?.map((f) => f.name)).toContain('id');
//   expect(response.data?.__type?.fields?.map((f) => f.name)).toContain('midnightTxHash');
//   expect(response.data?.__type?.fields?.map((f) => f.name)).toContain('blockHeight');
//   log.debug('BridgeEvent interface confirmed in schema');
// });

void INTROSPECT_TYPE;
void INTROSPECT_QUERY_FIELDS;
void INTROSPECT_SUBSCRIPTION_FIELDS;
void log;
void rawRequest;
