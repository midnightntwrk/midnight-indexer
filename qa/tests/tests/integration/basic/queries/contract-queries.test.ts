// This file is part of midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
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

import { IndexerHttpClient } from '@utils/indexer/http-client';

const indexerHttpClient = new IndexerHttpClient();

describe('contract queries', () => {
  describe('a contract query by address', () => {
    test.only('should return the the contract with that address, given that contract exists', async () => {
      const contractAddress =
        '000200e99d4445695a6244a01ab00d592825e2703c3f9a928f01429561585ce2db1e78';
      const response = await indexerHttpClient.getContractAction(contractAddress);
      expect(response).toBeSuccess();
      expect(response.data?.contractAction).toBeDefined();
      expect(response.data?.contractAction.address).toBe(contractAddress);
    });
    test('should return an empty response, given a contract with that address does not exist', async () => {});
    test('should return an error, given an invalid address', async () => {});
  });

  describe('a contract query by address and hash', () => {
    test('should return the the contract with that address and hash, given that contract exists', async () => {});
    test('should return an empty response, given a contract with that address and/or hash does not exist', async () => {});
    test('should return an error, given an invalid address and/or hash', async () => {});
  });
});
