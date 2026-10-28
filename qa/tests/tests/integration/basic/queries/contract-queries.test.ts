// This file is part of midnight-indexer.
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
import { env, EnvironmentName } from 'environment/model';
import type { TestContext } from 'vitest';
import { z } from 'zod';
import '@utils/logging/test-logging-hooks';
import {
  ContractActionStateAtSchema,
  ContractBalanceSchema,
  ContractDeployActionSchema,
  ContractCallActionSchema,
  ContractStateAtSchema,
  ContractUpdateActionSchema,
} from '@utils/indexer/graphql/schema';
import type { BlockReference, ContractActionStateAt } from '@utils/indexer/graphql/schema';
import { GET_CONTRACT_ACTION_STATE_AT } from '@utils/indexer/graphql/contract-queries';
import { GET_CONTRACT_STATE_AT } from '@utils/indexer/graphql/contract-type-queries';
import { GET_TRANSACTION_CONTRACT_ACTIONS_STATE_AT } from '@utils/indexer/graphql/transaction-queries';
import dataProvider, {
  type LedgerHardForkInfo,
  type TokenHoldingContractInfo,
} from '@utils/testdata-provider';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { NodeRpcClient } from '@utils/node/rpc-client';
import { contractEventsSurfacePresent } from '@utils/indexer/contract-events-support';
import {
  CONTRACT_STATE_V6_TAG,
  CONTRACT_STATE_V8_TAG,
  contractStateTag,
  currentStateAt,
  expectAnchorConfirmedByNode,
  forkBlock,
  ownBlock,
} from 'tests/shared/contract-state-utils';

/** Introspect the field names on a GraphQL type (null when the type is absent). */
async function introspectFieldNames(typeName: string): Promise<string[] | null> {
  const query = `query Fields($name: String!) { __type(name: $name) { fields { name } } }`;
  const response = await fetch(env.getIndexerHttpBaseURL() + '/api/v4/graphql', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ query, variables: { name: typeName } }),
  });
  const json = (await response.json()) as {
    data?: { __type: { fields: { name: string }[] } | null };
  };
  const fields = json.data?.__type?.fields;
  return fields ? fields.map((f) => f.name) : null;
}

const indexerHttpClient = new IndexerHttpClient();

