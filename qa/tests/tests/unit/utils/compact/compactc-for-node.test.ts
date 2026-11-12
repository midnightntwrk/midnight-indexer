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

// The e2e fixtures take their compactc from generate_node_data.sh's own
// node-line rule, read from the script. A wrong pick does not fail loudly later:
// the toolkit rejects the compiled contract and the suite skips. So these tests
// check the reader against bash itself, for every node line in NODE_VERSIONS, and
// check that a reworded script stops the reader instead of misleading it.

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { compactcForNode } from '@utils/compact/compact-compiler';

vi.mock('@utils/logging/logger', () => ({
  default: { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() },
}));
// Importing the module resolves the default pin for the node under test; the
// tests below call the reader directly, so any node line will do here.
vi.mock('../../../../environment/model', () => ({
  env: { getNodeVersion: () => '1.0.300' },
}));

const REPO_ROOT = path.resolve(__dirname, '../../../../../..');
const SCRIPT = path.join(REPO_ROOT, 'generate_node_data.sh');

const NODE_VERSIONS = fs
  .readFileSync(path.join(REPO_ROOT, 'NODE_VERSIONS'), 'utf8')
  .split('\n')
  .map((line) => line.trim())
  .filter(Boolean);

/** What bash itself makes of the script's `case` block for a node tag. */
function compactcByBash(nodeTag: string): string {
  const block = /case "\$node_version" in\n[\s\S]*?\n\s*esac/.exec(
    fs.readFileSync(SCRIPT, 'utf8'),
  )![0];
  return execFileSync(
    'bash',
    ['-c', `node_version="$1"\n${block}\necho "$default_compactc_version"`, '-', nodeTag],
    {
      encoding: 'utf8',
    },
  ).trim();
}

/** A throwaway script holding the given text. */
function scriptWith(text: string): string {
  const file = path.join(
    fs.mkdtempSync(path.join(os.tmpdir(), 'compactc-')),
    'generate_node_data.sh',
  );
  fs.writeFileSync(file, text);
  return file;
}

describe('compactcForNode', () => {
  describe('with generate_node_data.sh', () => {
    it.each([
      ['1.0.300', '0.30.0'],
      ['2.0.0-rc.4', '0.30.0'],
      ['2.1.0-rc.4', '0.33.0-rc.2'],
    ])('should pick compactc %s for node %s', (node, compactc) => {
      expect(compactcForNode(node)).toBe(compactc);
    });

    it.each(NODE_VERSIONS)('should agree with bash for node %s', (node) => {
      expect(compactcForNode(node)).toBe(compactcByBash(node));
    });
  });

  describe('with a given script', () => {
    it('should take the first matching arm, including alternatives', () => {
      const script = scriptWith(
        [
          'case "$node_version" in',
          '    2.1.*|2.2.*) default_compactc_version="0.33.0-rc.2" ;;',
          '    2.*) default_compactc_version="0.31.0" ;;',
          'esac',
        ].join('\n'),
      );

      expect(compactcForNode('2.2.0', script)).toBe('0.33.0-rc.2');
      expect(compactcForNode('2.0.0', script)).toBe('0.31.0');
      expect(() => compactcForNode('1.0.0', script)).toThrow(/No compactc for node 1\.0\.0/);
    });

    it('should treat a dot as a literal, not any character', () => {
      const script = scriptWith(
        [
          'case "$node_version" in',
          '    2.1.*) default_compactc_version="0.33.0-rc.2" ;;',
          'esac',
        ].join('\n'),
      );

      expect(() => compactcForNode('2x1x0', script)).toThrow();
    });

    it('should refuse a script without the case block', () => {
      const script = scriptWith('readonly compactc_version="${COMPACTC_VERSION:-0.30.0}"\n');

      expect(() => compactcForNode('1.0.300', script)).toThrow(
        /No 'case "\$node_version" in' block/,
      );
    });
  });
});
