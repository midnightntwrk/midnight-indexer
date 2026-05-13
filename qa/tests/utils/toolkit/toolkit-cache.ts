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

import { execFile } from 'child_process';
import { createServer } from 'net';
import fs from 'fs';
import path from 'path';
import { promisify } from 'util';

const execFileAsync = promisify(execFile);

const CONTAINER_NAME = 'toolkit-postgres';
const POSTGRES_IMAGE = 'postgres:16';
const POSTGRES_USER = 'toolkit';
const POSTGRES_PASSWORD = 'toolkit';
const POSTGRES_DB = 'toolkit';
const POSTGRES_INTERNAL_PORT = 5432;
const READY_TIMEOUT_MS = 30_000;
const READY_POLL_INTERVAL_MS = 1_000;

// Resolved relative to the qa/tests working directory (where vitest is invoked).
// Mirrors the convention used by ToolkitWrapper for `.tmp/toolkit`.
const DATA_DIR = path.resolve('.tmp/toolkit-postgres-data');

export interface ToolkitCacheConnection {
  host: string;
  port: number;
  user: string;
  password: string;
  database: string;
  fetchCacheUrl: string;
}

let cached: Promise<ToolkitCacheConnection> | undefined;

/**
 * Ensure a local Postgres container is running for the midnight-toolkit fetch
 * cache (`MN_FETCH_CACHE`) and return its connection details.
 *
 * Idempotent: subsequent calls within the same process reuse the result; a
 * pre-existing `toolkit-postgres` container (possibly started by another
 * test worker) is detected and reused at its currently-bound port.
 */
export async function ensureToolkitCachePostgres(): Promise<ToolkitCacheConnection> {
  if (!cached) {
    cached = bootstrap().catch((err) => {
      cached = undefined;
      throw err;
    });
  }
  return cached;
}

async function bootstrap(): Promise<ToolkitCacheConnection> {
  fs.mkdirSync(DATA_DIR, { recursive: true });

  const port = await ensureContainer();
  await waitForReady();

  const fetchCacheUrl = `postgres://${POSTGRES_USER}:${POSTGRES_PASSWORD}@host.docker.internal:${port}/${POSTGRES_DB}`;
  return {
    host: 'host.docker.internal',
    port,
    user: POSTGRES_USER,
    password: POSTGRES_PASSWORD,
    database: POSTGRES_DB,
    fetchCacheUrl,
  };
}

async function ensureContainer(): Promise<number> {
  const existing = await inspectContainer();
  if (existing) {
    if (!existing.running) {
      await execFileAsync('docker', ['start', CONTAINER_NAME]);
    }
    const port = existing.port ?? (await inspectContainer())?.port;
    if (!port) {
      throw new Error(`Could not determine host port for existing ${CONTAINER_NAME} container`);
    }
    console.log(`[CACHE] Reusing ${CONTAINER_NAME} on host port ${port}`);
    return port;
  }

  const port = await getFreePort();
  try {
    await execFileAsync('docker', [
      'run',
      '-d',
      '--name',
      CONTAINER_NAME,
      '-p',
      `${port}:${POSTGRES_INTERNAL_PORT}`,
      '-e',
      `POSTGRES_USER=${POSTGRES_USER}`,
      '-e',
      `POSTGRES_PASSWORD=${POSTGRES_PASSWORD}`,
      '-e',
      `POSTGRES_DB=${POSTGRES_DB}`,
      '-v',
      `${DATA_DIR}:/var/lib/postgresql/data`,
      POSTGRES_IMAGE,
    ]);
    console.log(`[CACHE] Started ${CONTAINER_NAME} on host port ${port} (data: ${DATA_DIR})`);
    return port;
  } catch (err) {
    // Race: another worker may have started the container between our
    // inspect and our run. If so, fall back to inspecting the existing one.
    const message = errorMessage(err);
    if (isNameConflict(message)) {
      const raced = await inspectContainer();
      if (raced) {
        if (!raced.running) await execFileAsync('docker', ['start', CONTAINER_NAME]);
        if (!raced.port) {
          throw new Error(`Lost ${CONTAINER_NAME} startup race but could not read its host port`);
        }
        console.log(`[CACHE] Adopted ${CONTAINER_NAME} on host port ${raced.port} after race`);
        return raced.port;
      }
    }
    throw err;
  }
}

interface ContainerInfo {
  running: boolean;
  port?: number;
}

async function inspectContainer(): Promise<ContainerInfo | null> {
  try {
    const { stdout } = await execFileAsync('docker', [
      'inspect',
      '--format',
      `{{.State.Running}}|{{with index .NetworkSettings.Ports "${POSTGRES_INTERNAL_PORT}/tcp"}}{{(index . 0).HostPort}}{{end}}`,
      CONTAINER_NAME,
    ]);
    const [runningStr, portStr] = stdout.trim().split('|');
    const port = portStr ? parseInt(portStr, 10) : undefined;
    return {
      running: runningStr === 'true',
      port: Number.isFinite(port) ? port : undefined,
    };
  } catch {
    return null;
  }
}