describe('contract queries', () => {
  describe('a contract query by address', () => {
    /**
     * A contract query with boundary contract addresses returns success
     *
     * @given we have boundary contract addresses
     * @when we send a contract query using each boundary address
     * @then Indexer should respond with success for each address
     */
    test('should return success as they are valid contract addresses', async () => {
      const malformedAddress: string[] = dataProvider.getBoundaryContractAddresses();

      for (const address of malformedAddress) {
        const response = await indexerHttpClient.getContractAction(address);
        expect(response).toBeSuccess();
        expect(response.data?.contractAction).toBeDefined();
      }
    });

    /**
     * A contract query by address returns the most recent action for a contract with multiple actions
     *
     * @given we have a contract address with multiple actions (ContractDeploy at block 49, ContractUpdate at block 59)
     * @when we send a contract query using that address without offset
     * @then Indexer should respond with successful response and return the most recent action (ContractUpdate)
     */
    test('should return the most recent action for a contract with multiple actions', async (ctx: TestContext) => {
      let existingContractAddress: string;
      try {
        existingContractAddress = dataProvider.getKnownContractAddress() as string;
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
      }

      const response = await indexerHttpClient.getContractAction(existingContractAddress!);

      expect(response).toBeSuccess();
      expect(response.data?.contractAction).not.toBeNull();
      expect(response.data?.contractAction?.address).toBe(existingContractAddress!);
      expect(['ContractUpdate', 'ContractCall']).toContain(
        response.data?.contractAction?.__typename,
      );
    });

    /**
     * A contract query by address returns null when the contract does not exist
     *
     * @given we have a non-existent contract address
     * @when we send a contract query using that address
     * @then Indexer should respond with null contract action
     */
    test('should return null when contract with that address does not exist', async (ctx: TestContext) => {
      let nonExistentContractAddress: string;
      try {
        nonExistentContractAddress = dataProvider.getNonExistingContractAddress();
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
      }

      const response = await indexerHttpClient.getContractAction(nonExistentContractAddress!);
      expect(response).toBeSuccess();
      expect(response.data?.contractAction).toBeNull();
    });

    /**
     * A contract query with malformed addresses returns an error
     *
     * @given we have fabricated malformed contract addresses
     * @when we send a contract query using each malformed address
     * @then Indexer should respond with an error for each address
     */
    test('should return an error for malformed addresses', async () => {
      const fabricatedMalformedAddresses = dataProvider.getFabricatedMalformedContractAddresses();

      for (const malformedAddress of fabricatedMalformedAddresses) {
        const response = await indexerHttpClient.getContractAction(malformedAddress);

        expect.soft(response).toBeError();
      }
    });

    /**
     * A contract known to hold unshielded tokens reports non-empty unshieldedBalances
     *
     * Regression test for #1245: unshieldedBalances silently returned an empty array for
     * every contract from 3.0.0 to 4.3.3, which format-only assertions could not catch.
     * Requires an indexer with the #1246 fix and repaired history (backfill or re-index);
     * skipped on environments without configured token-holding contracts.
     *
     * @given we have contracts known to hold non-zero unshielded token balances
     * @when we send a contract query for each of them
     * @then unshieldedBalances is non-empty, well-formed, has positive amounts, and
     *       contains the expected token type
     */
    test('should return non-empty unshieldedBalances for contracts known to hold tokens', async (ctx: TestContext) => {
      let tokenHoldingContracts: TokenHoldingContractInfo[];
      try {
        tokenHoldingContracts = dataProvider.getTokenHoldingContracts();
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
        return;
      }
      if (tokenHoldingContracts.length === 0) {
        ctx.skip?.(true, 'no token-holding contracts configured for this environment');
        return;
      }

      for (const contract of tokenHoldingContracts) {
        const response = await indexerHttpClient.getContractAction(contract['contract-address']);

        expect(response).toBeSuccess();
        const balances = response.data?.contractAction?.unshieldedBalances;
        expect(balances).toBeDefined();
        expect(balances!.length).toBeGreaterThan(0);

        for (const balance of balances!) {
          expect(ContractBalanceSchema.safeParse(balance).success).toBe(true);
          expect(BigInt(balance.amount)).toBeGreaterThan(0n);
        }

        if (contract['token-type'] !== undefined) {
          expect(balances!.map((balance) => balance.tokenType.toLowerCase())).toContain(
            contract['token-type'].toLowerCase(),
          );
        }
      }
    });
  });

  describe('a contract query by address and offset', () => {
    const validAddress = dataProvider.getNonExistingContractAddress();

    /**
     * A contract query by address and offset returns the correct action using exact block hash
     *
     * @given we have an existing contract address and the exact block hash where it was included
     * @when we send a contract query using that address and block hash
     * @then Indexer should respond with successful response and non-null contractAction with correct data
     */
    test('should return the correct action using exact block hash where it was included', async (ctx: TestContext) => {
      let existingContractAddress: string;
      let contractDeployBlockHash: string;
      try {
        existingContractAddress = dataProvider.getKnownContractAddress() as string;
        contractDeployBlockHash = await dataProvider.getContractDeployBlockHash();
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
      }
      const response = await indexerHttpClient.getContractAction(existingContractAddress!, {
        blockOffset: { hash: contractDeployBlockHash! },
      });

      expect(response).toBeSuccess();
      expect(response.data?.contractAction).not.toBeNull();
      expect(response.data?.contractAction?.address).toBe(existingContractAddress!);
    });

    /**
     * A contract query by address and offset returns the latest state using a future block hash
     *
     * @given we have an existing contract address and a valid block hash from a future block
     * @when we send a contract query using that address and future block hash
     * @then Indexer should respond with successful response and non-null contractAction reflecting latest state
     */
    test('should return the latest state using a future block hash', async (ctx: TestContext) => {
      let existingContractAddress: string;
      let contractUpdateBlockHash: string;
      try {
        existingContractAddress = dataProvider.getContractAddressForActionType('ContractUpdate');
        contractUpdateBlockHash = await dataProvider.getContractUpdateBlockHash();
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
      }

      const response = await indexerHttpClient.getContractAction(existingContractAddress!, {
        blockOffset: { hash: contractUpdateBlockHash! },
      });

      expect(response).toBeSuccess();
      expect(response.data?.contractAction).not.toBeNull();
      expect(response.data?.contractAction?.address).toBe(existingContractAddress!);
    });

    /**
     * A contract query by address returns a contract action that conforms to the correct schema
     *
     * @given we have an existing contract address
     * @when we send a contract query using that address
     * @then Indexer should respond with successful response and contractAction that conforms to the correct schema
     */
    test('should respond with a contract action according to the expected schema', async (ctx: TestContext) => {
      let contractAddress: string;
      try {
        contractAddress = dataProvider.getKnownContractAddress() as string;
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
      }
      const response = await indexerHttpClient.getContractAction(contractAddress!);
      expect(response).toBeSuccess();
      expect(response.data?.contractAction).toBeDefined();

      const contractAction = response.data!.contractAction!;
      const typename = contractAction.__typename;

      log.debug(`Validating contract action schema for type: ${typename}`);

      const schemaMap = {
        ContractDeploy: ContractDeployActionSchema,
        ContractCall: ContractCallActionSchema,
        ContractUpdate: ContractUpdateActionSchema,
      } as const;

      expect(
        Object.keys(schemaMap).includes(typename),
        `Unexpected contract action type: ${typename}`,
      ).toBe(true);

      const schema = schemaMap[typename as keyof typeof schemaMap];
      log.debug(`Validating with schema: ${typename}`);

      const parsed = schema.safeParse(contractAction);

      if (!parsed.success) {
        log.debug('Schema validation failed');
        log.debug(JSON.stringify(parsed.error, null, 2));
      } else {
        log.debug(`Schema validation succeeded for ${typename}`);
      }
      expect(
        parsed.success,
        `Contract action schema validation failed: ${JSON.stringify(parsed.error, null, 2)}`,
      ).toBe(true);
    });

    /**
     * A contract query by address and offset returns the correct action using exact block height
     *
     * @given we have an existing contract address and the exact block height where it was included
     * @when we send a contract query using that address and block height
     * @then Indexer should respond with successful response and non-null contractAction with correct data
     */
    test('should return the correct action using exact block height where it was included', async (ctx: TestContext) => {
      let existingContractAddress: string;
      let contractDeployHeight: number;
      try {
        existingContractAddress = dataProvider.getKnownContractAddress() as string;
        contractDeployHeight = await dataProvider.getContractDeployBlockHeight();
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
      }
      const response = await indexerHttpClient.getContractAction(existingContractAddress!, {
        blockOffset: { height: contractDeployHeight! },
      });

      expect(response).toBeSuccess();
      expect(response.data?.contractAction).not.toBeNull();
      expect(response.data?.contractAction?.address).toBe(existingContractAddress!);
    });

    /**
     * A contract query by address and offset returns the latest state using a future block height
     *
     * @given we have an existing contract address and a valid block height from a future block
     * @when we send a contract query using that address and future block height
     * @then Indexer should respond with successful response and non-null contractAction reflecting latest state
     */
    test('should return the latest state using a future block height', async (ctx: TestContext) => {
      let existingContractAddress: string;
      let contractUpdateHeight: number;
      try {
        existingContractAddress = dataProvider.getContractAddressForActionType('ContractUpdate');
        contractUpdateHeight = await dataProvider.getContractUpdateBlockHeight();
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
      }
      const response = await indexerHttpClient.getContractAction(existingContractAddress!, {
        blockOffset: { height: contractUpdateHeight! },
      });

      expect(response).toBeSuccess();
      expect(response.data?.contractAction).not.toBeNull();
      expect(response.data?.contractAction?.address).toBe(existingContractAddress!);
    });

    /**
     * A contract query by address and block offset by height returns the most recent contract action for that address before the specified block
     *
     * @given we have multiple contract actions in different blocks (example: ContractDeploy block 49, ContractUpdate block 59)
     * @when we send a contract query using that address and a past block height (example: block 49)
     * @then Indexer should return the most recent action for the address before the specified block height (so ContractDeploy block 49)
     */
    test('should return the most recent contract action for that address before the specified block', async (ctx: TestContext) => {
      let existingContractAddress: string;
      let contractDeployHeight: number;
      let contractCallHeight: number;

      try {
        existingContractAddress = dataProvider.getKnownContractAddress() as string;
        contractDeployHeight = await dataProvider.getContractDeployBlockHeight();
        contractCallHeight = await dataProvider.getContractCallBlockHeight();
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
      }
      let response = await indexerHttpClient.getContractAction(existingContractAddress!, {
        blockOffset: { height: contractDeployHeight! },
      });

      expect(response).toBeSuccess();
      expect(response.data?.contractAction).not.toBeNull();
      expect(response.data?.contractAction?.address).toBe(existingContractAddress!);
      expect(response.data?.contractAction?.__typename).toBe('ContractDeploy');

      response = await indexerHttpClient.getContractAction(existingContractAddress!, {
        blockOffset: { height: contractCallHeight! },
      });

      expect(response).toBeSuccess();
      expect(response.data?.contractAction).not.toBeNull();
      expect(response.data?.contractAction?.address).toBe(existingContractAddress!);
      expect(response.data?.contractAction?.__typename).toBe('ContractCall');
    });

    /**
     * A contract query by address and offset returns null when contract does not exist
     *
     * @given we have a valid address and valid block offset
     * @when we send a contract query using that address and offset
     * @then Indexer should respond with null contract action
     */
    test('should return null when contract with valid address and valid offset does not exist', async (ctx: TestContext) => {
      let knownBlockHash: string;
      try {
        knownBlockHash = await dataProvider.getContractDeployBlockHash();
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
      }
      const response = await indexerHttpClient.getContractAction(validAddress, {
        blockOffset: { hash: knownBlockHash! },
      });
      expect(response).toBeSuccess();
      expect(response.data?.contractAction).toBeNull();
    });

    /**
     * A contract query by address and non-existing hash returns null when contract does not exist
     *
     * @given we have a valid address and non-existing block hash
     * @when we send a contract query using that address and hash
     * @then Indexer should respond with null contract action
     */
    test('should return null when contract with valid address and non-existing hash does not exist', async () => {
      const nonExistingHash = dataProvider.getNonExistingHash();
      const response = await indexerHttpClient.getContractAction(validAddress, {
        blockOffset: { hash: nonExistingHash },
      });
      expect(response).toBeSuccess();
      expect(response.data?.contractAction).toBeNull();
    });

    /**
     * A contract query with invalid address and valid hash returns an error
     *
     * @given we have an invalid address and valid block hash
     * @when we send a contract query using that address and hash
     * @then Indexer should respond with an error
     */
    test('should return error when contract with invalid address and valid hash', async (ctx: TestContext) => {
      let knownBlockHash: string;
      try {
        knownBlockHash = await dataProvider.getContractDeployBlockHash();
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
      }
      const invalidAddress = dataProvider.getFabricatedMalformedContractAddresses()[10];
      const response = await indexerHttpClient.getContractAction(invalidAddress, {
        blockOffset: { hash: knownBlockHash! },
      });
      expect(response).toBeError();
    });

    /**
     * A contract query with invalid address and non-existing hash returns an error
     *
     * @given we have an invalid address and non-existing block hash
     * @when we send a contract query using that address and hash
     * @then Indexer should respond with an error
     */
    test('should return error when contract with invalid address and non-existing hash', async () => {
      const invalidAddress = dataProvider.getFabricatedMalformedContractAddresses()[10];
      const nonExistingHash = dataProvider.getNonExistingHash();
      const response = await indexerHttpClient.getContractAction(invalidAddress, {
        blockOffset: { hash: nonExistingHash },
      });
      expect(response).toBeError();
    });

    /**
     * A contract query with valid address and invalid hash returns an error
     *
     * @given we have a valid address and invalid block hash
     * @when we send a contract query using that address and hash
     * @then Indexer should respond with an error
     */
    test('should return error when contract with valid address and invalid hash', async () => {
      const malformedHashes = dataProvider.getFabricatedMalformedHashes();
      const response = await indexerHttpClient.getContractAction(validAddress, {
        blockOffset: { hash: malformedHashes[0] },
      });
      expect(response).toBeError();
    });

    /**
     * A contract query with invalid address and invalid hash returns an error
     *
     * @given we have an invalid address and invalid block hash
     * @when we send a contract query using that address and hash
     * @then Indexer should respond with an error
     */
    test('should return error when contract with invalid address and invalid hash', async () => {
      const invalidAddress = dataProvider.getFabricatedMalformedContractAddresses()[10]; // empty string
      const malformedHashes = dataProvider.getFabricatedMalformedHashes();
      for (const malformedHash of malformedHashes) {
        const response = await indexerHttpClient.getContractAction(invalidAddress, {
          blockOffset: { hash: malformedHash },
        });
        expect(response).toBeError();
      }
    });

    /**
     * A contract query with valid address and valid height returns null when contract does not exist
     *
     * @given we have a valid address and valid block height
     * @when we send a contract query using that address and height
     * @then Indexer should respond with null contract action
     */
    test('should return null when contract with valid address and valid height does not exist', async () => {
      const response = await indexerHttpClient.getContractAction(validAddress, {
        blockOffset: { height: 0 },
      });
      expect(response).toBeSuccess();
      expect(response.data?.contractAction).toBeNull();
    });

    /**
     * A contract query with valid address and non-existing height returns null when contract does not exist
     *
     * @given we have a valid address and non-existing block height
     * @when we send a contract query using that address and height
     * @then Indexer should respond with null contract action
     */
    test('should return null when contract with valid address and non-existing height does not exist', async () => {
      const response = await indexerHttpClient.getContractAction(validAddress, {
        blockOffset: { height: 999999 },
      });
      expect(response).toBeSuccess();
      expect(response.data?.contractAction).toBeNull();
    });

    /**
     * A contract query with invalid address and valid height returns an error
     *
     * @given we have an invalid address and valid block height
     * @when we send a contract query using that address and height
     * @then Indexer should respond with an error
     */
    test('should return error when contract with invalid address and valid height', async () => {
      const invalidAddress = dataProvider.getFabricatedMalformedContractAddresses()[10];
      const response = await indexerHttpClient.getContractAction(invalidAddress, {
        blockOffset: { height: 0 },
      });
      expect(response).toBeError();
    });

    /**
     * A contract query with invalid address and invalid height returns an error
     *
     * @given we have an invalid address and invalid block height
     * @when we send a contract query using that address and height
     * @then Indexer should respond with an error
     */
    test('should return error when contract with invalid address and invalid height', async () => {
      const invalidAddress = dataProvider.getFabricatedMalformedContractAddresses()[10];
      const malformedHeights = dataProvider.getFabricatedMalformedHeights();
      for (const malformedHeight of malformedHeights) {
        const response = await indexerHttpClient.getContractAction(invalidAddress, {
          blockOffset: { height: malformedHeight },
        });
        expect(response).toBeError();
      }
    });

    /**
     * A contract query with negative height returns an error
     *
     * @given we have a valid address and negative block height
     * @when we send a contract query using that address and height
     * @then Indexer should respond with an error
     */
    test('should return error for negative height', async () => {
      const response = await indexerHttpClient.getContractAction(validAddress, {
        blockOffset: { height: -1 },
      });
      expect(response).toBeError();
    });

    /**
     * A contract query with non-integer height returns an error
     *
     * @given we have a valid address and non-integer block height
     * @when we send a contract query using that address and height
     * @then Indexer should respond with an error
     */
    test('should return error for non-integer height', async () => {
      const response = await indexerHttpClient.getContractAction(validAddress, {
        blockOffset: { height: 0.5 },
      });
      expect(response).toBeError();
    });

    /**
     * A contract query with extremely large height returns an error
     *
     * @given we have a valid address and extremely large block height
     * @when we send a contract query using that address and height
     * @then Indexer should respond with an error
     */
    test('should return error for extremely large height', async () => {
      const response = await indexerHttpClient.getContractAction(validAddress, {
        blockOffset: { height: 2 ** 32 },
      });
      expect(response).toBeError();
    });

    /**
     * A contract query by address and offset returns null when using a block hash from before the action existed
     *
     * @given we have an existing contract address and a valid block hash from before the contract existed
     * @when we send a contract query using that address and past block hash
     * @then Indexer should respond with successful response and null contractAction
     */
    test('should return null when using a block hash from before the action existed', async (ctx: TestContext) => {
      let existingContractAddress: string;
      const genesisBlockHash = (await indexerHttpClient.getBlockByOffset({ height: 0 })).data?.block
        .hash;
      try {
        existingContractAddress = dataProvider.getKnownContractAddress() as string;
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
      }
      const response = await indexerHttpClient.getContractAction(existingContractAddress!, {
        blockOffset: { hash: genesisBlockHash! },
      });

      expect(response).toBeSuccess();
      expect(response.data?.contractAction).toBeNull();
    });
  });

  describe('the contractEvents field on contract actions', () => {
    let surfacePresent = false;

    beforeAll(async () => {
      surfacePresent = await contractEventsSurfacePresent();
    }, 30_000);

    /**
     * ContractCall exposes the nested contractEvents field.
     *
     * Only ContractCall executes circuits and can therefore emit events, so the
     * field is exposed there. Gated on the contract events surface being present.
     *
     * @given a deployed indexer exposing the contract events surface
     * @when the ContractCall type is introspected
     * @then it exposes a contractEvents field
     */
    test('should expose contractEvents on ContractCall', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'ContractEvents', 'SchemaValidation'] };
      if (!surfacePresent) return ctx.skip?.(true, 'contract events surface not present');

      const fields = await introspectFieldNames('ContractCall');
      expect(fields, 'ContractCall type not found in schema').not.toBeNull();
      expect(fields).toContain('contractEvents');
    });

    /**
     * ContractDeploy and ContractUpdate never expose the contractEvents field.
     *
     * They do not execute circuits and therefore emit no events; the nested field
     * must not leak onto them. This invariant holds on every environment.
     *
     * @given the deployed indexer GraphQL schema
     * @when the ContractDeploy and ContractUpdate types are introspected
     * @then neither exposes a contractEvents field
     */
    test('should not expose contractEvents on ContractDeploy or ContractUpdate', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'ContractEvents', 'SchemaValidation'] };

      for (const typeName of ['ContractDeploy', 'ContractUpdate']) {
        const fields = await introspectFieldNames(typeName);
        expect(fields, `${typeName} type not found in schema`).not.toBeNull();
        expect(fields, `${typeName} must not expose contractEvents`).not.toContain(
          'contractEvents',
        );
      }
    });
  });

  describe('the block a contract state is anchored at', () => {
    const fork = dataProvider.getLedgerHardFork();
    const nodeClient = new NodeRpcClient();
    // Up to 3 query attempts plus 6 node requests of up to 10 s each, so a slow node fails an
    // assertion rather than the test timeout.
    const NODE_CHECKED_TEST_MS = 120_000;
    let knownContract: string | null = null;

    beforeAll(() => {
      try {
        knownContract = dataProvider.getKnownContractAddress();
      } catch (error) {
        log.warn(error);
      }
    });

    /**
     * Runs `query` for the known contract's current state and checks it against the node, which
     * the indexer under test cannot influence: the state must be the node's at the indexer tip
     * read just before the query, so a state the node has since replaced fails (#1605: the
     * pre-fork encoding served after a fork), and the node must confirm its stateAt. On a chain
     * that declares its hard fork, stateAt must also be the block the fixture predicts. A query
     * that saw a block past that tip raced a new write to the contract and is repeated.
     */
    async function expectCurrentStateConfirmedByNode(
      query: () => Promise<{
        state: string;
        stateAt: BlockReference;
        latest: ContractActionStateAt;
      }>,
    ) {
      for (let attempt = 1; ; attempt++) {
        const tip = await indexerHttpClient.getLatestBlock();
        expect(tip).toBeSuccess();
        const { height: tipHeight, hash: tipHash } = tip.data!.block;
        const { state, stateAt, latest } = await query();
        if (Math.max(stateAt.height, latest.transaction.block.height) > tipHeight) {
          expect(attempt, 'contract written during every query attempt').toBeLessThan(3);
          continue;
        }
        expect(state, `state vs the node's at the indexer tip ${tipHeight}`).toBe(
          await nodeClient.getContractState(knownContract!, tipHash),
        );
        await expectAnchorConfirmedByNode(nodeClient, knownContract!, state, stateAt, latest);
        if (fork) expect(stateAt).toEqual(currentStateAt(latest, fork));
        return;
      }
    }

    /**
     * The latest contract action's state is anchored at the block it was last written: the
     * action's own block, or the hard fork block if the chain crossed one after the action.
     *
     * @given a contract known to exist on the chain
     * @when its latest contract action is queried by address without an offset
     * @then its state is the node's state at the indexer tip and at the block stateAt names
     * @and stateAt names the action's own block, or a later block on a newer protocol version
     *      where the node's state changed: the fork block on a chain that crossed one after it
     */
    test(
      'should anchor the latest contract action state at the block it was last written',
      async (ctx: TestContext) => {
        ctx.task!.meta.custom = { labels: ['Query', 'ContractAction', 'StateAt'] };
        if (!knownContract) return ctx.skip(true, 'no known contract fixture on this env');

        await expectCurrentStateConfirmedByNode(async () => {
          const response = await indexerHttpClient.getContractAction(
            knownContract!,
            undefined,
            GET_CONTRACT_ACTION_STATE_AT,
          );
          expect(response).toBeSuccess();
          const action = ContractActionStateAtSchema.parse(response.data?.contractAction);
          expect(action.address).toBe(knownContract);
          return { state: action.state, stateAt: action.stateAt, latest: action };
        });
      },
      NODE_CHECKED_TEST_MS,
    );

    /**
     * A contract's current state is anchored where its latest action's state is.
     *
     * @given a contract known to exist on the chain
     * @when the contract is queried by address without an offset
     * @then its state is the node's state at the indexer tip and at the block stateAt names
     * @and stateAt names the block of its latest action, or a later block on a newer protocol
     *      version where the node's state changed: the fork block on a chain that crossed one
     */
    test(
      'should anchor the contract state at the block of its latest action',
      async (ctx: TestContext) => {
        ctx.task!.meta.custom = { labels: ['Query', 'Contract', 'StateAt'] };
        if (!knownContract) return ctx.skip(true, 'no known contract fixture on this env');

        await expectCurrentStateConfirmedByNode(async () => {
          const response = await indexerHttpClient.getContract(
            knownContract!,
            { actionsLimit: 1 },
            GET_CONTRACT_STATE_AT,
          );
          expect(response).toBeSuccess();
          const contract = ContractStateAtSchema.parse(response.data?.contract);
          expect(contract.actions).toHaveLength(1);
          return { state: contract.state, stateAt: contract.stateAt, latest: contract.actions[0] };
        });
      },
      NODE_CHECKED_TEST_MS,
    );

    /**
     * Recorded contract actions keep the state written by their own transaction, so each is
     * anchored at its own block, whether listed under the contract or under its transaction.
     *
     * @given a contract known to exist on the chain
     * @when its recent actions are listed through Contract.actions
     * @and the transactions of those actions are queried by hash for their contract actions
     * @then every listed contract action's stateAt names the block of its own transaction
     */
    test('should anchor every recorded contract action at its own block', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Contract', 'Transaction', 'StateAt'] };
      if (!knownContract) return ctx.skip(true, 'no known contract fixture on this env');

      const contractResponse = await indexerHttpClient.getContract(
        knownContract,
        { actionsLimit: 3 },
        GET_CONTRACT_STATE_AT,
      );
      expect(contractResponse).toBeSuccess();
      const { actions } = ContractStateAtSchema.parse(contractResponse.data?.contract);
      expect(actions.length).toBeGreaterThan(0);

      const txResponses = await Promise.all(
        actions.map((action) =>
          indexerHttpClient.getTransactionByOffset(
            { hash: action.transaction.hash },
            GET_TRANSACTION_CONTRACT_ACTIONS_STATE_AT,
          ),
        ),
      );
      for (const [i, action] of actions.entries()) {
        expect(action.stateAt, `Contract.actions item ${action.transaction.hash}`).toEqual(
          ownBlock(action),
        );

        const txResponse = txResponses[i];
        expect(txResponse).toBeSuccess();
        const txActions = z
          .array(ContractActionStateAtSchema)
          .parse(txResponse.data?.transactions.flatMap((tx) => tx.contractActions));
        expect(txActions.length).toBeGreaterThan(0);
        for (const txAction of txActions) {
          expect(
            txAction.stateAt,
            `Transaction.contractActions item ${txAction.transaction.hash}`,
          ).toEqual(ownBlock(txAction));
        }
      }
    });
  });
});

