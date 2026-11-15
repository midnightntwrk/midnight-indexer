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

import { deflateSync, inflateSync } from 'node:zlib';
import WebSocket from 'ws';
import { env } from 'environment/model';

export const GRAPHQL_TRANSPORT_WS = 'graphql-transport-ws';
export const GRAPHQL_WS = 'graphql-ws';
export const GRAPHQL_TRANSPORT_WS_DEFLATE = 'graphql-transport-ws+deflate';

// Large enough to exceed the server's 256-byte compression threshold many
// times over, so a deflate connection must answer it with a binary frame.
export const LARGE_RESPONSE_QUERY = `{
  __schema {
    types {
      name
      fields { name }
    }
  }
}`;

export interface WsProbeMessage {
  type: string;
  id?: string;
  payload?: unknown;
}

export interface WsProbeFrame {
  // True when the server sent a binary (zlib-compressed) frame.
  binary: boolean;
  // Size of the frame as it travelled, before inflating.
  wireBytes: number;
  // Size of the JSON text the frame carries, after inflating.
  jsonBytes: number;
  message: WsProbeMessage;
}

export interface WsProbeResult {
  // False when the server answered the handshake with a plain HTTP response.
  upgraded: boolean;
  // 101 when upgraded, otherwise the HTTP status of the rejection.
  status: number;
  // The Sec-WebSocket-Protocol the server selected, or null when not upgraded.
  selectedProtocol: string | null;
  // Every frame the server sent, in order.
  frames: WsProbeFrame[];
  // The close code and reason the connection ended with, or null when not
  // upgraded. 1000 is the probe's own orderly close; anything else is the
  // server (or 1006, a dropped connection) and explains missing frames.
  closeCode: number | null;
  closeReason: string | null;
}

export interface WsProbeOptions {
  // The operation to run once the connection is acknowledged.
  query?: string;
  // Send client frames zlib-compressed as binary, as the deflate protocol allows.
  compressClientFrames?: boolean;
  // Stop after this many result frames. Unset: run until the server completes
  // the operation, which a query does by itself and a live subscription never does.
  maxResults?: number;
  timeoutMs?: number;
}

/**
 * Opens a raw WebSocket to the GraphQL endpoint offering exactly the given
 * subprotocols, runs one operation, and reports what happened on the wire.
 *
 * `IndexerWsClient` cannot be used for this: it hard-codes the subprotocol,
 * only understands text frames, and hides the handshake. This probe uses the
 * `ws` package rather than the global WebSocket because only `ws` exposes the
 * HTTP status of a rejected upgrade.
 *
 * The operation is started in the message format of the protocol the SERVER
 * SELECTED (`start` for graphql-ws, `subscribe` otherwise), which is what a
 * conforming client does. Callers then compare the reply's format with the
 * selected protocol; the probe itself does not judge.
 */
export async function probeGraphQLWs(
  offers: string[],
  options: WsProbeOptions = {},
): Promise<WsProbeResult> {
  const {
    query = LARGE_RESPONSE_QUERY,
    compressClientFrames = false,
    maxResults,
    timeoutMs = 30_000,
  } = options;
  const url = `${env.getIndexerGraphqlWsURL()}/ws`;
  const operationId = '1';

  return new Promise((resolve, reject) => {
    const frames: WsProbeFrame[] = [];
    const socket = new WebSocket(url, offers);
    let results = 0;

    const timer = setTimeout(() => {
      socket.terminate();
      reject(
        new Error(
          `ws probe timed out after ${timeoutMs}ms, offers=${JSON.stringify(offers)}, ` +
            `frames so far: ${JSON.stringify(frames.map((frame) => frame.message.type))}`,
        ),
      );
    }, timeoutMs);

    const finish = (result: WsProbeResult) => {
      clearTimeout(timer);
      resolve(result);
    };
    const fail = (error: Error) => {
      clearTimeout(timer);
      socket.terminate();
      reject(error);
    };
    const send = (message: WsProbeMessage) => {
      const json = JSON.stringify(message);
      socket.send(compressClientFrames ? deflateSync(json) : json);
    };
    socket.on('unexpected-response', (request, response) => {
      request.destroy();
      finish({
        upgraded: false,
        status: response.statusCode ?? 0,
        selectedProtocol: null,
        frames,
        closeCode: null,
        closeReason: null,
      });
    });

    socket.on('open', () => send({ type: 'connection_init' }));

    socket.on('message', (data, isBinary) => {
      const wire = Buffer.isBuffer(data)
        ? data
        : Array.isArray(data)
          ? Buffer.concat(data)
          : Buffer.from(data);
      let message: WsProbeMessage;
      let jsonBytes: number;
      try {
        const json = isBinary ? inflateSync(wire) : wire;
        jsonBytes = json.length;
        message = JSON.parse(json.toString('utf8')) as WsProbeMessage;
      } catch (error) {
        fail(new Error(`ws probe cannot decode a ${isBinary ? 'binary' : 'text'} frame: ${error}`));
        return;
      }
      frames.push({ binary: isBinary, wireBytes: wire.length, jsonBytes, message });

      const legacy = socket.protocol === GRAPHQL_WS;
      switch (message.type) {
        case 'connection_ack':
          send({ id: operationId, type: legacy ? 'start' : 'subscribe', payload: { query } });
          break;
        case 'next':
        case 'data':
          results += 1;
          if (maxResults !== undefined && results >= maxResults) {
            send({ id: operationId, type: legacy ? 'stop' : 'complete' });
            socket.close(1000);
          }
          break;
        case 'complete':
        case 'error':
        case 'connection_error':
          socket.close(1000);
          break;
        default:
          // Keepalives (`ka`, `ping`, `pong`) carry nothing to act on.
          break;
      }
    });

    socket.on('close', (code, reason) =>
      finish({
        upgraded: true,
        status: 101,
        selectedProtocol: socket.protocol,
        frames,
        closeCode: code,
        closeReason: reason.toString(),
      }),
    );
    socket.on('error', (error) => fail(error));
  });
}
