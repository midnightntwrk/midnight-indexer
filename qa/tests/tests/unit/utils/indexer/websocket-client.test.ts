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

// `IndexerWsClient.assertSocketAlive()` is what turns a dropped websocket into
// an immediate, explicit failure instead of a poll that runs out its budget and
// reports "not found yet". It has two triggers:
//
//   1. the socket is closed, whatever the reason;
//   2. the socket is open but has delivered nothing for 90s — which only proves
//      anything while the client keepalive is pinging, because a quiet
//      subscription on a healthy socket is silent too.
//
// The integration counter-tests (websocket-keepalive.test.ts) deliberately read
// `isSocketOpen()` and cost 150s each, so neither trigger is exercised there.
// These tests drive both against a fake socket and a mocked clock, in
// milliseconds, with no environment.

import { DeadSocketError, IndexerWsClient } from '@utils/indexer/websocket-client';
import { NonRetryableError } from '@utils/retry-helper';

// The client's constructor reads the target URL from the environment model,
// whose own constructor demands TARGET_ENV and a NODE_VERSIONS file. Neither
// has any bearing on liveness detection.
vi.mock('environment/model', () => ({
  env: { getIndexerWebsocketBaseURL: () => 'ws://indexer.test' },
}));
vi.mock('@utils/logging/logger', () => ({
  default: { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() },
}));

/** Mirrors `IndexerWsClient.DEAD_SOCKET_AFTER_MS`, which is private. */
const DEAD_SOCKET_AFTER_MS = 90_000;

type Listener = (event: unknown) => void;

/**
 * The smallest stand-in for the global `WebSocket` that `IndexerWsClient` can
 * connect through.
 *
 * It starts OPEN and answers `connection_init` with `connection_ack` from
 * inside `send()`, so `connectionInit()` completes without a single timer
 * firing. It answers NOTHING else — in particular not the keepalive `ping`:
 * every inbound frame stamps the client's last-inbound time, so a fake that
 * ponged would make the silence trigger unreachable. A test that wants the
 * clock reset calls `receive()` explicitly.
 *
 * `emit()` invokes both `addEventListener` listeners and the `on<type>`
 * property, because the client uses the former for its one-off waits and the
 * latter for routing.
 */
class FakeWebSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;

  /** Every socket constructed since the last reset, in creation order. */
  static instances: FakeWebSocket[] = [];

  readyState = FakeWebSocket.OPEN;
  readonly sent: string[] = [];
  onmessage: Listener | null = null;
  onerror: Listener | null = null;
  onclose: Listener | null = null;
  private readonly listeners = new Map<string, Set<Listener>>();

  constructor(
    readonly url: string,
    readonly protocol?: string,
  ) {
    FakeWebSocket.instances.push(this);
  }

  addEventListener(type: string, listener: Listener): void {
    const set = this.listeners.get(type) ?? new Set<Listener>();
    set.add(listener);
    this.listeners.set(type, set);
  }

  removeEventListener(type: string, listener: Listener): void {
    this.listeners.get(type)?.delete(listener);
  }

  send(raw: string): void {
    if (this.readyState !== FakeWebSocket.OPEN) {
      throw new Error('WebSocket is not open');
    }
    this.sent.push(raw);
    if (JSON.parse(raw).type === 'connection_init') {
      this.receive({ type: 'connection_ack' });
    }
  }

  /** A clean, client-initiated close, as `connectionClose()` performs. */
  close(): void {
    if (this.readyState === FakeWebSocket.CLOSED) return;
    this.readyState = FakeWebSocket.CLOSED;
    this.emit('close', { code: 1000, reason: '', wasClean: true });
  }

  /** The drop this whole mechanism exists for: the ingress vanishes, bare 1006. */
  dropFromServer(): void {
    this.readyState = FakeWebSocket.CLOSED;
    this.emit('close', { code: 1006, reason: '', wasClean: false });
  }

  /** Deliver a frame from the "server". */
  receive(message: Record<string, unknown>): void {
    this.emit('message', { data: JSON.stringify(message) });
  }

  private emit(type: string, event: unknown): void {
    const handler = (this as unknown as Record<string, Listener | null>)[`on${type}`];
    handler?.(event);
    // Snapshot: listeners remove themselves while being notified.
    [...(this.listeners.get(type) ?? [])].forEach((listener) => listener(event));
  }
}

/** Move only the clock, not the timers: `Date.now()` is what the client reads. */
const advanceClock = (ms: number) => vi.setSystemTime(Date.now() + ms);

