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

// Shared constants and helpers for the SPO (stake pool operator) indexer
// integration suites (#1003): spo-queries.test.ts (governance, epoch and
// committee data that is live on permissioned environments) and
// spo-registration-queries.test.ts (registration, performance and stake data
// that stays empty until post-mainnet registration tooling exists).

import log from '@utils/logging/logger';
import { EnvironmentName, env } from 'environment/model';
import type { TestContext } from 'vitest';
import type { z } from 'zod';
import type { IndexerHttpClient } from '@utils/indexer/http-client';
import type { CommitteeMember, GraphQLResponse } from '@utils/indexer/indexer-types';
import { fetchQueryFieldNames } from '@utils/indexer/schema-feature-probe';

// A well-formed (56 hex chars, 28-byte) Cardano pool id that is not registered anywhere.
export const FABRICATED_POOL_ID = 'deadbeef'.repeat(7);
// A well-formed (64 hex chars, 32-byte) sidechain / SPO key that is not registered anywhere.
export const FABRICATED_SPO_SK = 'cafebabe'.repeat(8);
export const MALFORMED_POOL_IDS = ['', 'not-a-pool-id', 'abc', 'deadbeef'.repeat(7) + 'ff'];
export const MAX_GRAPHQL_INT = 2_147_483_647;
// The widest fromEpoch..toEpoch span the series resolvers accept, mirroring
// MAX_EPOCH_SPAN in indexer-api/src/infra/api/v4/query.rs (#1455). The cap
// itself is covered by spo-epoch-series-queries.test.ts.
export const MAX_EPOCH_SPAN = 10_000;
// How far below the current epoch to look for committee data. Epochs are 30
// minutes on current environments, so 48 covers a day of spo-indexer lag.
export const COMMITTEE_LOOKBACK_EPOCHS = 48;
// Committee lookback requests issued concurrently per round; bounded so a shared
// environment is not hit with the whole window at once.
export const COMMITTEE_LOOKBACK_CHUNK = 12;

// Resolver defaults / clamps, see indexer-api/src/infra/api/v4/query.rs. Limits
// are clamped silently into 1..max, never rejected, so the clamp probes assert
// the exact row count that clamp implies given the rows available.
export const SPO_LIST_DEFAULT_LIMIT = 20;
export const SPO_LIST_MAX_LIMIT = 200;
export const SPO_IDENTITIES_DEFAULT_LIMIT = 50;
export const SPO_IDENTITIES_MAX_LIMIT = 500;
export const STAKE_POOL_OPERATORS_DEFAULT_LIMIT = 20;
export const STAKE_POOL_OPERATORS_MAX_LIMIT = 100;
export const POOL_METADATA_LIST_DEFAULT_LIMIT = 50;
export const SPO_PERFORMANCE_LATEST_DEFAULT_LIMIT = 20;
export const SPO_PERFORMANCE_BY_SK_DEFAULT_LIMIT = 100;
export const EPOCH_PERFORMANCE_DEFAULT_LIMIT = 100;
export const STAKE_DISTRIBUTION_DEFAULT_LIMIT = 50;
// Shared upper clamp of poolMetadataList, spoIdentities, the performance
// lists and stakeDistribution.
export const WIDE_LIST_MAX_LIMIT = 500;
// Out-of-range limits every list endpoint must clamp rather than reject: zero
// clamps up to a single row, the large value down to the endpoint's maximum.
export const CLAMP_PROBE_LIMITS = [0, 10_000];

/**
 * Every root Query field of the SPO surface. All of them ship in every indexer
 * release the QA suites support (4.0.0 onwards), so a missing one is a
 * regression of the API and must fail a run, never skip it.
 */
export const SPO_QUERY_FIELDS = [
  'dParameterHistory',
  'termsAndConditionsHistory',
  'currentEpochInfo',
  'committee',
  'registeredSpoSeries',
  'registeredPresence',
  'registeredTotalsSeries',
  'registeredFirstValidEpochs',
  'spoCount',
  'spoList',
  'spoIdentities',
  'stakePoolOperators',
  'spoByPoolId',
  'spoIdentityByPoolId',
  'spoCompositeByPoolId',
  'poolMetadata',
  'poolMetadataList',
  'spoPerformanceLatest',
  'spoPerformanceBySpoSk',
  'epochPerformance',
  'epochUtilization',
  'stakeDistribution',
];

/**
 * Environments that run spo-indexer and must therefore serve epoch and
 * committee data. On these a missing epoch or committee is a failure of the
 * system under test; everywhere else (qanet runs no spo-indexer, the
 * undeployed stack has only just started backfilling) the tests that need
 * that data skip with a reason. Extend this set when spo-indexer is deployed
 * to another environment. preprod is absent because its indexer was
 * decommissioned on 2026-10-09.
 */
export const SPO_DATA_ENVIRONMENTS: ReadonlySet<EnvironmentName> = new Set([
  EnvironmentName.DEVNET,
  EnvironmentName.PREVIEW,
]);

/** Whether the current environment is one that must have spo-indexer data. */
export function spoDataRequired(): boolean {
  return SPO_DATA_ENVIRONMENTS.has(env.getCurrentEnvironmentName());
}

