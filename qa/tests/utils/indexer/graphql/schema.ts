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
    protocolVersion: z.number(),
    author: z.string().optional(),
    ledgerParameters: z.string(),
    parent: PartialBlockSchema,
    transactions: z.array(FullTransactionSchema).min(0),
  }),
);

export const UnshieldedUtxoSchema = z.object({
  owner: z.string(),
  intentHash: Hash64,
  value: z.string(),
  tokenType: z
    .string()
    .length(64)
    .regex(/^[a-f0-9]+$/),
  outputIndex: z.number(),
  ctime: z.number().nullable(),
  initialNonce: z.string(),
  registeredForDustGeneration: z.boolean(),
  createdAtTransaction: z.object({
    hash: Hash64,
    identifiers: z.array(z.string()).optional(),
  }),
  spentAtTransaction: z
    .object({
      hash: Hash64,
      identifiers: z.array(z.string()).optional(),
    })
    .nullable(),
});

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

// Base transaction schema (common to both RegularTransaction and SystemTransaction)
const BaseTransactionFields = {
  id: z.number(),
  hash: Hash64,
  protocolVersion: z.number(),
  raw: VarLenghtHex,
  block: PartialBlockSchema,
  contractActions: z.array(z.any()), // Will be validated separately
  unshieldedCreatedOutputs: z.array(UnshieldedUtxoSchema),
  unshieldedSpentOutputs: z.array(z.any()), // Will be validated separately
  zswapLedgerEvents: z.array(ZswapLedgerEventSchema),
  dustLedgerEvents: z.array(DustLedgerEventSchema),
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
      segments: z
        .array(
          z.object({
            id: z.number(),
            success: z.boolean(),
          }),
        )
        .nullable(),
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

// Contract balance schema
export const ContractBalanceSchema = z.object({
  tokenType: z.string(),
  amount: z.string(),
});

// Updated contract action schemas to match current API
export const ContractDeployActionSchema = z.object({
  __typename: z.literal('ContractDeploy'),
  address: Hash64,
  state: VarLenghtHex,
  zswapState: VarLenghtHex,
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

// DUST Generation Status schema
export const DustGenerationStatusSchema = z.object({
  cardanoStakeKey: z.string().regex(/^[a-f0-9]{64}$/),
  dustAddress: z.string().nullable(),
  registered: z.boolean(),
  nightBalance: z.string().regex(/^\d+$/),
  generationRate: z.string().regex(/^\d+$/),
  currentCapacity: z.string().regex(/^\d+$/),
});

// Simplified version used in subscription responses
export const UnshieldedTxEventTransactionSchema = z.object({
  id: z.number(),
  hash: z.string().regex(/^[a-f0-9]+$/),
  identifiers: z.array(z.string()),
});

export const UnshieldedTxEventTransactionRefSchema = z.object({
  hash: z.string().regex(/^[a-f0-9]+$/),
  identifiers: z.array(z.string()),
});

export const UnshieldedTransactionEventSchema = z.object({
  __typename: z.literal('UnshieldedTransaction'),
  transaction: UnshieldedTxEventTransactionSchema,
  createdUtxos: z.array(UnshieldedUtxoSchema),
  spentUtxos: z.array(UnshieldedUtxoSchema),
});

export const UnshieldedTransactionsProgressSchema = z.object({
  __typename: z.literal('UnshieldedTransactionsProgress'),
  highestTransactionId: z.number(),
});

export const UnshieldedTxSubscriptionResponseSchema = z.union([
  UnshieldedTransactionEventSchema,
  UnshieldedTransactionsProgressSchema,
]);

export const RelevantTransactionSchema = z.object({
  __typename: z.literal('RelevantTransaction'),
  transaction: z.object({
    hash: Hash64,
  }),
  collapsedMerkleTree: z
    .object({
      startIndex: z.number(),
      endIndex: z.number(),
      update: VarLenghtHex,
      protocolVersion: z.number(),
    })
    .nullable(),
});

export const ShieldedTransactionsProgressSchema = z.object({
  __typename: z.literal('ShieldedTransactionsProgress'),
  highestEndIndex: z.number(),
  highestCheckedEndIndex: z.number(),
  highestRelevantEndIndex: z.number(),
});

export const ShieldedTransactionEventSchema = z.union([
  RelevantTransactionSchema,
  ShieldedTransactionsProgressSchema,
]);
