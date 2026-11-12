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

// A protocol contract test, of the kind graphql-http-compression.test.ts
// describes: it checks a transport property (WebSocket subprotocol negotiation
// and the opt-in `graphql-transport-ws+deflate` wire format), not domain data.
//
// Two parties decide the outcome of one handshake: the WebSocket layer picks
// the subprotocol it echoes to the client, and the GraphQL layer picks the
// message format it speaks. The contract is that they agree:
//
//   - whatever subprotocol the server says it selected, it speaks that format;
//   - compressed (binary) frames appear only when deflate was selected;
//   - an offer naming no known protocol is refused before the upgrade.
//
// Deflate offered WITHOUT a standard protocol next to it was refused with 400
// until midnight-indexer #1751. These tests run through whatever sits in front
// of the indexer (ingress, gateway), which the Rust unit tests cannot reach.
//
// Deliberately not gated on the build: a build that refuses a deflate-only
// offer must show up as a failure, not as a skip that a regression would also
// produce. The suite therefore needs an indexer that includes #1751, and the
// `graphql-ws` before `graphql-transport-ws` case needs #1775 as well. The
// pipeline that runs it unattended (build-indexer-images) tests the images
// built from the same commit as these tests, so it always has both. Pointing
// the suite at an older build, by hand or on a deployed environment that has
// not been upgraded yet, fails these cases, and that failure is accurate.

import '@utils/logging/test-logging-hooks';
import {
  GRAPHQL_TRANSPORT_WS,
  GRAPHQL_TRANSPORT_WS_DEFLATE,
  GRAPHQL_WS,
  probeGraphQLWs,
  type WsProbeFrame,
  type WsProbeResult,
} from '@utils/indexer/ws-protocol-probe';

/** The frames carrying operation results, in either message format. */
function resultFrames(result: WsProbeResult): WsProbeFrame[] {
  return result.frames.filter(({ message }) => message.type === 'next' || message.type === 'data');
}

/** The result message type each protocol's format uses. */
function resultTypeOf(protocol: string | null): string {
  return protocol === GRAPHQL_WS ? 'data' : 'next';
}

/**
 * How the connection ended, as the message of an assertion on the frames. A
 * server that upgrades and then rejects sends no frames, only a close code.
 */
function closeOf(result: WsProbeResult): string {
  return `connection closed with code ${result.closeCode}, reason "${result.closeReason}"`;
}

function schemaTypesOf(frame: WsProbeFrame): unknown {
  const payload = frame.message.payload as { data?: { __schema?: { types?: unknown } } };
  return payload?.data?.__schema?.types;
}

