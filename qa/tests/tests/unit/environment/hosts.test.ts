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

import { resolveHosts } from 'environment/hosts';

describe('resolveHosts', () => {
  test('uses the built-in hosts of the local stack', () => {
    expect(resolveHosts('undeployed')).toEqual({
      indexerHost: 'localhost:8088',
      nodeHost: 'localhost:9944',
    });
  });

  test('each override replaces only its own built-in host', () => {
    expect(resolveHosts('undeployed', { indexerHost: 'indexer.example.org' })).toEqual({
      indexerHost: 'indexer.example.org',
      nodeHost: 'localhost:9944',
    });
    expect(resolveHosts('undeployed', { nodeHost: 'rpc.example.org' })).toEqual({
      indexerHost: 'localhost:8088',
      nodeHost: 'rpc.example.org',
    });
  });

  test('an environment without built-in hosts resolves from both overrides', () => {
    expect(
      resolveHosts('qanet', { indexerHost: ' indexer.example.org ', nodeHost: 'rpc.example.org' }),
    ).toEqual({ indexerHost: 'indexer.example.org', nodeHost: 'rpc.example.org' });
  });

  test.each(['devnet', 'qanet', 'stagenet', 'preview', 'preprod', 'mainnet'])(
    '%s has no built-in hosts and names both missing variables',
    (envName) => {
      expect(() => resolveHosts(envName)).toThrow(
        `TARGET_ENV="${envName}" has no built-in hosts. Set INDEXER_HOST and NODE_HOST`,
      );
    },
  );

  test('names only the variable that is still missing', () => {
    expect(() => resolveHosts('qanet', { indexerHost: 'indexer.example.org' })).toThrow(
      'Set NODE_HOST to',
    );
    expect(() => resolveHosts('qanet', { nodeHost: 'rpc.example.org', indexerHost: '  ' })).toThrow(
      'Set INDEXER_HOST to',
    );
  });

  test.each(['https://indexer.example.org', 'indexer.example.org/api/v4'])(
    'rejects an override that is not a bare host: %s',
    (value) => {
      expect(() => resolveHosts('undeployed', { indexerHost: value })).toThrow(
        'Invalid INDEXER_HOST',
      );
    },
  );
});
