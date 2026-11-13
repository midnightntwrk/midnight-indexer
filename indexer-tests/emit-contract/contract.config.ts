// This file is part of midnight-indexer.
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
import { CompiledContract, ContractExecutable } from '@midnight-ntwrk/compact-js/effect';
import { Contract as C_ } from './out/contract/index.js';

type EmitCounterContract = C_<{}>;
const EmitCounterContract = C_;

export default {
  contractExecutable: CompiledContract.make<EmitCounterContract>('EmitCounterContract', EmitCounterContract).pipe(
    CompiledContract.withVacantWitnesses,
    CompiledContract.withCompiledFileAssets('./out'),
    ContractExecutable.make
  ),
  createInitialPrivateState: () => ({}),
  config: {
    keys: {
      coinPublic: 'aa0d72bb77ea46f986a800c66d75c4e428a95bd7e1244f1ed059374e6266eb98',
    },
    network: 'undeployed'
  }
}
