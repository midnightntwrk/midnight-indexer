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

import {
  ToolkitWrapper,
  type AddressType,
  type DeployContractResult,
  type DustBalance,
  type PrivateWalletState,
  type PublicWalletState,
} from '../toolkit/toolkit-wrapper';
import type { DerivedAddresses, TransactionResult, TxBackend } from './tx-backend';

/** {@link TxBackend} over the `midnight-node-toolkit` container. */
export class ToolkitTxBackend implements TxBackend {
  readonly name = 'toolkit' as const;

  constructor(readonly toolkit: ToolkitWrapper = new ToolkitWrapper({})) {}

  start(): Promise<void> {
    return this.toolkit.start();
  }

  stop(): Promise<void> {
    return this.toolkit.stop();
  }

  async addresses(seed: string, networkId?: string): Promise<DerivedAddresses> {
    const { unshielded, shielded, dust } = await this.toolkit.showAddress(seed, networkId);
    return { unshielded, shielded, dust };
  }

  viewingKey(seed: string, networkId?: string): Promise<string> {
    return this.toolkit.showViewingKey(seed, networkId);
  }

  transfer(
    seed: string,
    addressType: AddressType,
    destinationAddress: string,
    amount: number,
    tokenType?: string,
  ): Promise<TransactionResult> {
    return this.toolkit.generateSingleTx(seed, addressType, destinationAddress, amount, tokenType);
  }

  deployContract(seed: string): Promise<DeployContractResult> {
    return this.toolkit.deployContract(seed);
  }

  callContract(
    deployment: DeployContractResult,
    seed: string,
    circuitId?: string,
  ): Promise<TransactionResult> {
    return this.toolkit.callContract(circuitId, deployment, undefined, seed);
  }

  updateContract(deployment: DeployContractResult, seed: string): Promise<TransactionResult> {
    return this.toolkit.updateContract(deployment, seed);
  }

  privateWalletState(seed: string): Promise<PrivateWalletState> {
    return this.toolkit.showPrivateWalletState(seed);
  }

  publicWalletState(_seed: string, address: string): Promise<PublicWalletState> {
    return this.toolkit.showPublicWalletState(address);
  }

  dustBalance(seed: string): Promise<DustBalance> {
    return this.toolkit.getDustBalance(seed);
  }

  // The toolkit reports UTXO token types under `token_type`, in the same hex
  // encoding the indexer reports as `tokenType`.
  async unshieldedHoldings(_seed: string, address: string): Promise<Map<string, bigint>> {
    const { utxos } = await this.toolkit.showPublicWalletState(address);
    return utxos.reduce(
      (totals, utxo) =>
        totals.set(utxo.token_type, (totals.get(utxo.token_type) ?? 0n) + BigInt(utxo.value)),
      new Map<string, bigint>(),
    );
  }
}
