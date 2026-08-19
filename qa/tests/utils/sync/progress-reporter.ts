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

/**
 * Block budget sentinel meaning "no bound — keep syncing until the chain tip".
 *
 * `MAX_BLOCKS=0` does NOT mean "index zero blocks". It is the explicit opt-in
 * to an unbounded run, where progress and the remaining-time estimate are
 * measured against the live chain tip instead of a fixed budget. `MAX_DURATION_MS`
 * is the only bound on such a run.
 */
export const UNBOUNDED_MAX_BLOCKS = 0;

/** Window the short-term rate is averaged over. */
const WINDOW_MS = 30_000;

/** How often the non-interactive mode emits a line. */
export const PLAIN_INTERVAL_MS = 300_000;

/** Minimum delay between spinner frame advances. */
const SPINNER_FRAME_MS = 100;

const SPINNER_FRAMES = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

export type ProgressMode = 'live' | 'plain';

/** One observation of the indexer's height. */
export interface HeightSample {
  atMs: number;
  height: number;
}

export interface ProgressOptions {
  /** Height the indexer was at when the run started (0 for a fresh sync). */
  startHeight: number;
  /** Block budget, or `UNBOUNDED_MAX_BLOCKS` to sync to the chain tip. */
  maxBlocks: number;
  /** Wall-clock start of the run, so the overall rate covers the whole run. */
  startedAtMs: number;
}

export interface ProgressStats {
  /** Latest reported height, or undefined while the indexer has reported none. */
  height?: number;
  /** Latest known chain tip, or undefined if it could not be read. */
  tip?: number;
  /** Blocks indexed since the run started. */
  synced: number;
  /** Blocks the run is expected to index in total, if that is knowable. */
  total?: number;
  percent?: number;
  /** Blocks per second across the whole run. */
  overallRate: number;
  /** Blocks per second over the last 30 seconds. */
  windowRate: number;
  etaMs?: number;
}

/**
 * Height the run is aiming at: the budget's end for a bounded run, the chain tip
 * for an unbounded one. Undefined when unbounded and the tip is not yet known.
 */
function resolveTarget(tip: number | undefined, options: ProgressOptions): number | undefined {
  if (options.maxBlocks !== UNBOUNDED_MAX_BLOCKS) {
    return options.startHeight + options.maxBlocks;
  }
  return tip;
}

/**
 * Derive rates, progress and a remaining-time estimate from height observations.
 *
 * Pure: every input is explicit, so the maths is testable without a running
 * indexer. A missing height (the indexer has not reported one yet) yields zero
 * rates rather than pretending the height is 0 — the metrics endpoint publishes
 * nothing until the first block batch is processed.
 */
export function computeProgress(
  samples: readonly HeightSample[],
  tip: number | undefined,
  options: ProgressOptions,
): ProgressStats {
  const target = resolveTarget(tip, options);
  const total = target === undefined ? undefined : Math.max(target - options.startHeight, 0);

  const latest = samples.at(-1);
  if (latest === undefined) {
    return { tip, synced: 0, total, overallRate: 0, windowRate: 0 };
  }

  const synced = Math.max(latest.height - options.startHeight, 0);
  const overallSeconds = (latest.atMs - options.startedAtMs) / 1000;
  const overallRate = overallSeconds > 0 ? synced / overallSeconds : 0;

  // Oldest sample still inside the window; fall back to the last two samples so a
  // slow poll (fewer than two samples per window) still yields a short-term rate.
  const windowStart = latest.atMs - WINDOW_MS;
  const inWindow = samples.filter((sample) => sample.atMs >= windowStart);
  const window = inWindow.length >= 2 ? inWindow : samples.slice(-2);
  const oldest = window[0];
  const windowSeconds = (latest.atMs - oldest.atMs) / 1000;
  const windowRate = windowSeconds > 0 ? (latest.height - oldest.height) / windowSeconds : 0;

  // A single poll can carry the run past its budget, so synced may exceed total.
  // The overshoot is real and stays visible in `synced`; the percentage is capped.
  const percent =
    total !== undefined && total > 0 ? Math.min((synced / total) * 100, 100) : undefined;
  const remaining = target === undefined ? undefined : Math.max(target - latest.height, 0);
  const etaMs =
    remaining !== undefined && overallRate > 0 ? (remaining / overallRate) * 1000 : undefined;

  return { height: latest.height, tip, synced, total, percent, overallRate, windowRate, etaMs };
}