// Expected states come from the node and blocks from the fork fixture, never from the indexer
// under test: a contract untouched since the fork must serve the node's translated state.
describe.runIf(env.getCurrentEnvironmentName() === EnvironmentName.DEVNET)(
  'contract queries across a ledger hard fork',
  () => {
    const nodeClient = new NodeRpcClient();
    let fork: LedgerHardForkInfo;
    let address: string;
    let actions: LedgerHardForkInfo['untouched-contracts'][number]['contract-actions'];

    beforeAll(() => {
      const declared = dataProvider.getLedgerHardFork();
      if (!declared) throw new Error('devnet must declare the hard fork it crossed');
      fork = declared;
      ({ 'contract-address': address, 'contract-actions': actions } =
        fork['untouched-contracts'][0]);
    });

    /** The node's state of the contract as of the given height, checked for its encoding. */
    async function nodeStateAt(height: number, tag: string): Promise<string> {
      const blockHash = await nodeClient.getBlockHash(height);
      expect(blockHash, `node block hash at height ${height}`).not.toBeNull();
      const state = await nodeClient.getContractState(address, blockHash!);
      expect(contractStateTag(state), `node state tag at height ${height}`).toBe(tag);
      return state;
    }

    describe('a contract untouched since the fork queried without an offset', () => {
      /**
       * The latest contract action of a contract untouched since the fork serves the state the
       * node serves: the pre-fork state translated to the post-fork encoding.
       *
       * @given a contract deployed and last called before the fork (latest action at 124,423)
       * @when its latest contract action is queried by address without an offset
       * @then its state starts with midnight:contract-state[v8] and matches the node's state
       */
      test('should serve the contract action state in the post-fork encoding of the node', async (ctx: TestContext) => {
        ctx.task!.meta.custom = { labels: ['Query', 'ContractAction', 'HardFork'] };

        const nodeState = await nodeClient.getContractState(address);
        const response = await indexerHttpClient.getContractAction(address);

        expect(response).toBeSuccess();
        const state = response.data!.contractAction!.state;
        expect(contractStateTag(nodeState), 'node state tag').toBe(CONTRACT_STATE_V8_TAG);
        expect(contractStateTag(state)).toBe(CONTRACT_STATE_V8_TAG);
        expect(state).toBe(nodeState);
      });

      /**
       * The contract of a contract untouched since the fork serves the state the node serves.
       *
       * @given a contract deployed and last called before the fork
       * @when the contract is queried by address without an offset
       * @then its state starts with midnight:contract-state[v8] and matches the node's state
       */
      test('should serve the contract state in the post-fork encoding of the node', async (ctx: TestContext) => {
        ctx.task!.meta.custom = { labels: ['Query', 'Contract', 'HardFork'] };

        const nodeState = await nodeClient.getContractState(address);
        const response = await indexerHttpClient.getContract(address);

        expect(response).toBeSuccess();
        const state = response.data!.contract!.state;
        expect(contractStateTag(nodeState), 'node state tag').toBe(CONTRACT_STATE_V8_TAG);
        expect(contractStateTag(state)).toBe(CONTRACT_STATE_V8_TAG);
        expect(state).toBe(nodeState);
      });

      /**
       * The translated state is anchored at the fork block, while the contract action keeps the
       * transaction that last wrote the contract.
       *
       * @given a contract deployed and last called before the fork
       * @when its latest contract action and the contract are queried by address without an offset
       * @then both stateAt name the fork block (169,496, protocol version 2001000)
       * @and the contract action keeps its original transaction, from block 124,423
       */
      test('should anchor the translated state at the fork block', async (ctx: TestContext) => {
        ctx.task!.meta.custom = { labels: ['Query', 'ContractAction', 'Contract', 'HardFork'] };
        const latest = actions.at(-1)!;

        const [actionResponse, contractResponse] = await Promise.all([
          indexerHttpClient.getContractAction(address, undefined, GET_CONTRACT_ACTION_STATE_AT),
          indexerHttpClient.getContract(address, {}, GET_CONTRACT_STATE_AT),
        ]);

        expect(actionResponse).toBeSuccess();
        const action = ContractActionStateAtSchema.parse(actionResponse.data?.contractAction);
        expect(action.stateAt).toEqual(forkBlock(fork));
        expect(action.transaction.hash).toBe(latest['transaction-hash']);
        expect(action.transaction.block.height).toBe(latest['block-height']);
        expect(contractResponse).toBeSuccess();
        expect(ContractStateAtSchema.parse(contractResponse.data?.contract).stateAt).toEqual(
          forkBlock(fork),
        );
      });
    });

    describe('a contract untouched since the fork queried as of the blocks around it', () => {
      /**
       * As-of views serve the encoding of the queried block: the pre-fork encoding the block
       * before the fork, the post-fork encoding from the fork block on.
       *
       * @given a contract deployed and last called before the fork
       * @when the contract is queried as of the block before the fork and as of the fork block
       * @then the states match the node's at those blocks: [v6] at 169,495, [v8] at 169,496
       */
      test('should serve the state in the encoding of the queried block', async (ctx: TestContext) => {
        ctx.task!.meta.custom = { labels: ['Query', 'Contract', 'ByHeight', 'HardFork'] };
        const forkHeight = fork['fork-block'].height;

        for (const [height, tag] of [
          [forkHeight - 1, CONTRACT_STATE_V6_TAG],
          [forkHeight, CONTRACT_STATE_V8_TAG],
        ] as const) {
          const nodeState = await nodeStateAt(height, tag);
          const response = await indexerHttpClient.getContract(address, { offset: { height } });

          expect(response).toBeSuccess();
          const state = response.data!.contract!.state;
          expect(contractStateTag(state), `state tag as of height ${height}`).toBe(tag);
          expect(state, `state as of height ${height}`).toBe(nodeState);
        }
      });

      /**
       * As-of views anchor the state at the latest action before the fork and at the fork block
       * from it on.
       *
       * @given a contract deployed and last called before the fork
       * @when the contract is queried as of the block before the fork and as of the fork block
       * @then stateAt names the latest action's block (124,423) at 169,495 and the fork block at 169,496
       */
      test('should anchor the state at the latest action before the fork and at the fork block after', async (ctx: TestContext) => {
        ctx.task!.meta.custom = {
          labels: ['Query', 'Contract', 'ByHeight', 'StateAt', 'HardFork'],
        };
        const forkHeight = fork['fork-block'].height;
        const latest = actions.at(-1)!;

        const [before, at] = await Promise.all(
          [forkHeight - 1, forkHeight].map((height) =>
            indexerHttpClient.getContract(address, { offset: { height } }, GET_CONTRACT_STATE_AT),
          ),
        );

        expect(before).toBeSuccess();
        expect(ContractStateAtSchema.parse(before.data?.contract).stateAt).toMatchObject({
          height: latest['block-height'],
          hash: latest['block-hash'],
        });
        expect(at).toBeSuccess();
        expect(ContractStateAtSchema.parse(at.data?.contract).stateAt).toEqual(forkBlock(fork));
      });
    });

    describe('the recorded contract actions of a contract untouched since the fork', () => {
      /**
       * Records keep the state their own transaction wrote, in the pre-fork encoding.
       *
       * @given a contract with three actions before the fork (deploy, then two calls)
       * @when each action is read through its transaction and through a block-offset query
       * @then every state starts with midnight:contract-state[v6]
       */
      test('should keep every recorded contract action in the pre-fork encoding', async (ctx: TestContext) => {
        ctx.task!.meta.custom = { labels: ['Query', 'ContractAction', 'Transaction', 'HardFork'] };

        for (const recorded of actions) {
          const txResponse = await indexerHttpClient.getTransactionByOffset({
            hash: recorded['transaction-hash'],
          });
          const byBlock = await indexerHttpClient.getContractAction(address, {
            blockOffset: { height: recorded['block-height'] },
          });

          expect(txResponse).toBeSuccess();
          const txAction = txResponse
            .data!.transactions.flatMap((tx) => tx.contractActions ?? [])
            .find((action) => action.address === address);
          expect(txAction, `contract action in tx ${recorded['transaction-hash']}`).toBeDefined();
          expect(contractStateTag(txAction!.state)).toBe(CONTRACT_STATE_V6_TAG);
          expect(byBlock).toBeSuccess();
          expect(contractStateTag(byBlock.data!.contractAction!.state)).toBe(CONTRACT_STATE_V6_TAG);
        }
      });

      /**
       * Contract.actions lists the recorded actions, newest first, each anchored at its own block.
       *
       * @given a contract with three actions before the fork and none since
       * @when the contract's actions are listed through Contract.actions
       * @then exactly those three actions are listed, newest first, each in the [v6] encoding
       * @and each stateAt names the block of the action's own transaction
       */
      test('should list the recorded actions anchored at their own blocks', async (ctx: TestContext) => {
        ctx.task!.meta.custom = { labels: ['Query', 'Contract', 'StateAt', 'HardFork'] };

        const response = await indexerHttpClient.getContract(address, {}, GET_CONTRACT_STATE_AT);

        expect(response).toBeSuccess();
        const listed = ContractStateAtSchema.parse(response.data?.contract).actions;
        expect(
          listed.map((action) => action.transaction.hash),
          'contract touched since the fork? update ledger-hard-fork.jsonc',
        ).toEqual(actions.map((action) => action['transaction-hash']).reverse());
        for (const action of listed) {
          expect(contractStateTag(action.state)).toBe(CONTRACT_STATE_V6_TAG);
          expect(action.stateAt).toEqual(ownBlock(action));
        }
      });
    });
  },
);