/**
 * Asserts that the deployed schema serves every SPO root Query field. For use
 * in `beforeAll`: throws with the missing fields so a removed or renamed field
 * fails the whole suite instead of narrowing it.
 */
export async function assertSpoQueryFieldsServed(): Promise<void> {
  const served = await fetchQueryFieldNames();
  const missing = SPO_QUERY_FIELDS.filter((field) => !served.has(field));
  if (missing.length > 0) {
    throw new Error(
      `SPO Query fields missing from the schema on ${env.getCurrentEnvironmentName()}: ${missing.join(', ')}`,
    );
  }
}

/**
 * Records in `beforeAll` that SPO data the suite depends on is absent. On an
 * environment listed in SPO_DATA_ENVIRONMENTS that is a failure of the system
 * under test and throws; elsewhere it is the expected state, so it is logged
 * and the tests that need the data skip with `reason`.
 */
export function reportMissingSpoData(reason: string): void {
  if (spoDataRequired()) {
    throw new Error(
      `${reason}, but ${env.getCurrentEnvironmentName()} runs spo-indexer and must have this data (SPO_DATA_ENVIRONMENTS)`,
    );
  }
  log.warn(`${reason}; the tests that need it skip on ${env.getCurrentEnvironmentName()}`);
}

/** The row bound a resolver applies to `limit` given its maximum. */
export function clampedLimit(limit: number, maxLimit: number): number {
  return Math.min(Math.max(limit, 1), maxLimit);
}

/** The inclusive ascending epoch range `from..to`. */
export function epochRange(from: number, to: number): number[] {
  return Array.from({ length: to - from + 1 }, (_, i) => from + i);
}

/**
 * Asserts every adjacent pair of `items` satisfies `inOrder(previous, current)`.
 * The failure message names both offending items so a single out-of-order row
 * in a long list can be found without re-running.
 */
export function expectOrdered<T>(
  items: T[],
  inOrder: (previous: T, current: T) => boolean,
  label: string,
): void {
  items.slice(1).forEach((current, i) => {
    const previous = items[i];
    expect(
      inOrder(previous, current),
      `${label}: item ${i + 1} ${JSON.stringify(current)} should not sort before item ${i} ${JSON.stringify(previous)}`,
    ).toBe(true);
  });
}

/**
 * Asserts that `items` is an array whose every element parses with `schema`,
 * and returns it typed. Empty arrays pass: the registration surface is empty on
 * every environment today and these checks must stay valid once data appears.
 */
export function expectValidList<T>(items: unknown, schema: z.ZodType<T>, label: string): T[] {
  expect(Array.isArray(items), `${label} should be an array`).toBe(true);
  for (const item of items as unknown[]) {
    const parsed = schema.safeParse(item);
    expect(
      parsed.success,
      `${label} item failed schema validation ${JSON.stringify(parsed.error, null, 2)}`,
    ).toBe(true);
  }
  return items as T[];
}

/**
 * A bounded sample of `items` for per-row schema checks on very long lists:
 * the first and last items plus evenly spaced ones between them, `count` in
 * total (at least two). Lists no longer than `count` are returned whole, so
 * short lists are still checked exhaustively.
 */
export function sampleRows<T>(items: T[], count: number): T[] {
  const size = Math.max(count, 2);
  if (items.length <= size) return items;
  const step = (items.length - 1) / (size - 1);
  return Array.from({ length: size }, (_, index) => items[Math.round(index * step)]);
}

/**
 * The spellings of a hex identifier that the pool-id and SPO-key resolvers
 * actually normalise to the canonical lowercase, unprefixed form: an upper-case
 * `0X` prefix and upper-case digits (query.rs `normalize_hex`). Each of these
 * must resolve identically to the canonical spelling.
 */
export function hexNormalisedVariants(hex: string): string[] {
  return [`0X${hex}`, hex.toUpperCase()];
}

/**
 * The lowercase `0x` prefixed spellings of a hex identifier. `normalize_hex`
 * (query.rs, and its twin in storage/spo.rs) chains two `strip_prefix` calls
 * whose second `unwrap_or` falls back to the original input rather than the
 * stripped intermediate, so a lowercase `0x` prefix survives and the lookup
 * runs against the literal `0x...` string, which never matches a stored id.
 * Tracked as midnightntwrk/midnight-indexer#1712.
 */
export function hexLowercasePrefixVariants(hex: string): string[] {
  return [`0x${hex}`, `0x${hex.toUpperCase()}`];
}

/**
 * Describes an endpoint whose ORDER BY has no unique tiebreaker: `limit` is
 * the page size the comparison was fetched with and `orderKey` reproduces the
 * ORDER BY columns for a row, so rows with equal keys are the ones Postgres may
 * return in any order.
 */
export type OrderTie<T> = { limit: number; orderKey: (row: T) => string };

/** One independently fetched page of a list endpoint, named for messages. */
export type Page<T> = { label: string; rows: T[] };

