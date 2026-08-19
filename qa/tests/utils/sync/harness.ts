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
import { createServer } from 'node:net';
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
const DEFAULT_POSTGRES_PORT = 5433;
const DEFAULT_NATS_PORT = 4422;
const POLL_INTERVAL_MS = 5_000;
/** Live-line redraw cadence, so the spinner animates between polls. */
const SPINNER_REFRESH_MS = 120;
const METRICS_TIMEOUT_MS = 5_000;
// The buffer must comfortably outlast one poll interval: three containers on debug
// logging emit tens of lines a second, and a 40-line tail was measured spanning under
// two seconds - short enough to evict the very line that explains a failure.
const LOG_BUFFER_LINES = 4000;
const LOG_TAIL_LINES = 80;
/** Matched bail-out lines are kept apart from the tail so they can never be evicted. */
const MAX_FAILURE_LINES = 20;

/**
 * Divergence bail-outs. Kept identical to the set `qa/scripts/test-hardfork-8to9.sh`
 * greps for, so both surfaces agree on what counts as a divergence.
 */
const DIVERGENCE_PATTERN =
  /ledger state root mismatch|zswap state root mismatch|translate ledger state/i;

/**
 * The line every indexer binary logs on its way out (see each crate's `main.rs`),
 * carrying the fatal error in its `error` field.
 *
 * Matching on it as well as on the divergence phrases matters: an incompatible image
 * can bail out for reasons no phrase list enumerates - a wrong network id, for one,
 * fails with "malformed transaction: invalid network ID" and matches none of the
 * divergence patterns. Without this the run would report only an exit code.
 */
const FATAL_PATTERN = /process exited with ERROR/;

/**
 * Pulls the offending block out of a mismatch line, matching the format strings in
 * `chain-indexer/src/application.rs`. The height is the product of this whole suite:
 * it localises a divergence to a single block.
 */
const MISMATCH_PATTERN = /(ledger|zswap) state root mismatch for block (\S+) at height (\d+)/i;

/** True when a log line reports a bail-out worth failing and reporting on. */
export function isBailOutLine(line: string): boolean {
  return FATAL_PATTERN.test(line) || DIVERGENCE_PATTERN.test(line);
}

/** The offending block named by a state root mismatch line, if it is one. */
export function parseStateRootMismatch(line: string): StateRootMismatch | undefined {
  const match = MISMATCH_PATTERN.exec(line);
  if (match === null) return undefined;
  return {
    kind: `${match[1].toLowerCase()} state root`,
    block: match[2],
    height: Number(match[3]),
    line,
  };
}

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
  postgresPort: number;
  natsPort: number;
  progressMode: ProgressMode;
}

/** `failed` covers both a non-zero container exit and a bail-out line in the logs. */
export type SyncOutcome = 'caught-up' | 'budget-reached' | 'timed-out' | 'failed';

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
  /** The bail-out line that ended the run, when one was matched in the logs. */
  failure?: string;
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

/**
 * Parse an integer option with an explicit floor. Only `MAX_BLOCKS` accepts 0, where it
 * is the documented "unbounded" sentinel; a 0 duration would time a run out instantly
 * and a 0 port would bind a random one the harness could then never scrape.
 */
