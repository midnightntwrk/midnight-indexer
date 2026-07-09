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

import log from '@utils/logging/logger';
import { env } from 'environment/model';
import type { TestContext } from 'vitest';
import '@utils/logging/test-logging-hooks';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { contractEventsSurfacePresent } from '@utils/indexer/contract-events-support';
import { ContractEventUnionSchema } from '@utils/indexer/graphql/schema';
import { CONTRACT_EVENT_TYPES } from '@utils/indexer/indexer-types';
import dataProvider, { type EventEmittingContractInfo } from '@utils/testdata-provider';

const httpClient = new IndexerHttpClient();

// Maps each concrete event __typename to its ContractEventType enum value, so a
// type-filtered query result can be checked against the requested type.
const TYPE_BY_TYPENAME: Record<string, (typeof CONTRACT_EVENT_TYPES)[number]> = {
  ShieldedSpendEvent: 'SHIELDED_SPEND',
  ShieldedReceiveEvent: 'SHIELDED_RECEIVE',
  ShieldedMintEvent: 'SHIELDED_MINT',
  ShieldedBurnEvent: 'SHIELDED_BURN',
  UnshieldedSpendEvent: 'UNSHIELDED_SPEND',
  UnshieldedReceiveEvent: 'UNSHIELDED_RECEIVE',
  UnshieldedMintEvent: 'UNSHIELDED_MINT',
  UnshieldedBurnEvent: 'UNSHIELDED_BURN',
  PausedEvent: 'PAUSED',
  UnpausedEvent: 'UNPAUSED',
  MiscContractEvent: 'MISC',
};

type IntrospectedType = {
  kind: string;
  fields: { name: string }[] | null;
  inputFields: { name: string }[] | null;
  enumValues: { name: string }[] | null;
  possibleTypes: { name: string }[] | null;
};

// Per-type introspection issued directly (mirrors the schema smoke tests). The
// contractEvents surface (PR #1185) is not yet on every environment, so the
// whole suite gates on a one-shot presence probe: it asserts the contract where
// the surface exists and skips where it does not, instead of failing on envs
// that have not yet received the feature.
async function introspect(typeName: string): Promise<IntrospectedType | null> {
  const query = `query Introspect($name: String!) {
    __type(name: $name) {
      kind
      fields { name }
      inputFields { name }
      enumValues { name }
      possibleTypes { name }
    }
  }`;
  const response = await fetch(env.getIndexerHttpBaseURL() + '/api/v4/graphql', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ query, variables: { name: typeName } }),
  });
  const json = (await response.json()) as { data?: { __type: IntrospectedType | null } };
  return json.data?.__type ?? null;
}

