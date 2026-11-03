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

import { env } from '../../environment/model';
import { getContractDeploymentHashes, resolveBlockHash } from '../../tests/e2e/test-utils';
import type {
  AddressType,
  DeployContractResult,
  DustBalance,
  PrivateWalletState,
  PublicWalletState,
} from '../toolkit/toolkit-wrapper';
import { deriveMothAddresses, deriveMothViewingKey } from '../moth/moth-addresses';
import {
  closeMothWallets,
  generateSingleTxViaMoth,
  getDustBalanceViaMoth,
  listUnshieldedHoldingsViaMoth,
  showPrivateWalletStateViaMoth,
  showPublicWalletStateViaMoth,
} from '../moth/moth-backend';
import {
  callCircuitViaMoth,
  closeMothContractContexts,
  deployContractViaMoth,
  updateContractViaMoth,
} from '../moth/moth-contracts';
import type { DerivedAddresses, TransactionResult, TxBackend } from './tx-backend';

/**
 * {@link TxBackend} over moth-wallet, in-process: transfers and wallet reads go
 * through moth's synced wallets, contract actions through midnight-js with moth
 * as the wallet, and key material is derived locally.
 */
export class MothTxBackend implements TxBackend {
  readonly name = 'moth' as const;

  async start(): Promise<void> {}

  async stop(): Promise<void> {
    // The midnight-js providers wrap the wallet facades, so they go first.
    closeMothContractContexts();
    await closeMothWallets();
  }

  async addresses(seed: string, networkId?: string): Promise<DerivedAddresses> {
    return deriveMothAddresses(seed, networkId ?? env.getNetworkId());
  }

  async viewingKey(seed: string, networkId?: string): Promise<string> {
    return deriveMothViewingKey(seed, networkId ?? env.getNetworkId());
  }

  transfer(
    seed: string,
    addressType: AddressType,
    destinationAddress: string,
    amount: number,
    tokenType?: string,
  ): Promise<TransactionResult> {
    return generateSingleTxViaMoth(seed, addressType, destinationAddress, amount, tokenType);
  }

  // The contract is the counter committed under qa/tests/contracts, not the
  // toolkit's built-in one. midnight-js returns neither a tagged address nor the
  // deployer's coin public key, and no assertion reads them, so they stay empty.
  async deployContract(seed: string): Promise<DeployContractResult> {
    const { contractAddress } = await deployContractViaMoth(seed);
    const { txHash, blockHash } = await getContractDeploymentHashes(contractAddress);
    return {
      'contract-address-untagged': contractAddress,
      'contract-address-tagged': '',
      'coin-public': '',
      'deploy-tx-hash': txHash,
      'deploy-block-hash': blockHash,
    };
  }

  async callContract(
    deployment: DeployContractResult,
    seed: string,
    circuitId?: string,
  ): Promise<TransactionResult> {
    const txHash = await callCircuitViaMoth(
      seed,
      deployment['contract-address-untagged'],
      circuitId,
    );
    return withBlockHash(txHash);
  }

  // `seed` must be the deploying seed: the contract's signing key lives in that
  // seed's in-process private state. The new authority is a fresh sample key.
  async updateContract(deployment: DeployContractResult, seed: string): Promise<TransactionResult> {
    const txHash = await updateContractViaMoth(seed, deployment['contract-address-untagged']);
    return withBlockHash(txHash);
  }

  privateWalletState(seed: string): Promise<PrivateWalletState> {
    return showPrivateWalletStateViaMoth(seed);
  }

  publicWalletState(seed: string, address: string): Promise<PublicWalletState> {
    return showPublicWalletStateViaMoth(seed, address);
  }

  dustBalance(seed: string): Promise<DustBalance> {
    return getDustBalanceViaMoth(seed);
  }

  unshieldedHoldings(seed: string): Promise<Map<string, bigint>> {
    return listUnshieldedHoldingsViaMoth(seed);
  }
}

/** midnight-js reports the transaction it sent, not its block; the indexer supplies that. */
async function withBlockHash(txHash: string): Promise<TransactionResult> {
  const result: TransactionResult = {
    txHash,
    blockHash: '',
    status: 'sent',
    rawOutput: `moth/midnight-js submitted ${txHash}`,
  };
  await resolveBlockHash(result);
  return result;
}