function parseIntOption(
  raw: string | undefined,
  fallback: number,
  name: string,
  min: number,
): number {
  if (raw === undefined || raw.trim() === '') return fallback;
  const parsed = Number(raw);
  if (!Number.isInteger(parsed) || parsed < min) {
    throw new Error(`${name} must be an integer >= ${min}, got: ${JSON.stringify(raw)}`);
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
  assertSyncEnvironmentSupported();
  // Single source of truth for preconditions: the suite skips on this, and a direct
  // caller gets the same explanation as an error rather than a second, divergent check.
  const gap = syncPrerequisiteGap();
  if (gap !== undefined) {
    throw new Error(`Cannot run a sync: ${gap}.`);
  }
  const indexerTag = process.env.SYNC_INDEXER_TAG!.trim();

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
    // 0 is the documented "unbounded" sentinel here, and only here.
    maxBlocks: parseIntOption(process.env.MAX_BLOCKS, DEFAULT_MAX_BLOCKS, 'MAX_BLOCKS', 0),
    maxDurationMs: parseIntOption(
      process.env.MAX_DURATION_MS,
      DEFAULT_MAX_DURATION_MS,
      'MAX_DURATION_MS',
      1000,
    ),
    apiPort: parseIntOption(process.env.SYNC_API_PORT, DEFAULT_API_PORT, 'SYNC_API_PORT', 1),
    metricsPort: parseIntOption(
      process.env.SYNC_METRICS_PORT,
      DEFAULT_METRICS_PORT,
      'SYNC_METRICS_PORT',
      1,
    ),
    postgresPort: parseIntOption(
      process.env.SYNC_POSTGRES_PORT,
      DEFAULT_POSTGRES_PORT,
      'SYNC_POSTGRES_PORT',
      1,
    ),
    natsPort: parseIntOption(process.env.SYNC_NATS_PORT, DEFAULT_NATS_PORT, 'SYNC_NATS_PORT', 1),
    progressMode: process.env.SYNC_PROGRESS === 'live' ? 'live' : 'plain',
  };
}

/**
 * Reject an environment this suite cannot serve.
 *
 * A hard error rather than a skip: asking for an environment the suite cannot index
 * deserves to fail, not to pass quietly having tested nothing. It lives here rather than
 * in the project config because that config is resolved on every Vitest invocation, so
 * throwing there breaks suites that never asked for the sync project.
 *
 * `mainnet` cannot be caught here at all: it has no host entry, so `environment/model`
 * throws while being imported, before any of this runs - exactly as it does for every
 * other suite in the framework.
 */
export function assertSyncEnvironmentSupported(): void {
  if (env.isUndeployedEnv()) {
    throw new Error(
      'TARGET_ENV=undeployed is not supported by the sync suite: its node URL is ' +
        'localhost, which inside the indexer container is the container itself. Point ' +
        'the suite at a deployed chain, or reach the node over the host gateway.',
    );
  }
}

/**
 * Why a sync cannot run here, or undefined when it can. These are the gaps a developer
 * can close, so the suite skips on them and says so rather than failing.
 *
 * Unsupported values of `TARGET_ENV` are deliberately NOT reported here: they are a hard
 * error from `assertSyncEnvironmentSupported()`, not something to skip over.
 */
export function syncPrerequisiteGap(): string | undefined {
  if (!process.env.SYNC_INDEXER_TAG?.trim()) {
    return 'SYNC_INDEXER_TAG is not set, so there is no indexer version to test';
  }
  if (spawnSync('docker', ['compose', 'version'], { stdio: 'ignore' }).status !== 0) {
    return 'docker compose is not available';
  }
  return undefined;
}