async function waitForReady(): Promise<void> {
  const deadline = Date.now() + READY_TIMEOUT_MS;
  while (Date.now() < deadline) {
    try {
      await execFileAsync('docker', [
        'exec',
        CONTAINER_NAME,
        'pg_isready',
        '-U',
        POSTGRES_USER,
        '-d',
        POSTGRES_DB,
      ]);
      return;
    } catch {
      await sleep(READY_POLL_INTERVAL_MS);
    }
  }
  throw new Error(`${CONTAINER_NAME} did not become ready within ${READY_TIMEOUT_MS}ms`);
}

async function getFreePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const srv = createServer();
    srv.unref();
    srv.on('error', reject);
    srv.listen(0, '127.0.0.1', () => {
      const addr = srv.address();
      if (addr && typeof addr === 'object') {
        const port = addr.port;
        srv.close(() => resolve(port));
      } else {
        srv.close();
        reject(new Error('Failed to allocate a free port'));
      }
    });
  });
}

function sleep(ms: number): Promise<void> {
  return new Promise((res) => setTimeout(res, ms));
}

function errorMessage(err: unknown): string {
  if (err && typeof err === 'object') {
    const e = err as { stderr?: string | Buffer; message?: string };
    return String(e.stderr ?? e.message ?? '');
  }
  return String(err);
}

function isNameConflict(message: string): boolean {
  return (
    message.includes('is already in use by container') ||
    message.includes('Conflict. The container name') ||
    message.includes('already exists')
  );
}

// ---------------------------------------------------------------------------
// Progress reporter
// ---------------------------------------------------------------------------

const PROGRESS_INTERVAL_MS = 10_000;

interface ChainProgress {
  chainId: string;
  blockCount: number;
  highestBlock: number;
}

export interface CacheProgressReporter {
  stop: () => void;
}

/**
 * Start a periodic reporter that prints cache sync progress to the console.
 * Polls highest_verified and raw_block_data_v2 every 10 s, showing block
 * counts per chain_id and flagging any stale chains left over from past
 * env resets.
 *
 * Returns a handle whose stop() must be called when warmup completes.
 */
export function startCacheProgressReporter(label: string = 'cache'): CacheProgressReporter {
  const prev = new Map<string, number>();

  const tick = async () => {
    try {
      const chains = await queryChainProgress();
      if (chains.length === 0) {
        console.log(`[CACHE:${label}] Waiting for first blocks…`);
        return;
      }

      // Newest active chain = highest block_count (most writes)
      const active = chains.reduce((a, b) => (a.blockCount >= b.blockCount ? a : b));

      for (const c of chains) {
        const delta = c.blockCount - (prev.get(c.chainId) ?? 0);
        prev.set(c.chainId, c.blockCount);
        const tag = c.chainId === active.chainId ? '' : ' ⚠ stale';
        console.log(
          `[CACHE:${label}] chain ${c.chainId}${tag} — ${c.blockCount.toLocaleString()} blocks` +
            ` (Δ +${delta.toLocaleString()} in ${PROGRESS_INTERVAL_MS / 1000}s)` +
            ` — highest: ${c.highestBlock.toLocaleString()}`,
        );
      }

      const staleCount = chains.length - 1;
      if (staleCount > 0) {
        console.warn(
          `[CACHE:${label}] ⚠  ${staleCount} stale chain(s) detected from past env resets.` +
            ` Run \`docker rm -f ${CONTAINER_NAME}\` and delete \`.tmp/toolkit-postgres-data\`` +
            ` to reclaim space.`,
        );
      }
    } catch {
      // Non-fatal — reporter runs best-effort alongside warmup
    }
  };

  const handle = setInterval(() => void tick(), PROGRESS_INTERVAL_MS);
  // Fire an initial tick after a short delay so the first row has time to appear
  setTimeout(() => void tick(), 3_000);

  return { stop: () => clearInterval(handle) };
}

async function queryChainProgress(): Promise<ChainProgress[]> {
  const sql = `
    SELECT
      encode(r.chain_id, 'hex') AS chain_id,
      COUNT(*)::bigint            AS block_count,
      COALESCE(h.height, 0)       AS highest_block
    FROM raw_block_data_v2 r
    LEFT JOIN highest_verified h USING (chain_id)
    GROUP BY r.chain_id, h.height
    ORDER BY block_count DESC;
  `.trim();

  const { stdout } = await execFileAsync('docker', [
    'exec',
    CONTAINER_NAME,
    'psql',
    '-U',
    POSTGRES_USER,
    '-d',
    POSTGRES_DB,
    '-t',
    '-A',
    '-F',
    '|',
    '-c',
    sql,
  ]);

  return stdout
    .trim()
    .split('\n')
    .filter(Boolean)
    .map((line) => {
      const [chainId, blockCountStr, highestBlockStr] = line.split('|');
      return {
        chainId: `0x${chainId.slice(0, 8)}…`,
        blockCount: parseInt(blockCountStr, 10),
        highestBlock: parseInt(highestBlockStr, 10),
      };
    });
}
