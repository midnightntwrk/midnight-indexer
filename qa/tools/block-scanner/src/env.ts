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

// Host of the local stack, the only environment this repository targets. Any
// other TARGET_ENV needs INDEXER_HOST, which also takes precedence over it.
const INDEXER_BASE_URL: Record<string, string> = {
  undeployed: "localhost:8088",
};

export let TARGET_ENV: string;
export let INDEXER_API_VERSION: string;

if (Bun.env.TARGET_ENV === undefined || Bun.env.TARGET_ENV === "") {
  console.warn(
    "[WARN ] - TARGET_ENV not set, default to undeployed environment",
  );
  TARGET_ENV = "undeployed";
} else {
  TARGET_ENV = Bun.env.TARGET_ENV;
  console.info(`[INFO ] - Target environment: ${TARGET_ENV}`);
}

if (
  Bun.env.INDEXER_API_VERSION === undefined ||
  Bun.env.INDEXER_API_VERSION === ""
) {
  console.warn(
    "[WARN ] - INDEXER_API_VERSION not set explicitly, default to v4",
  );
  INDEXER_API_VERSION = "v4";
} else {
  INDEXER_API_VERSION = Bun.env.INDEXER_API_VERSION;
  console.info(`[INFO ] - Indexer API version: ${INDEXER_API_VERSION}`);
}

const INDEXER_HOST: string | undefined =
  Bun.env.INDEXER_HOST?.trim() || INDEXER_BASE_URL[TARGET_ENV];

if (INDEXER_HOST === undefined) {
  throw new Error(
    `TARGET_ENV="${TARGET_ENV}" has no built-in indexer host. ` +
      `Set INDEXER_HOST to the host (and optional port) to scan.`,
  );
}

export const INDEXER_WS_URL: string =
  TARGET_ENV === "undeployed"
    ? `ws://${INDEXER_HOST}/api/${INDEXER_API_VERSION}/graphql/ws`
    : `wss://${INDEXER_HOST}/api/${INDEXER_API_VERSION}/graphql/ws`;

export const INDEXER_HTTP_URL: string =
  TARGET_ENV === "undeployed"
    ? `http://${INDEXER_HOST}`
    : `https://${INDEXER_HOST}`;