/** True when nothing is listening on the given host port. */
async function isPortFree(port: number): Promise<boolean> {
  return await new Promise((resolve) => {
    const server = createServer();
    server.once('error', () => resolve(false));
    server.once('listening', () => server.close(() => resolve(true)));
    server.listen(port, '127.0.0.1');
  });
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
  private readonly failureLines: string[] = [];
  private startedByUs = false;
  private logStream?: ChildProcess;
  private mismatch?: StateRootMismatch;
  /** Torn down on SIGINT/SIGTERM so an interrupted run does not leak its stack. */
  private readonly onSignal = (signal: string) => {
    console.warn(`\n[SYNC] ${signal} received - tearing down the stack.`);
    this.teardown();
    process.exit(130);
  };

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

    await this.assertPortsFree();

    process.on('SIGINT', this.onSignal);
    process.on('SIGTERM', this.onSignal);

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

    // The poll interval is far longer than a spinner frame, so the live line is
    // redrawn between polls to stay animated. No-op in plain mode.
    const animation = setInterval(() => reporter.refresh(nodeTip), SPINNER_REFRESH_MS);

    try {
      for (;;) {
        const metrics = await this.scrapeMetrics();
        height = metrics.blockHeight ?? height;
        caughtUp = metrics.caughtUp ?? false;
        // Prefer the tip the indexer itself sees; fall back to the node directly while
        // the metrics endpoint is still silent.
        nodeTip = metrics.nodeBlockHeight ?? (await this.chainTip()) ?? nodeTip;

        reporter.update(height, nodeTip);

        const exit = this.exitedService();
        const failure = this.failureLines[0];
        if (exit !== undefined || failure !== undefined) {
          outcome = 'failed';
          reporter.clear();
          console.log(`[SYNC] ${outcome}: ${reporter.summary(nodeTip)}`);
          return {
            outcome,
            startHeight,
            height,
            nodeTip,
            caughtUp,
            elapsedMs: Date.now() - startedAtMs,
            exit,
            mismatch: this.mismatch,
            failure,
            logTail: this.logTail(exit?.service),
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
    } finally {
      clearInterval(animation);
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
      failure: this.failureLines[0],
      logTail: this.logTail(),
    };
  }

  /** Remove the stack, but only if this instance brought it up. */
  async stop(): Promise<void> {
    process.off('SIGINT', this.onSignal);
    process.off('SIGTERM', this.onSignal);
    this.teardown();
  }

  /** Remove the stack, synchronously, so it is also usable from a signal handler. */
  private teardown(): void {
    this.logStream?.kill('SIGTERM');
    this.logStream = undefined;

    if (!this.startedByUs) return;
    this.startedByUs = false;
    const result = spawnSync('docker', this.composeArgs(['down', '-v', '--remove-orphans']), {
      env: this.composeEnv(),
      stdio: 'inherit',
    });
    if (result.status !== 0) {
      // Best effort: a failing teardown must not mask the test result.
      console.warn(`[SYNC] docker compose down exited with status ${result.status}.`);
    }
  }

  /**
   * Fail fast on a port clash. The host ports are fixed, so a leaked stack from an
   * interrupted run otherwise surfaces only as `up exited with status 1`.
   */
  private async assertPortsFree(): Promise<void> {
    const { topology, apiPort, metricsPort, postgresPort, natsPort } = this.options;
    const ports =
      topology === 'cloud'
        ? {
            SYNC_API_PORT: apiPort,
            SYNC_METRICS_PORT: metricsPort,
            SYNC_POSTGRES_PORT: postgresPort,
            SYNC_NATS_PORT: natsPort,
          }
        : { SYNC_API_PORT: apiPort, SYNC_METRICS_PORT: metricsPort };

    const taken: string[] = [];
    for (const [name, port] of Object.entries(ports)) {
      if (!(await isPortFree(port))) taken.push(`${port} (${name})`);
    }
    if (taken.length > 0) {
      throw new Error(
        `[SYNC] Host port(s) already in use: ${taken.join(', ')}. Another stack is running - ` +
          'remove it (`docker compose ls`) or point the suite at different ports.',
      );
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
      SYNC_POSTGRES_PORT: String(this.options.postgresPort),
      SYNC_NATS_PORT: String(this.options.natsPort),
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
    if (isBailOutLine(line) && this.failureLines.length < MAX_FAILURE_LINES) {
      this.failureLines.push(line);
    }
    this.mismatch ??= parseStateRootMismatch(line);
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

  /**
   * Recent log lines for the report, narrowed to the failing service when one is known
   * so a noisy sibling container cannot crowd out the relevant output.
   */
  private logTail(service?: string): string[] {
    // A bail-out is usually seen in the log stream before the container is observed to
    // have exited, so fall back to the service that logged it: `docker compose logs`
    // prefixes every line with "<service>-<n>  | ".
    const source = service ?? this.failureLines[0]?.match(/^(\S+?)-\d+\s+\|/)?.[1];
    const lines =
      source === undefined
        ? this.logBuffer
        : this.logBuffer.filter((line) => line.startsWith(source));
    const tail = (lines.length > 0 ? lines : this.logBuffer).slice(-LOG_TAIL_LINES);
    // Matched bail-out lines are prepended: they are the point of the report and are
    // held outside the evictable buffer precisely so they survive a busy tail.
    const missing = this.failureLines.filter((line) => !tail.includes(line));
    return [...missing, ...tail];
  }
}
