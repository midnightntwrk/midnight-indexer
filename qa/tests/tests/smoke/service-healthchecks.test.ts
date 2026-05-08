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
import { env } from 'environment/model';
import '@utils/logging/test-logging-hooks';

describe(`service health checks`, () => {
  const baseUrl = `${env.getIndexerHttpBaseURL()}`;

  describe(`a request to the /ready endpoint`, async () => {
    /**
     * This test checks if the Indexer is ready to serve requests.
     *
     * @When a request is sent to the Indexer's /ready endpoint
     * @Then the response should be OK
     */
    test('should return a 200 status code OK', async () => {
      log.info('Checking Indexer is ready');
      const targetUrl = baseUrl + '/ready';
      log.debug(`Target URL: ${targetUrl}`);
      const response = await fetch(targetUrl);

      expect(response.ok).toBe(true);
    });
  });

  describe(`a request to an unrecognised path under /api/v4`, () => {
    const unknownPath = '/api/v4/__regression_unknown_path';

    /**
     * Regression test for midnight-indexer#1085: unrecognised paths under
     * /api/v4 must not respond with a 308 whose Location double-prepends
     * /api/v4 (e.g. /api/v4/schema -> /api/v4/v4/schema), which causes any
     * client that follows redirects to loop until its redirect cap is hit.
     *
     * @When a GET is sent to an unrecognised path under /api/v4
     * @Then the response is NOT a 308 redirect whose Location contains
     *       /api/v4/v4 (a 404, or any non-prefix-doubling response, is fine)
     */
    test('should not 308 to a /api/v4-double-prefixed Location', async () => {
      const targetUrl = baseUrl + unknownPath;
      log.debug(`Target URL: ${targetUrl}`);
      const response = await fetch(targetUrl, { redirect: 'manual' });
      log.debug(`Status: ${response.status}`);

      if (response.status === 308 || response.status === 301 || response.status === 302) {
        const location = response.headers.get('location') ?? '';
        log.debug(`Location: ${location}`);
        expect(location.includes('/api/v4/v4')).toBe(false);
      } else {
        expect(response.status).toBeGreaterThanOrEqual(400);
      }
    });

    /**
     * Regression test for midnight-indexer#1085: when redirects are followed,
     * the request must terminate (no infinite redirect loop).
     *
     * @When a GET to an unrecognised /api/v4 path follows redirects
     * @Then fetch resolves with a 4xx (no redirect-cap exhaustion)
     */
    test('should terminate when redirects are followed', async () => {
      const targetUrl = baseUrl + unknownPath;
      const response = await fetch(targetUrl, { redirect: 'follow' });
      log.debug(`Status (after redirects): ${response.status}`);
      expect(response.status).toBeGreaterThanOrEqual(400);
    });
  });
});
