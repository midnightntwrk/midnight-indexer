// This file is part of midnight-indexer.
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

import { CompiledContract, ContractExecutable } from '@midnight-ntwrk/compact-js/effect';
import { Contract as C_ } from './managed/zswap-holder/contract/index.js';

type ZswapHolderContract = C_<{}>;
const ZswapHolderContract = C_;

export default {
  contractExecutable: CompiledContract.make<ZswapHolderContract>('ZswapHolderContract', ZswapHolderContract).pipe(
    CompiledContract.withVacantWitnesses,
    CompiledContract.withCompiledFileAssets('./managed/zswap-holder'),
    ContractExecutable.make
  ),
  createInitialPrivateState: () => ({}),
  config: {
    keys: {
      coinPublic: '1bd4f827be97ff013c4a702e4b08f30ec378728a54670cf7cc92cb9b1a14eff6',
    },
    network: 'undeployed'
  }
}
