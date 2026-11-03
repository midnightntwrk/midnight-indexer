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
import { MothTxBackend } from './moth-tx-backend';
import { ToolkitTxBackend } from './toolkit-tx-backend';
import type { TxBackend, TxBackendName } from './tx-backend';

export type { DerivedAddresses, TransactionResult, TxBackend, TxBackendName } from './tx-backend';
export { MothTxBackend } from './moth-tx-backend';
export { ToolkitTxBackend } from './toolkit-tx-backend';
export { NIGHT_TOKEN_TYPE, pickSpendableCustomToken } from './holdings';

/** A new, unstarted backend of the kind `TX_BACKEND` selects (default: toolkit). */
export function createTxBackend(): TxBackend {
  return env.getTxBackend() === 'moth' ? new MothTxBackend() : new ToolkitTxBackend();
}

/**
 * Gate a suite that only one backend can run.
 *
 * @example
 *   const gate = requiresBackend('toolkit');
 *   describe.skipIf(gate.skip)(`crafted duplicate deploys ${gate.label}`, () => { ... });
 *
 * @returns `skip` when `TX_BACKEND` selects the other backend, and a `label` for the
 *          suite title so the report says why it skipped.
 */
export function requiresBackend(required: TxBackendName): { skip: boolean; label: string } {
  const selected = env.getTxBackend();
  return {
    skip: selected !== required,
    label: `[TX_BACKEND=${required} only${selected === required ? '' : `, running ${selected}`}]`,
  };
}
