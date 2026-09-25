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

import log from '@utils/logging/logger';
import '@utils/logging/test-logging-hooks';
import type { TestContext } from 'vitest';
import { env } from 'environment/model';
import {
  introspectTypeFields,
  isProgressProtocolVersionSupported,
} from '@utils/indexer/schema-feature-probe';

/**
 * Contract guard for the progress `protocolVersion` field (midnight-indexer#1463).
 *
 * A wallet with nothing to receive learns about a protocol upgrade only from the
 * progress update of the subscription it holds open, so every progress type has to
 * carry the field, as a non-null Int. Deployed builds are not homogeneous, so the
 * whole file gates on the field being served at all and names the environment
 * lacking it rather than failing there; what it then asserts is that an environment
 * carrying the change carries it on all three types, with the right type.
 */

const PROGRESS_TYPES = [
  'UnshieldedTransactionsProgress',
  'ShieldedTransactionsProgress',
  'DustGenerationsProgress',
];

const FIELD_NAME = 'protocolVersion';

// Enough of the type reference to tell `Int!` from `Int`, `String!` or a list.
const TYPE_SELECTION = 'type { kind name ofType { kind name } }';

describe('progress protocol version schema', () => {
  let supported = false;

  beforeAll(async () => {
    supported = await isProgressProtocolVersionSupported();
    if (!supported) {
      log.warn(
        `progress ${FIELD_NAME} is absent on ${env.getCurrentEnvironmentName()}; ` +
          'skipping the whole surface (midnight-indexer#1463)',
      );
    }
  }, 30_000);

  for (const typeName of PROGRESS_TYPES) {
    describe(`${typeName}.${FIELD_NAME}`, () => {
      /**
       * The progress type serves protocolVersion, typed so a consumer can rely on
       * always receiving a number.
       *
       * @given a deployed indexer GraphQL endpoint that serves the progress
       *        protocolVersion field
       * @when the progress type is introspected
       * @then it exposes protocolVersion as a non-null Int
       */
      test('should be served as a non-null Int', async (ctx: TestContext) => {
        ctx.task!.meta.custom = { labels: ['SchemaValidation', 'Subscription', 'Progress'] };
        if (!supported) {
          return ctx.skip(
            true,
            `progress ${FIELD_NAME} is absent on ${env.getCurrentEnvironmentName()} ` +
              '(midnight-indexer#1463)',
          );
        }

        log.debug(`Introspecting ${typeName} for field ${FIELD_NAME}`);
        const fields = await introspectTypeFields(typeName, TYPE_SELECTION);
        if (fields === null) {
          return ctx.skip(
            true,
            `type "${typeName}" is not served by ${env.getCurrentEnvironmentName()}`,
          );
        }

        const field = fields.find((f) => f.name === FIELD_NAME);
        expect(field, `field "${typeName}.${FIELD_NAME}" not found in schema`).toBeDefined();
        expect(field!.type?.kind, `${typeName}.${FIELD_NAME} should be non-null`).toBe('NON_NULL');
        expect(field!.type?.ofType?.name, `${typeName}.${FIELD_NAME} should be an Int`).toBe('Int');
      });
    });
  }
});
