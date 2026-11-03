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

// The moth backend derives addresses and viewing keys locally instead of asking
// a toolkit container. These fixtures are `midnight-node-toolkit:2.1.0-rc.4`
// `show-address` / `show-viewing-key` output for public test seeds, so any drift
// between the two backends' key material fails here rather than in an e2e run.

import { deriveMothAddresses, deriveMothViewingKey } from '@utils/moth/moth-addresses';

const TOOLKIT_OUTPUT = [
  {
    seed: '0000000000000000000000000000000000000000000000000000000000000001',
    network: 'undeployed',
    addresses: {
      unshielded: 'mn_addr_undeployed1h3ssm5ru2t6eqy4g3she78zlxn96e36ms6pq996aduvmateh9p9sk96u7s',
      shielded:
        'mn_shield-addr_undeployed1r020sfa7jllsz0z2wqhykz8npmphsu5223nsea7vjt9ekxs5almtvtnrpgpszud4uyd0yjrlqyp7v5xvwqljsng2g79j5w4al9c4kuqy0xtw4',
      dust: 'mn_dust_undeployed1w0l54txthpu8q05j9j9ttk3j5dyu5766dc9zkz2s435vdglzwdr35dw790y',
    },
    viewingKey:
      'mn_shield-esk_undeployed1dlyj7u8juj68fd4psnkqhjxh32sec0q480vzswg8kd485e2kljcs9ete5h',
  },
  {
    seed: '0000000000000000000000000000000000000000000000000000000000000001',
    network: 'preview',
    addresses: {
      unshielded: 'mn_addr_preview1h3ssm5ru2t6eqy4g3she78zlxn96e36ms6pq996aduvmateh9p9svl7juu',
      shielded:
        'mn_shield-addr_preview1r020sfa7jllsz0z2wqhykz8npmphsu5223nsea7vjt9ekxs5almtvtnrpgpszud4uyd0yjrlqyp7v5xvwqljsng2g79j5w4al9c4kuqzxa6l0',
      dust: 'mn_dust_preview1w0l54txthpu8q05j9j9ttk3j5dyu5766dc9zkz2s435vdglzwdr356gp76y',
    },
    viewingKey: 'mn_shield-esk_preview1dlyj7u8juj68fd4psnkqhjxh32sec0q480vzswg8kd485e2kljcsctgnju',
  },
  {
    seed: 'a3f1c2d4e5b60718293a4b5c6d7e8f90112233445566778899aabbccddeeff00',
    network: 'undeployed',
    addresses: {
      unshielded: 'mn_addr_undeployed1kfj86nc5jy4t63h033mf9qgmcgaxrk0rkz0l2kp4hlmuk8kdxc6smm2k4z',
      shielded:
        'mn_shield-addr_undeployed1m4shumggswxchl7k2p7rrvdssg7npyztxk6na6f0wk68zs938kuemc792q5fllfsadk8jc3k72feht7x4sphysns3upt95gvmjn75ts2km027',
      dust: 'mn_dust_undeployed1wv78wtssv6qmkm7j2x9r6dkch2gwvcks3gm43jv3p075ck3cs2d5jahgzwq',
    },
    viewingKey:
      'mn_shield-esk_undeployed1wd7dn78a8c78r2uu8py3cyzpqply2ue582fxu86lhf6jmmhgw5gsxw6r2jv',
  },
  {
    seed: 'a3f1c2d4e5b60718293a4b5c6d7e8f90112233445566778899aabbccddeeff00',
    network: 'preview',
    addresses: {
      unshielded: 'mn_addr_preview1kfj86nc5jy4t63h033mf9qgmcgaxrk0rkz0l2kp4hlmuk8kdxc6sppwchw',
      shielded:
        'mn_shield-addr_preview1m4shumggswxchl7k2p7rrvdssg7npyztxk6na6f0wk68zs938kuemc792q5fllfsadk8jc3k72feht7x4sphysns3upt95gvmjn75tsvlq7my',
      dust: 'mn_dust_preview1wv78wtssv6qmkm7j2x9r6dkch2gwvcks3gm43jv3p075ck3cs2d5j23hemq',
    },
    viewingKey: 'mn_shield-esk_preview1wd7dn78a8c78r2uu8py3cyzpqply2ue582fxu86lhf6jmmhgw5gsx9sntq2',
  },
];

describe.each(TOOLKIT_OUTPUT)('moth key derivation for $network seed $seed', (expected) => {
  test('derives the same addresses as the toolkit', () => {
    expect(deriveMothAddresses(expected.seed, expected.network)).toEqual(expected.addresses);
  });

  test('derives the same viewing key as the toolkit', () => {
    expect(deriveMothViewingKey(expected.seed, expected.network)).toBe(expected.viewingKey);
  });
});
