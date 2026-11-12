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

import type { BlockReference, ContractActionStateAt } from '@utils/indexer/graphql/schema';
import type { LedgerHardForkInfo } from '@utils/testdata-provider';
import type { NodeRpcClient } from '@utils/node/rpc-client';

// Tagged-serialization prefixes of a contract state in the ledger 8 and ledger 9 encodings.
// Fork tests also assert the node's states carry them, so a renamed tag fails loudly there.
export const CONTRACT_STATE_V6_TAG = 'midnight:contract-state[v6]:';
export const CONTRACT_STATE_V8_TAG = 'midnight:contract-state[v8]:';

/** The ASCII serialization tag a hex-encoded contract state starts with, up to its second `:`. */
export function contractStateTag(stateHex: string): string {
  const text = Buffer.from(stateHex.slice(0, 128), 'hex').toString('latin1');
  return text.slice(0, text.indexOf(':', 'midnight:'.length) + 1);
}

/** The block of a contract action's own transaction. */
export function ownBlock(action: ContractActionStateAt): BlockReference {
  const { block, protocolVersion } = action.transaction;
  return { height: block.height, hash: block.hash, protocolVersion };
}

/** Whether two block references name the same block on the same protocol version. */
export function sameBlock(a: BlockReference, b: BlockReference): boolean {
  return a.height === b.height && a.hash === b.hash && a.protocolVersion === b.protocolVersion;
}

/**
 * Checks `state`, anchored at `stateAt`, against the node, which the indexer under test cannot
 * influence, so the check holds on every environment, whether or not it declares a hard fork:
 * - `stateAt` is the node's block at that height and the state is the node's state there;
 * - an anchor other than the action's own block is a later block on a newer protocol version
 *   where the node's state changed without an action: a translation at a hard fork, not any
 *   block the state merely still holds at.
 */
export async function expectAnchorConfirmedByNode(
  node: NodeRpcClient,
  address: string,
  state: string,
  stateAt: BlockReference,
  action: ContractActionStateAt,
): Promise<void> {
  const label = `state anchored at ${stateAt.height}`;
  expect(strip0x(await node.getBlockHash(stateAt.height)), `${label}: node block hash`).toBe(
    stateAt.hash,
  );
  expect(state, `${label}: node state there`).toBe(
    await node.getContractState(address, stateAt.hash),
  );
  const own = ownBlock(action);
  if (sameBlock(stateAt, own)) return;
  expect(stateAt.height, `${label}: after the action's block ${own.height}`).toBeGreaterThan(
    own.height,
  );
  expect(stateAt.protocolVersion, `${label}: protocol version`).not.toBe(own.protocolVersion);
  const parentHash = await node.getBlockHash(stateAt.height - 1);
  expect(
    await node.getContractState(address, parentHash!),
    `${label}: node state must change there`,
  ).not.toBe(state);
}

const strip0x = (hex: string | null) => hex?.replace(/^0x/, '') ?? null;

export function forkBlock(fork: LedgerHardForkInfo): BlockReference {
  const { height, hash, 'protocol-version': protocolVersion } = fork['fork-block'];
  return { height, hash, protocolVersion };
}

/**
 * Where the current state of a contract whose latest action is `latest` is anchored: the fork
 * block if the action predates the chain's hard fork (its state was translated there), else the
 * action's own block.
 */
export function currentStateAt(
  latest: ContractActionStateAt,
  fork: LedgerHardForkInfo | null,
): BlockReference {
  return fork && latest.transaction.block.height < fork['fork-block'].height
    ? forkBlock(fork)
    : ownBlock(latest);
}
