// This file is part of midnightntwrk/midnight-indexer
// Copyright (C) 2025 Midnight Foundation
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

const INDEXER_BASE_URL: Record<string, string> = {
  undeployed: "localhost:8088",
  nodedev01: "indexer.node-dev-01.dev.midnight.network",
  devnet: "indexer.devnet.midnight.network",
  preview: "indexer.preview.midnight.network",
  qanet: "indexer.qanet.dev.midnight.network",
  testnet02: "indexer.testnet-02.midnight.network",
};

export let TARGET_ENV: string;

if (Bun.env.TARGET_ENV === undefined || Bun.env.TARGET_ENV === "") {
  console.warn(
    "[WARN ] - TARGET_ENV not set, default to undeployed environment",
  );
  TARGET_ENV = "undeployed";
} else {
  TARGET_ENV = Bun.env.TARGET_ENV;
  console.info(`[INFO ] - Target environment: ${TARGET_ENV}`);
}

export const INDEXER_WS_URL: string =
  TARGET_ENV === "undeployed"
    ? `ws://${INDEXER_BASE_URL[TARGET_ENV]}/api/v3/graphql/ws`
    : `wss://${INDEXER_BASE_URL[TARGET_ENV]}/api/v3/graphql/ws`;

export const INDEXER_HTTP_URL: string =
  TARGET_ENV === "undeployed"
    ? `http://${INDEXER_BASE_URL[TARGET_ENV]}`
    : `https://${INDEXER_BASE_URL[TARGET_ENV]}`;
