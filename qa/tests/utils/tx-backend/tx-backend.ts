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

import type {
  AddressType,
  DeployContractResult,
  ToolkitTransactionResult,
} from '../toolkit/toolkit-wrapper';
import type { DustBalance, PrivateWalletState, PublicWalletState } from '../toolkit/schemas';

/** The transaction backends `TX_BACKEND` can select. */
export type TxBackendName = 'toolkit' | 'moth';

/** A submitted transaction. Both backends report this shape. */
export type TransactionResult = ToolkitTransactionResult;

/** The bech32m addresses of one seed, which both backends derive identically. */
export interface DerivedAddresses {
  unshielded: string;
  shielded: string;
  dust: string;
}

/**
 * What the e2e and integration suites need from whichever wallet tool builds
 * and submits their transactions. Anything only one tool can do stays off this
 * interface: suites that need it use that tool directly and declare it with
 * `requiresBackend`.
 */
export interface TxBackend {
  readonly name: TxBackendName;

  /** Acquire what the backend needs (the toolkit container); a no-op for moth. */
  start(): Promise<void>;

  /** Release everything `start` and later calls acquired. Safe to call when unused. */
  stop(): Promise<void>;

  addresses(seed: string, networkId?: string): Promise<DerivedAddresses>;

  /** The shielded viewing key (`mn_shield-esk_…`) the indexer's wallet `connect` takes. */
  viewingKey(seed: string, networkId?: string): Promise<string>;

  /**
   * Build and submit one transfer.
   *
   * @param tokenType - 32-byte hex token type; omit for NIGHT.
   */
  transfer(
    seed: string,
    addressType: AddressType,
    destinationAddress: string,
    amount: number,
    tokenType?: string,
  ): Promise<TransactionResult>;

  /** Deploy the backend's counter contract, funded by `seed`. */
  deployContract(seed: string): Promise<DeployContractResult>;

  /** Call `circuitId` on a contract `deployContract` returned. */
  callContract(
    deployment: DeployContractResult,
    seed: string,
    circuitId?: string,
  ): Promise<TransactionResult>;

  /** Replace the maintenance authority of a contract `deployContract` returned. */
  updateContract(deployment: DeployContractResult, seed: string): Promise<TransactionResult>;

  privateWalletState(seed: string): Promise<PrivateWalletState>;

  /** The public state of `address`, which `seed` must own. */
  publicWalletState(seed: string, address: string): Promise<PublicWalletState>;

  dustBalance(seed: string): Promise<DustBalance>;

  /** Spendable unshielded value per token type (hex) held at `address`, owned by `seed`. */
  unshieldedHoldings(seed: string, address: string): Promise<Map<string, bigint>>;
}
