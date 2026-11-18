// This file is part of midnightntwrk/midnight-indexer.
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

import { NodeRpcClient } from '@utils/node/rpc-client';
import { env } from 'environment/model';

const PRE_FUND_WALLETS = 4;
const PRE_FUND_WALLETS_UNDEPLOYED_NODE_3 = 20;

/**
 * The number of distinct wallets the genesis block funds with unshielded NIGHT.
 *
 * An undeployed chain is created fresh from the genesis the node image ships
 * (`/res/genesis/genesis_block_undeployed.mn`), so the count follows the node line: node 3.x
 * funds the 20 seeds of `/res/dev/undeployed-genesis-seeds.json`, older nodes fund 4.
 * A deployed chain keeps the genesis it was created with whatever node runs it now.
 */
export function preFundWalletCount(isUndeployed: boolean, nodeVersion: string): number {
  const major = Number.parseInt(nodeVersion.trim().replace(/^v/, ''), 10);
  if (Number.isNaN(major)) {
    throw new Error(`Cannot read a major version from node version "${nodeVersion}"`);
  }
  return isUndeployed && major >= 3 ? PRE_FUND_WALLETS_UNDEPLOYED_NODE_3 : PRE_FUND_WALLETS;
}

/**
 * The pre-fund wallet count expected on the environment under test.
 *
 * The node version is read from the running node rather than NODE_TAG: the harness reuses a
 * stack that is already up, in which case NODE_TAG does not describe the chain being queried.
 */
export async function expectedPreFundWalletCount(): Promise<number> {
  if (!env.isUndeployedEnv()) return PRE_FUND_WALLETS;
  return preFundWalletCount(true, await new NodeRpcClient().getSystemVersion());
}
