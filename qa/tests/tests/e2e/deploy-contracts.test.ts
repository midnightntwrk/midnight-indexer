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
//
// How it works (mirrors scripts/tests/toolkit-contracts-e2e.sh):
// 1) Copy the sample compiled contract from the toolkit image into a temp dir
// 2) generate-intent deploy  -> produces deploy.bin + initial private state
// 3) send-intent --to-bytes  -> produces deploy_tx.mn
// 4) generate-txs ... send   -> sends the deploy transaction to the node
// 5) contract-address        -> computes the contract address from the deploy_tx.mn
// (Optional sanity) contract-state -> ensure we can read on-chain state
//
// Defaults can be overridden via env vars:
//   NODE_CONTAINER:  name of the node container to reuse (default: midnight-indexer-node-1)
//   TOOLKIT_IMAGE:   toolkit image ref (default: ghcr.io/midnight-ntwrk/midnight-node-toolkit:0.16.3-3b7b8d7c)
//

import { execSync } from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";

type DeployResult = {
  contractAddress: string;
  deployTxPath: string;
  tempDir: string;
};

/** Run a shell command and return stdout. Keep output quiet unless DEBUG is set. */
function sh(cmd: string, opts: { cwd?: string } = {}) {
  if (process.env.DEBUG) {
    // eslint-disable-next-line no-console
    console.log(`$ ${cmd}`);
  }
  return execSync(cmd, { stdio: "pipe", encoding: "utf8", ...opts });
}

/** Run a toolkit CLI command inside a container that shares the node container's network namespace. */
function runToolkit(
  toolkitImage: string,
  nodeContainer: string,
  args: string[],
  mounts: Array<{ host: string; container: string }> = [],
  extraEnv: Record<string, string> = {},
) {
  const mountFlags = mounts.map((m) => `-v ${m.host}:${m.container}`).join(" ");
  const envFlags = Object.entries(extraEnv)
    .map(([k, v]) => `-e ${k}=${JSON.stringify(v)}`)
    .join(" ");
  const cmd = [
    "docker run --rm",
    `--network container:${nodeContainer}`,
    envFlags,
    mountFlags,
    toolkitImage,
    args.map((a) => (a.includes(" ") ? `"${a}"` : a)).join(" "),
  ]
    .filter(Boolean)
    .join(" ");
  return sh(cmd);
}

/** Copy the sample compiled "counter" contract out of the toolkit image to a host temp dir. */
function copySampleContract(toolkitImage: string, destDir: string) {
  const cid = sh(`docker create ${toolkitImage}`).trim();
  try {
    sh(`docker cp ${cid}:/toolkit-js/test/contract ${destDir}/contract`);
  } finally {
    sh(`docker rm -v ${cid}`);
  }
}

/**
 * Deploy the sample "counter" contract and return its address.
 * Assumes the indexer env node container (midnight-indexer-node-1) is already running.
 */
export async function deployContract(params?: {
  toolkitImage?: string;
  nodeContainer?: string;
}): Promise<DeployResult> {
  const toolkitImage =
    params?.toolkitImage ||
    process.env.TOOLKIT_IMAGE ||
    "ghcr.io/midnight-ntwrk/midnight-node-toolkit:0.16.3-3b7b8d7c";
  const nodeContainer = params?.nodeContainer || process.env.NODE_CONTAINER || "midnight-indexer-node-1";

  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "toolkit-deploy-"));
  const outDir = tempDir;
  const contractMount = path.join(tempDir, "contract");

  // 1) Bring the compiled contract onto the host so we can mount it.
  copySampleContract(toolkitImage, tempDir);

  // Standard filenames (match the bash script for predictability).
  const deployIntent = "deploy.bin";
  const deployTx = "deploy_tx.mn";
  const addressFile = "contract_address.mn";
  const stateFile = "contract_state.mn";
  const initialPrivateState = "initial_state.json";

  const mounts = [
    { host: outDir, container: "/out" },
    { host: contractMount, container: "/toolkit-js/contract" },
  ];

  // 2) generate-intent deploy
  runToolkit(
    toolkitImage,
    nodeContainer,
    [
      "generate-intent",
      "deploy",
      "-c",
      "/toolkit-js/contract/contract.config.ts",
      "--output-intent",
      `/out/${deployIntent}`,
      "--output-private-state",
      `/out/${initialPrivateState}`,
    ],
    mounts,
    { RUST_BACKTRACE: "1" },
  );
  if (!fs.existsSync(path.join(outDir, deployIntent))) {
    throw new Error("deploy intent was not produced");
  }

  // 3) send-intent -> deploy_tx.mn
  // Note: compiled-contract-dir matches the layout in the toolkit image we mounted.
  runToolkit(
    toolkitImage,
    nodeContainer,
    [
      "send-intent",
      "--intent-files",
      `/out/${deployIntent}`,
      "--compiled-contract-dir",
      "contract/managed/counter",
      "--to-bytes",
      "--dest-file",
      `/out/${deployTx}`,
    ],
    mounts,
    { RUST_BACKTRACE: "1" },
  );
  const deployTxPath = path.join(outDir, deployTx);
  if (!fs.existsSync(deployTxPath)) {
    throw new Error("deploy tx (.mn) was not produced");
  }

  // 4) generate-txs ... send
  runToolkit(
    toolkitImage,
    nodeContainer,
    ["generate-txs", "--src-files", `/out/${deployTx}`, "-r", "1", "send"],
    mounts,
    { RUST_BACKTRACE: "1" },
  );

  // 5) contract-address (compute address from deploy tx)
  runToolkit(
    toolkitImage,
    nodeContainer,
    [
      "contract-address",
      "--src-file",
      `/out/${deployTx}`,
      "--network",
      "undeployed",
      "--dest-file",
      `/out/${addressFile}`,
    ],
    mounts,
    { RUST_BACKTRACE: "1" },
  );
  const addr = fs.readFileSync(path.join(outDir, addressFile), "utf8").trim();
  if (!/^[0-9a-f]{64}$/i.test(addr)) {
    throw new Error(`invalid contract address computed: ${addr}`);
  }

  // 6) quick read
  runToolkit(
    toolkitImage,
    nodeContainer,
    ["contract-state", "--contract-address", addr, "--dest-file", `/out/${stateFile}`],
    mounts,
    { RUST_BACKTRACE: "1" },
  );
  if (!fs.existsSync(path.join(outDir, stateFile))) {
    throw new Error("failed to fetch contract state after deploy");
  }

  return { contractAddress: addr, deployTxPath, tempDir };
}

describe("deploy contracts via toolkit wrapper", () => {
  it(
    "deploys the sample counter contract and returns its address",
    async () => {
      // Ensure the indexer env is up and midnight-indexer-node-1 exists
      const { contractAddress, deployTxPath } = await deployContract();
      expect(contractAddress).toMatch(/^[0-9a-f]{64}$/i);
      expect(fs.existsSync(deployTxPath)).toBe(true);
    },
    120_000,
  );
});