/**
 * Normalises one independently fetched page of a list so two such pages can be
 * deep-compared. Rows are sorted by their JSON form, because two calls to the
 * same query are free to return rows tied on the ORDER BY in different orders.
 * When `tie` is given the ordering is not unique, and a full page may then also
 * differ in which tied rows fall inside it, so the trailing group of rows tied
 * on `orderKey` is dropped from a full page before sorting: every row whose key
 * sorts strictly before that boundary is guaranteed to be on both pages.
 *
 * Returns null when a full page is a single tie group (say 20 performance rows
 * all with zero produced blocks): no row on it is guaranteed to be on the other
 * page, so there is nothing to compare. That is a property of the data, not of
 * the indexer, which is why `expectSamePage` skips rather than fails on it.
 */
export function comparableRows<T>(rows: T[], tie?: OrderTie<T>): T[] | null {
  const kept = tie && rows.length >= tie.limit ? withoutTrailingTies(rows, tie.orderKey) : rows;
  if (kept.length === 0 && rows.length > 0) return null;
  return [...kept].sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)));
}

function withoutTrailingTies<T>(rows: T[], orderKey: (row: T) => string): T[] {
  const boundary = orderKey(rows[rows.length - 1]);
  return rows.filter((row) => orderKey(row) !== boundary);
}

/**
 * Asserts two independently fetched pages of the same list hold the same rows,
 * compared order-insensitively through `comparableRows`. When either page is a
 * full page made of one tie group the test is skipped with a reason naming the
 * page and its tie key, since nothing on it can be compared.
 */
export function expectSamePage<T>(
  ctx: TestContext,
  actual: Page<T>,
  expected: Page<T>,
  tie?: OrderTie<T>,
): void {
  const pages = [actual, expected].map((page) => ({
    ...page,
    kept: comparableRows(page.rows, tie),
  }));
  const tied = pages.find((page) => page.kept === null);
  if (tied) {
    return skipWithReason(
      ctx,
      `${tied.label}: all ${tied.rows.length} rows of the page tie on ORDER BY key ${tie!.orderKey(tied.rows[tied.rows.length - 1])}, nothing is guaranteed to be on both pages`,
    );
  }
  expect(pages[0].kept, `${actual.label} should hold the rows of ${expected.label}`).toEqual(
    pages[1].kept,
  );
}

/**
 * Skips the current test with a reason that shows up in the report.
 *
 * `ctx.skip` throws to abort the rest of the test body, which is what lets
 * call sites write `if (!gate) return skipWithReason(ctx, reason)` and rely on
 * nothing after it running. It is deliberately not an optional call: with so
 * much of the SPO suite gated behind these skips, a context without `skip`
 * must fail loudly with a TypeError here rather than return normally and let
 * the test pass green having asserted nothing.
 */
export function skipWithReason(ctx: TestContext, reason: string): void {
  ctx.skip(true, reason);
}

/**
 * Throws when a response carries GraphQL errors or no data. For use inside
 * `beforeAll`: a probe that fails must fail the suite loudly rather than be
 * read as "no data here" and turn every test into a skip.
 */
export function assertNoGraphqlErrors(label: string, response: GraphQLResponse<unknown>): void {
  if (response.errors?.length) {
    throw new Error(
      `${label} failed on ${env.getCurrentEnvironmentName()}: ${JSON.stringify(response.errors)}`,
    );
  }
  if (response.data == null) {
    throw new Error(`${label} returned no data on ${env.getCurrentEnvironmentName()}`);
  }
}

export interface KnownCommittee {
  epoch: number;
  members: CommitteeMember[];
}

/**
 * Finds the most recent epoch at or below `currentEpoch` that has committee
 * data, scanning `lookback` epochs downwards.
 *
 * Returns null when no epoch in the window has a committee. Throws on GraphQL
 * errors: this runs in `beforeAll` and an outage must not read as "no data".
 */
export async function findLatestCommitteeEpoch(
  client: IndexerHttpClient,
  currentEpoch: number,
  lookback = COMMITTEE_LOOKBACK_EPOCHS,
  chunkSize = COMMITTEE_LOOKBACK_CHUNK,
): Promise<KnownCommittee | null> {
  const oldest = Math.max(0, currentEpoch - lookback);
  const candidates = epochRange(oldest, currentEpoch).reverse();

  for (let start = 0; start < candidates.length; start += chunkSize) {
    const chunk = candidates.slice(start, start + chunkSize);
    const responses = await Promise.all(
      chunk.map(async (epoch) => ({ epoch, response: await client.getCommittee(epoch) })),
    );

    for (const { epoch, response } of responses) {
      assertNoGraphqlErrors(`committee(${epoch})`, response);
    }

    // Chunk is ordered newest first, so the first hit is the newest epoch with data.
    const hit = responses.find(({ response }) => (response.data?.committee.length ?? 0) > 0);
    if (hit) {
      const members = hit.response.data!.committee;
      log.info(
        `Using epoch ${hit.epoch} (${currentEpoch - hit.epoch} behind current) with ${members.length} committee members`,
      );
      return { epoch: hit.epoch, members };
    }
  }

  log.warn(
    `No committee data within epochs ${oldest}..${currentEpoch} on ${env.getCurrentEnvironmentName()}`,
  );
  return null;
}