describe('graphql websocket protocol negotiation', () => {
  describe('a handshake offering only graphql-transport-ws+deflate', () => {
    /**
     * @Given a running indexer
     * @When a WebSocket is opened offering only graphql-transport-ws+deflate
     * @Then the connection is upgraded and the deflate subprotocol is selected
     * @And the short connection_ack arrives as an uncompressed text frame
     * @And a large response arrives as a binary frame that inflates to a `next` message
     * @And the binary frame is smaller on the wire than the JSON it carries
     */
    test('should be upgraded and answer a large response with a compressed frame', async () => {
      const result = await probeGraphQLWs([GRAPHQL_TRANSPORT_WS_DEFLATE]);

      expect(result.status).toBe(101);
      expect(result.selectedProtocol).toBe(GRAPHQL_TRANSPORT_WS_DEFLATE);

      const ack = result.frames.at(0);
      expect(ack?.message.type, closeOf(result)).toBe('connection_ack');
      expect(ack?.binary).toBe(false);

      const results = resultFrames(result);
      expect(results, closeOf(result)).toHaveLength(1);
      expect(results[0].message.type).toBe('next');
      expect(results[0].binary).toBe(true);
      expect(results[0].wireBytes).toBeLessThan(results[0].jsonBytes);
      expect(Array.isArray(schemaTypesOf(results[0]))).toBe(true);
      expect(result.frames.at(-1)?.message.type).toBe('complete');
    });

    /**
     * @Given a connection that selected graphql-transport-ws+deflate
     * @When the client sends its own messages zlib-compressed as binary frames
     * @Then the server inflates them, acknowledges the connection and runs the operation
     */
    test('should understand compressed frames sent by the client', async () => {
      const result = await probeGraphQLWs([GRAPHQL_TRANSPORT_WS_DEFLATE], {
        compressClientFrames: true,
      });

      expect(result.selectedProtocol).toBe(GRAPHQL_TRANSPORT_WS_DEFLATE);
      expect(result.frames.at(0)?.message.type, closeOf(result)).toBe('connection_ack');
      const results = resultFrames(result);
      expect(results, closeOf(result)).toHaveLength(1);
      expect(Array.isArray(schemaTypesOf(results[0]))).toBe(true);
    });

    /**
     * @Given a connection that selected graphql-transport-ws+deflate
     * @When a live block subscription is started from height 1
     * @Then every block arrives as a compressed frame carrying a `next` message
     * @And the blocks arrive in height order starting at the requested height
     */
    test('should stream a block subscription as compressed frames', async () => {
      const result = await probeGraphQLWs([GRAPHQL_TRANSPORT_WS_DEFLATE], {
        query: `subscription {
          blocks(offset: { height: 1 }) {
            hash
            height
            timestamp
            ledgerParameters
            parent { hash height }
          }
        }`,
        maxResults: 3,
      });

      expect(result.selectedProtocol).toBe(GRAPHQL_TRANSPORT_WS_DEFLATE);
      const results = resultFrames(result);
      expect(results.length, closeOf(result)).toBeGreaterThanOrEqual(3);
      const blocks = results.slice(0, 3);
      for (const frame of blocks) {
        expect(frame.message.type).toBe('next');
        expect(frame.binary).toBe(true);
      }
      const heights = blocks.map(
        ({ message }) =>
          (message.payload as { data: { blocks: { height: number } } }).data.blocks.height,
      );
      expect(heights).toEqual([1, 2, 3]);
    });
  });

  describe('a handshake offering deflate next to other protocols', () => {
    /**
     * @Given a running indexer
     * @When a WebSocket is opened offering deflate together with a standard or unknown protocol, in any order
     * @Then the deflate subprotocol is selected
     * @And the server speaks the graphql-transport-ws message format, compressed
     */
    test.each([
      [[GRAPHQL_TRANSPORT_WS_DEFLATE, GRAPHQL_TRANSPORT_WS]],
      [[GRAPHQL_TRANSPORT_WS, GRAPHQL_TRANSPORT_WS_DEFLATE]],
      [[GRAPHQL_WS, GRAPHQL_TRANSPORT_WS_DEFLATE]],
      [['unknown', GRAPHQL_TRANSPORT_WS_DEFLATE]],
    ])('should select deflate for the offer %j', async (offers) => {
      const result = await probeGraphQLWs(offers);

      expect(result.status).toBe(101);
      expect(result.selectedProtocol).toBe(GRAPHQL_TRANSPORT_WS_DEFLATE);
      const results = resultFrames(result);
      expect(results, closeOf(result)).toHaveLength(1);
      expect(results[0].message.type).toBe('next');
      expect(results[0].binary).toBe(true);
    });
  });

  describe('a handshake offering only standard protocols', () => {
    /**
     * @Given a running indexer
     * @When a WebSocket is opened without offering deflate
     * @Then one of the offered standard protocols is selected
     * @And the server speaks the message format of the protocol it selected
     * @And no frame is compressed, however large the response
     *
     * The offer listing graphql-ws BEFORE graphql-transport-ws is the one that
     * tells the two negotiating layers apart: the WebSocket layer prefers the
     * modern protocol whatever the order, while the GraphQL layer on its own
     * would take the first protocol the client lists. Before midnight-indexer
     * #1775 the server announced graphql-transport-ws for that offer and then
     * answered in the legacy format.
     */
    test.each([
      [[GRAPHQL_TRANSPORT_WS], GRAPHQL_TRANSPORT_WS],
      [[GRAPHQL_WS], GRAPHQL_WS],
      [[GRAPHQL_TRANSPORT_WS, GRAPHQL_WS], GRAPHQL_TRANSPORT_WS],
      [[GRAPHQL_WS, GRAPHQL_TRANSPORT_WS], GRAPHQL_TRANSPORT_WS],
    ])(
      'should speak the selected protocol, uncompressed, for the offer %j',
      async (offers, expected) => {
        const result = await probeGraphQLWs(offers);

        expect(result.status).toBe(101);
        expect(result.selectedProtocol).toBe(expected);
        const results = resultFrames(result);
        expect(results, closeOf(result)).toHaveLength(1);
        expect(results[0].message.type).toBe(resultTypeOf(result.selectedProtocol));
        expect(result.frames.filter((frame) => frame.binary)).toEqual([]);
      },
    );
  });

  describe('a handshake offering no known protocol', () => {
    /**
     * @Given a running indexer
     * @When a WebSocket is opened offering nothing, an unknown protocol, or a near miss of the deflate name
     * @Then the upgrade is refused with 400 Bad Request
     *
     * The near misses guard against the deflate name being matched by prefix
     * or without regard to case.
     */
    test.each([
      [[]],
      [['unknown']],
      [[`${GRAPHQL_TRANSPORT_WS_DEFLATE}2`]],
      [[`${GRAPHQL_TRANSPORT_WS}+`]],
      [[GRAPHQL_TRANSPORT_WS_DEFLATE.toUpperCase()]],
    ])('should refuse the offer %j', async (offers) => {
      const result = await probeGraphQLWs(offers);

      expect(result.upgraded).toBe(false);
      expect(result.status).toBe(400);
    });
  });
});