describe('contract event queries', () => {
  let surfacePresent = false;

  beforeAll(async () => {
    surfacePresent = await contractEventsSurfacePresent();
    if (!surfacePresent) {
      log.warn(
        `Contract events surface absent on ${env.getCurrentEnvironmentName()}; ` +
          `contract event query tests will be skipped.`,
      );
    }
  }, 30_000);

  describe('the contract event GraphQL schema', () => {
    /**
     * The ContractEvent interface exposes the common fields shared by every variant.
     *
     * @given a deployed indexer exposing the contract events surface
     * @when the ContractEvent type is introspected
     * @then it is an interface carrying the eight common fields, including the
     *       transaction navigation field
     */
    test('should expose ContractEvent as an interface with the common fields', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'ContractEvents', 'SchemaValidation'] };
      if (!surfacePresent) return ctx.skip?.(true, 'contract events surface not present');

      const type = await introspect('ContractEvent');
      expect(type, 'ContractEvent type not found in schema').not.toBeNull();
      expect(type!.kind).toBe('INTERFACE');

      const fieldNames = (type!.fields ?? []).map((f) => f.name);
      for (const expected of [
        'id',
        'raw',
        'maxId',
        'protocolVersion',
        'version',
        'contractAddress',
        'transactionId',
        'transaction',
      ]) {
        expect(fieldNames, `ContractEvent.${expected} missing`).toContain(expected);
      }
    });

    /**
     * The ContractEvent interface is implemented by all eleven concrete event types.
     *
     * @given a deployed indexer exposing the contract events surface
     * @when the ContractEvent interface possible types are introspected
     * @then the eleven MIP-0002 variants (Shielded*, Unshielded*, Paused, Unpaused, Misc) are present
     */
    test('should expose all eleven concrete contract event types', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'ContractEvents', 'SchemaValidation'] };
      if (!surfacePresent) return ctx.skip?.(true, 'contract events surface not present');

      const type = await introspect('ContractEvent');
      const possible = (type!.possibleTypes ?? []).map((t) => t.name).sort();
      expect(possible).toEqual(
        [
          'MiscContractEvent',
          'PausedEvent',
          'ShieldedBurnEvent',
          'ShieldedMintEvent',
          'ShieldedReceiveEvent',
          'ShieldedSpendEvent',
          'UnpausedEvent',
          'UnshieldedBurnEvent',
          'UnshieldedMintEvent',
          'UnshieldedReceiveEvent',
          'UnshieldedSpendEvent',
        ].sort(),
      );
    });

    /**
     * The ContractEventType enum carries the eleven filterable event-type variants.
     *
     * @given a deployed indexer exposing the contract events surface
     * @when the ContractEventType enum is introspected
     * @then the eleven variants (SHIELDED_SPEND … MISC) are present
     */
    test('should expose the ContractEventType enum with eleven variants', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'ContractEvents', 'SchemaValidation'] };
      if (!surfacePresent) return ctx.skip?.(true, 'contract events surface not present');

      const type = await introspect('ContractEventType');
      const values = (type!.enumValues ?? []).map((v) => v.name).sort();
      expect(values).toEqual([...CONTRACT_EVENT_TYPES].sort());
    });

    /**
     * The ContractEventFilter input exposes the documented filter fields.
     *
     * @given a deployed indexer exposing the contract events surface
     * @when the ContractEventFilter input type is introspected
     * @then contractAddress, types, fieldPrefixes, fromBlock, toBlock and transactionHash are present
     */
    test('should expose the ContractEventFilter input fields', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'ContractEvents', 'SchemaValidation'] };
      if (!surfacePresent) return ctx.skip?.(true, 'contract events surface not present');

      const type = await introspect('ContractEventFilter');
      const inputs = (type!.inputFields ?? []).map((f) => f.name);
      for (const expected of [
        'contractAddress',
        'types',
        'fieldPrefixes',
        'fromBlock',
        'toBlock',
        'transactionHash',
      ]) {
        expect(inputs, `ContractEventFilter.${expected} missing`).toContain(expected);
      }
    });

    /**
     * The FieldPrefixFilter input exposes the field-name and prefix fields.
     *
     * @given a deployed indexer exposing the contract events surface
     * @when the FieldPrefixFilter input type is introspected
     * @then fieldName and prefix are present
     */
    test('should expose the FieldPrefixFilter input fields', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'ContractEvents', 'SchemaValidation'] };
      if (!surfacePresent) return ctx.skip?.(true, 'contract events surface not present');

      const type = await introspect('FieldPrefixFilter');
      const inputs = (type!.inputFields ?? []).map((f) => f.name).sort();
      expect(inputs).toEqual(['fieldName', 'prefix']);
    });
  });

  describe('a contract events query for an address with no emitted events', () => {
    const validAddress = dataProvider.getNonExistingContractAddress();

    /**
     * A query for a valid address that emitted nothing returns an empty list.
     *
     * @given a valid-format contract address that has emitted no events
     * @when a contract events query is issued for that address
     * @then the response is successful and the event list is empty
     */
    test('should return an empty list', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'ContractEvents'] };
      if (!surfacePresent) return ctx.skip?.(true, 'contract events surface not present');

      const response = await httpClient.getContractEvents({ contractAddress: validAddress });
      expect(response).toBeSuccess();
      expect(response.data?.contractEvents).toEqual([]);
    });

    /**
     * A query narrowed by event types for an address that emitted nothing is still empty.
     *
     * @given a valid-format contract address that has emitted no events
     * @when a contract events query is issued filtering on a subset of event types
     * @then the response is successful and the event list is empty
     */
    test('should return an empty list when filtered by event types', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'ContractEvents'] };
      if (!surfacePresent) return ctx.skip?.(true, 'contract events surface not present');

      const response = await httpClient.getContractEvents({
        contractAddress: validAddress,
        types: ['SHIELDED_SPEND', 'UNSHIELDED_MINT'],
      });
      expect(response).toBeSuccess();
      expect(response.data?.contractEvents).toEqual([]);
    });

    /**
     * A query narrowed by a block range for an address that emitted nothing is still empty.
     *
     * @given a valid-format contract address that has emitted no events
     * @when a contract events query is issued bounded by fromBlock and toBlock
     * @then the response is successful and the event list is empty
     */
    test('should return an empty list when filtered by a block range', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'ContractEvents'] };
      if (!surfacePresent) return ctx.skip?.(true, 'contract events surface not present');

      const response = await httpClient.getContractEvents({
        contractAddress: validAddress,
        fromBlock: 0,
        toBlock: 1,
      });
      expect(response).toBeSuccess();
      expect(response.data?.contractEvents).toEqual([]);
    });
  });

  describe('a contract events query with an invalid filter', () => {
    /**
     * An empty contractAddress is rejected by the filter validation.
     *
     * @given a contract events filter whose contractAddress is the empty string
     * @when a contract events query is issued with that filter
     * @then the indexer responds with an error
     */
    test('should return an error when the contract address is empty', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'ContractEvents', 'Negative'] };
      if (!surfacePresent) return ctx.skip?.(true, 'contract events surface not present');

      const response = await httpClient.getContractEvents({ contractAddress: '' });
      expect(response).toBeError();
    });

    /**
     * Malformed contract addresses are rejected.
     *
     * @given a set of fabricated malformed contract addresses
     * @when a contract events query is issued for each malformed address
     * @then the indexer responds with an error for each
     */
    test('should return an error for malformed contract addresses', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'ContractEvents', 'Negative'] };
      if (!surfacePresent) return ctx.skip?.(true, 'contract events surface not present');

      const malformedAddresses = dataProvider.getFabricatedMalformedContractAddresses();
      for (const malformedAddress of malformedAddresses) {
        const response = await httpClient.getContractEvents({ contractAddress: malformedAddress });
        expect.soft(response).toBeError();
      }
    });
  });

  describe('a contract events query for a contract with emitted events', () => {
    /**
     * Emitted events conform to the contract event schema and belong to the queried contract.
     *
     * Skipped until an event-emitting contract fixture is configured for the
     * environment (see testdata-provider.getEventEmittingContracts); the
     * emit-bearing toolchain path is tracked by midnight-indexer#1163.
     *
     * @given a contract known to have emitted public contract events
     * @when a contract events query is issued for that contract
     * @then the response is successful and every event matches the contract event
     *       schema and reports the queried contract address
     */
    test('should return events conforming to the contract event schema', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'ContractEvents'] };
      if (!surfacePresent) return ctx.skip?.(true, 'contract events surface not present');

      let contracts: EventEmittingContractInfo[];
      try {
        contracts = dataProvider.getEventEmittingContracts();
      } catch (error) {
        log.warn(error);
        return ctx.skip?.(true, (error as Error).message);
      }

      for (const contract of contracts) {
        const address = contract['contract-address'];
        const response = await httpClient.getContractEvents({ contractAddress: address });
        expect(response).toBeSuccess();

        const events = response.data?.contractEvents ?? [];
        expect(events.length).toBeGreaterThan(0);
        for (const event of events) {
          const parsed = ContractEventUnionSchema.safeParse(event);
          expect(
            parsed.success,
            `Contract event schema validation failed: ${JSON.stringify(parsed.error, null, 2)}`,
          ).toBe(true);
          expect(event.contractAddress).toBe(address);
        }
      }
    });

    /**
     * Filtering by a single event type returns only events of that type.
     *
     * Skipped until an event-emitting contract fixture is configured for the
     * environment; tracked by midnight-indexer#1163.
     *
     * @given a contract that emitted events of more than one type (e.g. an
     *        UNPAUSED and a MISC event)
     * @when a contract events query is issued filtering on a single type
     * @then the response contains at least one event and every returned event is
     *       of the requested type
     */
    test('should return only events of the requested type when filtered', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'ContractEvents'] };
      if (!surfacePresent) return ctx.skip?.(true, 'contract events surface not present');

      let contracts: EventEmittingContractInfo[];
      try {
        contracts = dataProvider.getEventEmittingContracts();
      } catch (error) {
        log.warn(error);
        return ctx.skip?.(true, (error as Error).message);
      }

      const withTypes = contracts.find((contract) =>
        (contract['event-types'] ?? []).some((type) =>
          CONTRACT_EVENT_TYPES.includes(type as (typeof CONTRACT_EVENT_TYPES)[number]),
        ),
      );
      if (!withTypes) {
        return ctx.skip?.(true, 'no event-emitting contract fixture declares a known event-type');
      }

      const requestedType = withTypes['event-types']!.find((type) =>
        CONTRACT_EVENT_TYPES.includes(type as (typeof CONTRACT_EVENT_TYPES)[number]),
      ) as (typeof CONTRACT_EVENT_TYPES)[number];

      const response = await httpClient.getContractEvents({
        contractAddress: withTypes['contract-address'],
        types: [requestedType],
      });
      expect(response).toBeSuccess();

      const events = response.data?.contractEvents ?? [];
      expect(events.length).toBeGreaterThan(0);
      for (const event of events) {
        expect(TYPE_BY_TYPENAME[event.__typename]).toBe(requestedType);
      }
    });
  });
});
