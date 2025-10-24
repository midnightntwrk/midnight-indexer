// This file is part of midnightntwrk/midnight-indexer
// Copyright (C) 2025 Midnight Foundation
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

import { z } from 'zod';

export const Hash64 = z
  .string()
  .length(64)
  .regex(/^[a-f0-9]+$/);
export const VarLenghtHex = z.string().regex(/^[a-f0-9]+$/);
export const BlockHeight = z.number().min(0);

export const PartialBlockSchema = z.lazy(() =>
  z.object({
    hash: Hash64,
    height: BlockHeight,
  }),
);

export const BlockSchema = z.lazy(() =>
  z.object({
    hash: Hash64,
    height: BlockHeight,
    timestamp: z.number(),
    parent: PartialBlockSchema,
    transactions: z.array(FullTransactionSchema).min(0),
  }),
);

// Base transaction schema (common to both RegularTransaction and SystemTransaction)
const BaseTransactionFields = {
  id: z.number(),
  hash: Hash64,
  protocolVersion: z.number(),
  raw: VarLenghtHex,
  block: PartialBlockSchema,
  contractActions: z.array(z.any()), // Will be validated separately
  unshieldedCreatedOutputs: z.array(z.any()), // Will be validated separately
  unshieldedSpentOutputs: z.array(z.any()), // Will be validated separately
  zswapLedgerEvents: z.array(
    z.object({
      id: z.number(),
      raw: z.string(),
      maxId: z.number(),
    }),
  ),
  dustLedgerEvents: z.array(
    z.object({
      id: z.number(),
      raw: z.string(),
      maxId: z.number(),
    }),
  ),
};

// RegularTransaction schema (includes additional fields)
export const RegularTransactionSchema = z.lazy(() =>
  z.object({
    ...BaseTransactionFields,
    merkleTreeRoot: z.string().regex(/^[a-f0-9]+$/),
    identifiers: z.array(z.string()),
    startIndex: z.number(),
    endIndex: z.number(),
    fees: z.object({
      paidFees: z.string(),
      estimatedFees: z.string(),
    }),
    transactionResult: z.object({
      status: z.enum(['SUCCESS', 'PARTIAL_SUCCESS', 'FAILURE']),
      segments: z.array(
        z.object({
          id: z.number(),
          success: z.boolean(),
        }),
      ),
    }),
  }),
);

// SystemTransaction schema (only base fields)
export const SystemTransactionSchema = z.lazy(() => z.object(BaseTransactionFields));

// Union schema for both transaction types
export const FullTransactionSchema = z.union([RegularTransactionSchema, SystemTransactionSchema]);

// Contract related schema validation
const BaseActionSchema = z.object({
  id: z.string(),
  type: z.enum(['CALL', 'DEPLOY', 'UPDATE']),
  timestamp: z.string(),
});

const ContractCallSchema = BaseActionSchema.extend({
  type: z.literal('CALL'),
  method: z.string(),
  args: z.array(z.string()),
});

const ContractDeploySchema = BaseActionSchema.extend({
  type: z.literal('DEPLOY'),
  code: z.string(),
});

const ContractUpdateSchema = BaseActionSchema.extend({
  type: z.literal('UPDATE'),
  patch: z.string(),
});

export const ContractActionSchema = z.discriminatedUnion('type', [
  ContractCallSchema,
  ContractDeploySchema,
  ContractUpdateSchema,
]);

// Ledger event schemas
export const ZswapLedgerEventSchema = z.object({
  id: z.number(),
  raw: z.string(),
  maxId: z.number(),
});

export const DustLedgerEventSchema = z.object({
  id: z.number(),
  raw: z.string(),
  maxId: z.number(),
});

// Contract balance schema
export const ContractBalanceSchema = z.object({
  tokenType: z.string(),
  amount: z.string(),
});

// Updated contract action schemas to match current API
export const ContractDeployActionSchema = z.object({
  __typename: z.literal('ContractDeploy'),
  address: z.string(),
  state: z.string(),
  zswapState: z.string(),
  transaction: z.any(), // Reference to transaction
  unshieldedBalances: z.array(ContractBalanceSchema),
});

export const ContractCallActionSchema = z.object({
  __typename: z.literal('ContractCall'),
  address: z.string(),
  state: z.string(),
  zswapState: z.string(),
  entryPoint: z.string(),
  transaction: z.any(), // Reference to transaction
  deploy: z.any(), // Reference to deploy
  unshieldedBalances: z.array(ContractBalanceSchema),
});

export const ContractUpdateActionSchema = z.object({
  __typename: z.literal('ContractUpdate'),
  address: z.string(),
  state: z.string(),
  zswapState: z.string(),
  transaction: z.any(), // Reference to transaction
  unshieldedBalances: z.array(ContractBalanceSchema),
});

export const ContractActionUnionSchema = z.discriminatedUnion('__typename', [
  ContractDeployActionSchema,
  ContractCallActionSchema,
  ContractUpdateActionSchema,
]);