/** Format a duration the way an operator reads it: `3h 28m`, `12m 04s`, `45s`. */
export function formatDuration(ms: number): string {
  const totalSeconds = Math.max(Math.round(ms / 1000), 0);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;

  if (hours > 0) return `${hours}h ${String(minutes).padStart(2, '0')}m`;
  if (minutes > 0) return `${minutes}m ${String(seconds).padStart(2, '0')}s`;
  return `${seconds}s`;
}

/** The four fields the run reports: progress, overall rate, 30s rate, ETA. */
export function formatProgressLine(stats: ProgressStats): string {
  if (stats.height === undefined) {
    return 'waiting for the indexer to report a height...';
  }

  const progress =
    stats.total === undefined
      ? `blocks synced: ${stats.synced} (chain tip unknown)`
      : `blocks synced: ${stats.synced}/${stats.total} (${stats.percent?.toFixed(0) ?? '0'}%)`;

  return [
    progress,
    `${stats.overallRate.toFixed(1)} blocks/s overall`,
    `${stats.windowRate.toFixed(1)} blocks/s (30s)`,
    `ETA: ${stats.etaMs === undefined ? 'unknown' : formatDuration(stats.etaMs)}`,
  ].join(' | ');
}

/**
 * Reports sync progress to the console in one of two modes.
 *
 * - `live`: a single `\r`-rewritten line with a spinner, for a terminal.
 * - `plain`: one plain line immediately, then every five minutes, then one at the
 *   end. A long-running sync must never go silent on a CI log.
 *
 * Writes to stdout directly rather than through the pino logger: pino sends test
 * output to per-run files under `logs/<timestamp>/`, where progress would be
 * invisible for the entire run.
 */
export class SyncProgressReporter {
  private readonly samples: HeightSample[] = [];
  private readonly mode: ProgressMode;
  private frame = 0;
  private frameAtMs = 0;
  private plainAtMs = 0;
  private plainEmitted = false;
  private plainHadHeight = false;
  private liveWidth = 0;

  constructor(
    private readonly options: ProgressOptions,
    mode: ProgressMode,
  ) {
    this.mode = mode;
  }

  /** Record an observation and render it if the current mode is due an update. */
  update(height: number | undefined, tip: number | undefined, nowMs: number = Date.now()): void {
    if (height !== undefined) {
      this.samples.push({ atMs: nowMs, height });
      // Two windows is all the history the maths needs.
      const cutoff = nowMs - WINDOW_MS * 2;
      while (this.samples.length > 2 && this.samples[0].atMs < cutoff) this.samples.shift();
    }

    const stats = this.stats(tip);
    const line = formatProgressLine(stats);
    if (this.mode === 'live') {
      this.writeLive(line, nowMs);
      return;
    }

    // Emit on the first update, again on the first update that carries a height (the
    // metrics endpoint is silent until the first block batch), then on the interval.
    const firstHeight = !this.plainHadHeight && stats.height !== undefined;
    if (!this.plainEmitted || firstHeight || nowMs - this.plainAtMs >= PLAIN_INTERVAL_MS) {
      this.plainEmitted = true;
      this.plainHadHeight ||= stats.height !== undefined;
      this.plainAtMs = nowMs;
      console.log(`[SYNC] ${line}`);
    }
  }

  /** Erase the live line. Must be called before any final or error output. */
  clear(): void {
    if (this.mode !== 'live' || this.liveWidth === 0) return;
    process.stdout.write(`\r${' '.repeat(this.liveWidth)}\r`);
    this.liveWidth = 0;
  }

  /** Final one-line summary, for the caller to log after `clear()`. */
  summary(tip: number | undefined, nowMs: number = Date.now()): string {
    const stats = this.stats(tip);
    const elapsed = formatDuration(nowMs - this.options.startedAtMs);
    return `${formatProgressLine(stats)} | elapsed: ${elapsed}`;
  }

  stats(tip: number | undefined): ProgressStats {
    return computeProgress(this.samples, tip, this.options);
  }

  private writeLive(line: string, nowMs: number): void {
    if (nowMs - this.frameAtMs >= SPINNER_FRAME_MS) {
      this.frame = (this.frame + 1) % SPINNER_FRAMES.length;
      this.frameAtMs = nowMs;
    }
    const rendered = `${SPINNER_FRAMES[this.frame]} ${line}`;
    process.stdout.write(`\r${rendered.padEnd(this.liveWidth)}`);
    this.liveWidth = Math.max(this.liveWidth, rendered.length);
  }
}