describe('IndexerWsClient liveness', () => {
  let client: IndexerWsClient | undefined;

  async function connect(): Promise<{ client: IndexerWsClient; socket: FakeWebSocket }> {
    client = new IndexerWsClient();
    await client.connectionInit();
    return { client, socket: FakeWebSocket.instances.at(-1)! };
  }

  beforeEach(() => {
    vi.stubGlobal('WebSocket', FakeWebSocket);
  });

  afterEach(async () => {
    // Releases the keepalive interval. Skipped on a socket that is already
    // closed: the real close path would then wait out its 2s fallback timeout.
    if (client?.isSocketOpen()) {
      await client.connectionClose();
    }
    client = undefined;
    FakeWebSocket.instances = [];
    vi.useRealTimers();
    vi.unstubAllEnvs();
    vi.unstubAllGlobals();
  });

  describe('a closed socket', () => {
    /**
     * @given a client that has never connected
     * @when its liveness is asserted
     * @then it reports a dead socket, and the error is non-retryable
     */
    test('should be reported dead before connectionInit', () => {
      client = new IndexerWsClient();

      expect(client.isSocketOpen()).toBe(false);
      expect(() => client!.assertSocketAlive()).toThrow(DeadSocketError);
    });

    /**
     * @given a connected client whose socket the server drops without a close frame
     * @when its liveness is asserted
     * @then it reports a dead socket at once, and the error is non-retryable
     */
    test('should be reported dead after the server drops it', async () => {
      const { client, socket } = await connect();
      expect(client.isSocketOpen()).toBe(true);

      socket.dropFromServer();

      expect(client.isSocketOpen()).toBe(false);
      let caught: unknown;
      try {
        client.assertSocketAlive();
      } catch (error) {
        caught = error;
      }
      expect(caught).toBeInstanceOf(DeadSocketError);
      expect(caught).toBeInstanceOf(NonRetryableError);
      expect((caught as Error).message).toMatch(/websocket is closed/);
    });
  });

  describe('an open socket with the client keepalive (the default)', () => {
    /**
     * @given a connected client with the keepalive running
     * @when the socket has delivered nothing for just under 90s
     * @then it is still considered alive
     */
    test('should be alive while the silence is under the limit', async () => {
      const { client } = await connect();
      expect(() => client.assertSocketAlive()).not.toThrow();

      advanceClock(DEAD_SOCKET_AFTER_MS - 1_000);

      expect(() => client.assertSocketAlive()).not.toThrow();
    });

    /**
     * @given a connected client with the keepalive running
     * @when the socket has delivered nothing for over 90s, not even a pong
     * @then it is reported dead, with the silence in the message, non-retryably
     */
    test('should be reported dead after 90s of silence', async () => {
      const { client } = await connect();

      advanceClock(DEAD_SOCKET_AFTER_MS + 1_000);

      expect(client.isSocketOpen()).toBe(true);
      let caught: unknown;
      try {
        client.assertSocketAlive();
      } catch (error) {
        caught = error;
      }
      expect(caught).toBeInstanceOf(DeadSocketError);
      expect(caught).toBeInstanceOf(NonRetryableError);
      expect((caught as Error).message).toMatch(/delivered nothing for 91s/);
    });

    /**
     * @given a connected client with the keepalive running
     * @when any frame arrives before the limit, such as the pong to a ping
     * @then the silence is measured from that frame, not from the connection
     */
    test('should measure the silence from the last inbound frame', async () => {
      const { client, socket } = await connect();

      advanceClock(80_000);
      socket.receive({ type: 'pong' });
      advanceClock(80_000);

      expect(() => client.assertSocketAlive()).not.toThrow();

      advanceClock(20_000);

      expect(() => client.assertSocketAlive()).toThrow(DeadSocketError);
    });
  });

  describe('an open socket without the client keepalive', () => {
    /**
     * @given a connected client with INDEXER_WS_KEEPALIVE=off
     * @when the socket has delivered nothing for far longer than 90s
     * @then it is still considered alive: without pings, silence proves nothing
     */
    test('should not be reported dead for silence alone', async () => {
      // Read in startKeepAlive(), i.e. during connectionInit(), not by the constructor.
      vi.stubEnv('INDEXER_WS_KEEPALIVE', 'off');
      const { client } = await connect();

      advanceClock(10 * 60_000);

      expect(() => client.assertSocketAlive()).not.toThrow();
    });

    /**
     * @given a connected client with INDEXER_WS_KEEPALIVE=off
     * @when the server drops the socket
     * @then it is still reported dead: the closed check does not depend on the keepalive
     */
    test('should still be reported dead once closed', async () => {
      vi.stubEnv('INDEXER_WS_KEEPALIVE', 'off');
      const { client, socket } = await connect();

      socket.dropFromServer();

      expect(() => client.assertSocketAlive()).toThrow(DeadSocketError);
    });
  });
});
