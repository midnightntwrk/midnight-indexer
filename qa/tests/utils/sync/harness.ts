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

import { type ChildProcess, spawn, spawnSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { env } from 'environment/model';
import { NodeRpcClient } from '@utils/node/rpc-client';
import {
  type ProgressMode,
  SyncProgressReporter,
  UNBOUNDED_MAX_BLOCKS,
} from '@utils/sync/progress-reporter';

/**
 * Blocks a run indexes before it is called a pass.
 *
 * Small on purpose: a deployed chain is millions of blocks deep, so an unbounded run
 * is a multi-day proposition and cannot be the default. Raise it with `MAX_BLOCKS`,
 * or set `MAX_BLOCKS=0` to drop the bound entirely and sync to the chain tip.
 */
export const DEFAULT_MAX_BLOCKS = 2000;

/** Wall-clock bound on a run. The only bound when `MAX_BLOCKS=0`. */
export const DEFAULT_MAX_DURATION_MS = 30 * 60_000;

const DEFAULT_API_PORT = 8188;
const DEFAULT_METRICS_PORT = 9100;
const POLL_INTERVAL_MS = 5_000;
const METRICS_TIMEOUT_MS = 5_000;
const LOG_BUFFER_LINES = 400;
const LOG_TAIL_LINES = 40;

/**
 * Failure lines chain-indexer bails out with. Kept identical to the set
 * `qa/scripts/test-hardfork-8to9.sh` greps for, so both surfaces agree on what
 * counts as a divergence.
 */
const FAILURE_PATTERN =
  /ledger state root mismatch|zswap state root mismatch|translate ledger state/i;

/**
 * Pulls the offending block out of a mismatch line. The height is the product of
 * this whole suite: it localises a divergence to a single block.
 */
const MISMATCH_PATTERN = /(ledger|zswap) state root mismatch for block (\S+) at height (\d+)/i;

export type SyncTopology = 'cloud' | 'standalone';

const TOPOLOGIES: readonly SyncTopology[] = ['cloud', 'standalone'];

export interface SyncHarnessOptions {
  topology: SyncTopology;
  indexerTag: string;
  imageRegistry: string;
  nodeUrl: string;
  networkId: string;
  /** Block budget, or `UNBOUNDED_MAX_BLOCKS` to sync to the chain tip. */
  maxBlocks: number;
  maxDurationMs: number;
  apiPort: number;
  metricsPort: number;
  progressMode: ProgressMode;
}

export type SyncOutcome = 'caught-up' | 'budget-reached' | 'timed-out' | 'exited';

export interface ServiceExit {
  service: string;
  exitCode: number;
}

export interface StateRootMismatch {
  kind: string;
  block: string;
  height: number;
  line: string;
}

export interface SyncRunResult {
  outcome: SyncOutcome;
  startHeight: number;
  height?: number;
  nodeTip?: number;
  caughtUp: boolean;
  elapsedMs: number;
  exit?: ServiceExit;
  mismatch?: StateRootMismatch;
  logTail: string[];
}

/** The three chain-indexer metrics this harness reads its progress from. */
export interface IndexerMetrics {
  blockHeight?: number;
  nodeBlockHeight?: number;
  caughtUp?: boolean;
}

/**
 * Read `indexer_block_height`, `indexer_node_block_height` and `indexer_caught_up`
 * out of a Prometheus exposition body.
 *
 * Every field is optional by design: none of these are published until the indexer
 * has processed its first block batch, so a fresh container serves a body without
 * them. An absent metric means "not known yet" and must never be read as height 0 —
 * that would make a stalled indexer look like a working one at the start of a chain.
 */
export function parseIndexerMetrics(body: string): IndexerMetrics {
  const read = (name: string): number | undefined => {
    for (const line of body.split('\n')) {
      if (line.startsWith('#')) continue;
      const [key, value] = line.trim().split(/\s+/);
      if (key !== name) continue;
      const parsed = Number(value);
      if (Number.isFinite(parsed)) return parsed;
    }
    return undefined;
  };

  const caughtUp = read('indexer_caught_up');

  return {
    blockHeight: read('indexer_block_height'),
    nodeBlockHeight: read('indexer_node_block_height'),
    caughtUp: caughtUp === undefined ? undefined : caughtUp > 0,
  };
}

function parsePositiveInt(raw: string | undefined, fallback: number, name: string): number {
  if (raw === undefined || raw.trim() === '') return fallback;
  const parsed = Number(raw);
  if (!Number.isInteger(parsed) || parsed < 0) {
    throw new Error(`${name} must be a non-negative integer, got: ${JSON.stringify(raw)}`);
  }
  return parsed;
}

/**
 * Build the run configuration from the environment.
 *
 * `TARGET_ENV` selects the chain (the node URL and network id come from the env
 * model); `SYNC_INDEXER_TAG` selects the indexer version under test. The suite
 * deliberately does not reuse `INDEXER_TAG`, which the framework documents as
 * "must not be set for deployed environments".
 */
export function readSyncOptions(): SyncHarnessOptions {
  const indexerTag = process.env.SYNC_INDEXER_TAG?.trim();
  if (!indexerTag) {
    throw new Error(
      'SYNC_INDEXER_TAG is required: it names the indexer image tag whose sync is under test.',
    );
  }

  const topology = (process.env.SYNC_TOPOLOGY?.trim() || 'cloud') as SyncTopology;
  if (!TOPOLOGIES.includes(topology)) {
    throw new Error(
      `Unknown SYNC_TOPOLOGY='${topology}'. Expected one of: ${TOPOLOGIES.join(', ')}.`,
    );
  }

  return {
    topology,
    indexerTag,
    imageRegistry: process.env.SYNC_IMAGE_REGISTRY?.trim() || 'midnightntwrk',
    nodeUrl: env.getNodeWebsocketBaseURL(),
    networkId: env.getNetworkId(),
    // 0 is the documented "unbounded" sentinel, not an empty budget.
    maxBlocks: parsePositiveInt(process.env.MAX_BLOCKS, DEFAULT_MAX_BLOCKS, 'MAX_BLOCKS'),
    maxDurationMs: parsePositiveInt(
      process.env.MAX_DURATION_MS,
      DEFAULT_MAX_DURATION_MS,
      'MAX_DURATION_MS',
    ),
    apiPort: parsePositiveInt(process.env.SYNC_API_PORT, DEFAULT_API_PORT, 'SYNC_API_PORT'),
    metricsPort: parsePositiveInt(
      process.env.SYNC_METRICS_PORT,
      DEFAULT_METRICS_PORT,
      'SYNC_METRICS_PORT',
    ),
    progressMode: process.env.SYNC_PROGRESS === 'live' ? 'live' : 'plain',
  };
}

/** Why the suite cannot run here, or undefined when it can. */
export function syncPrerequisiteGap(): string | undefined {
  if (!process.env.SYNC_INDEXER_TAG?.trim()) return 'SYNC_INDEXER_TAG is not set';
  if (spawnSync('docker', ['compose', 'version'], { stdio: 'ignore' }).status !== 0) {
    return 'docker compose is not available';
  }
  return undefined;
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * Runs one indexer version against one chain and watches it sync.
 *
 * Brings the stack up from the dedicated compose file, follows the container logs
 * for a bail-out, and polls the Prometheus endpoint for height and caught-up state.
 * The compose project name is unique per process, so a run can never collide with
 * the repo's development stack, and teardown only ever removes what this instance
 * started.
 */
export class IndexerSyncHarness {
  private readonly project: string;
  private readonly composeFile: string;
  private readonly rpc: NodeRpcClient;
  private readonly secrets: Record<string, string>;
  private readonly logBuffer: string[] = [];
  private startedByUs = false;
  private logStream?: ChildProcess;
  private mismatch?: StateRootMismatch;
  private failureLine?: string;

  constructor(private readonly options: SyncHarnessOptions) {
    this.project = `midnight-indexer-sync-${options.topology}-${process.pid}`;
    // utils/sync/harness.ts -> qa/tests/utils/sync, so four levels up is the repo root.
    const here = path.dirname(fileURLToPath(import.meta.url));
    this.composeFile = path.resolve(
      here,
      '..',
      '..',
      '..',
      '..',
      'qa/docker/docker-compose-sync.yaml',
    );
    this.rpc = new NodeRpcClient();
    // Generated per run: nothing in this suite reads wallet data, so there is no reason
    // to require the developer's real secrets. APP__INFRA__SECRET must be >= 32 hex bytes.
    this.secrets = {
      APP__INFRA__SECRET: randomBytes(32).toString('hex'),
      APP__INFRA__STORAGE__PASSWORD: randomBytes(16).toString('hex'),
      APP__INFRA__PUB_SUB__PASSWORD: randomBytes(16).toString('hex'),
    };
  }

  /** Bring the stack up and start following its logs. */
  async start(): Promise<void> {
    console.log(
      `[SYNC] Starting ${this.options.topology} stack, image tag ${this.options.indexerTag}, ` +
        `indexing ${this.options.nodeUrl} as network '${this.options.networkId}'.`,
    );

    const result = spawnSync('docker', this.composeArgs(['up', '-d']), {
      env: this.composeEnv(),
      stdio: 'inherit',
    });
    // Set eagerly: a partial bring-up still leaves containers to clean up.
    this.startedByUs = true;
    if (result.status !== 0) {
      throw new Error(`[SYNC] docker compose up exited with status ${result.status}`);
    }

    this.followLogs();
  }

  /** Poll until the run reaches its target, bails out, or runs out of time. */
  async run(): Promise<SyncRunResult> {
    const startedAtMs = Date.now();
    // A fresh stack always starts from genesis; there is no prior state to resume.
    const startHeight = 0;
    const { maxBlocks, maxDurationMs } = this.options;
    const reporter = new SyncProgressReporter(
      { startHeight, maxBlocks, startedAtMs },
      this.options.progressMode,
    );

    let outcome: SyncOutcome = 'timed-out';
    let height: number | undefined;
    let nodeTip: number | undefined;
    let caughtUp = false;

    for (;;) {
      const metrics = await this.scrapeMetrics();
      height = metrics.blockHeight ?? height;
      caughtUp = metrics.caughtUp ?? false;
      // Prefer the tip the indexer itself sees; fall back to the node directly while
      // the metrics endpoint is still silent.
      nodeTip = metrics.nodeBlockHeight ?? (await this.chainTip()) ?? nodeTip;

      reporter.update(height, nodeTip);

      const exit = this.exitedService();
      if (exit !== undefined || this.failureLine !== undefined) {
        outcome = 'exited';
        reporter.clear();
        return {
          outcome,
          startHeight,
          height,
          nodeTip,
          caughtUp,
          elapsedMs: Date.now() - startedAtMs,
          exit,
          mismatch: this.mismatch,
          logTail: this.logTail(),
        };
      }

      if (caughtUp) {
        outcome = 'caught-up';
        break;
      }
      const synced = height === undefined ? undefined : height - startHeight;
      if (maxBlocks !== UNBOUNDED_MAX_BLOCKS && synced !== undefined && synced >= maxBlocks) {
        outcome = 'budget-reached';
        break;
      }
      if (Date.now() - startedAtMs >= maxDurationMs) {
        outcome = 'timed-out';
        break;
      }

      await sleep(POLL_INTERVAL_MS);
    }

    reporter.clear();
    console.log(`[SYNC] ${outcome}: ${reporter.summary(nodeTip)}`);

    return {
      outcome,
      startHeight,
      height,
      nodeTip,
      caughtUp,
      elapsedMs: Date.now() - startedAtMs,
      mismatch: this.mismatch,
      logTail: this.logTail(),
    };
  }

  /** Remove the stack, but only if this instance brought it up. */
  async stop(): Promise<void> {
    this.logStream?.kill('SIGTERM');
    this.logStream = undefined;

    if (!this.startedByUs) return;
    const result = spawnSync('docker', this.composeArgs(['down', '-v', '--remove-orphans']), {
      env: this.composeEnv(),
      stdio: 'inherit',
    });
    if (result.status !== 0) {
      // Best effort: a failing teardown must not mask the test result.
      console.warn(`[SYNC] docker compose down exited with status ${result.status}.`);
    }
  }

  /** Readiness as the API reports it: 200 once caught up, 503 while still syncing. */
  async readyStatus(): Promise<number | undefined> {
    try {
      const response = await fetch(`http://localhost:${this.options.apiPort}/ready`, {
        signal: AbortSignal.timeout(METRICS_TIMEOUT_MS),
      });
      return response.status;
    } catch {
      return undefined;
    }
  }

  private composeArgs(extra: string[]): string[] {
    return [
      'compose',
      '-p',
      this.project,
      '--profile',
      this.options.topology,
      '-f',
      this.composeFile,
      ...extra,
    ];
  }

  private composeEnv(): Record<string, string | undefined> {
    return {
      ...process.env,
      ...this.secrets,
      SYNC_INDEXER_TAG: this.options.indexerTag,
      SYNC_IMAGE_REGISTRY: this.options.imageRegistry,
      SYNC_NODE_URL: this.options.nodeUrl,
      SYNC_NETWORK_ID: this.options.networkId,
      SYNC_API_PORT: String(this.options.apiPort),
      SYNC_METRICS_PORT: String(this.options.metricsPort),
    };
  }

  private followLogs(): void {
    this.logStream = spawn('docker', this.composeArgs(['logs', '-f', '--no-color']), {
      env: this.composeEnv(),
      stdio: ['ignore', 'pipe', 'pipe'],
    });

    const consume = (chunk: Buffer) => {
      for (const line of chunk.toString().split('\n')) {
        if (line.trim() === '') continue;
        this.logBuffer.push(line);
        if (this.logBuffer.length > LOG_BUFFER_LINES) this.logBuffer.shift();
        this.inspectLine(line);
      }
    };

    this.logStream.stdout?.on('data', consume);
    this.logStream.stderr?.on('data', consume);
    this.logStream.on('error', () => undefined);
  }

  private inspectLine(line: string): void {
    if (this.failureLine === undefined && FAILURE_PATTERN.test(line)) {
      this.failureLine = line;
    }
    const match = MISMATCH_PATTERN.exec(line);
    if (match !== null && this.mismatch === undefined) {
      this.mismatch = {
        kind: `${match[1].toLowerCase()} state root`,
        block: match[2],
        height: Number(match[3]),
        line,
      };
    }
  }

  private async scrapeMetrics(): Promise<IndexerMetrics> {
    try {
      const response = await fetch(`http://localhost:${this.options.metricsPort}/metrics`, {
        signal: AbortSignal.timeout(METRICS_TIMEOUT_MS),
      });
      if (!response.ok) return {};
      return parseIndexerMetrics(await response.text());
    } catch {
      // The endpoint is not up yet, or the container is gone. Either way the caller
      // treats the absence as "not known yet".
      return {};
    }
  }

  private async chainTip(): Promise<number | undefined> {
    return await this.rpc.getChainTip().catch(() => undefined);
  }

  /**
   * The first indexer service that exited non-zero, if any. Postgres and NATS are
   * `restart: always`, so only the indexer containers can be found stopped here.
   */
  private exitedService(): ServiceExit | undefined {
    const result = spawnSync('docker', this.composeArgs(['ps', '--all', '--format', 'json']), {
      env: this.composeEnv(),
      encoding: 'utf8',
    });
    if (result.status !== 0 || !result.stdout) return undefined;

    for (const line of result.stdout.split('\n')) {
      if (line.trim() === '') continue;
      let entry: { Service?: string; State?: string; ExitCode?: number };
      try {
        entry = JSON.parse(line);
      } catch {
        continue;
      }
      if (entry.State === 'exited' && typeof entry.ExitCode === 'number' && entry.ExitCode !== 0) {
        return { service: entry.Service ?? 'unknown', exitCode: entry.ExitCode };
      }
    }
    return undefined;
  }

  private logTail(): string[] {
    return this.logBuffer.slice(-LOG_TAIL_LINES);
  }
}
