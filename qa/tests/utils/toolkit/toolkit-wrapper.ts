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

import log from '@utils/logging/logger';
import { env, networkIdByEnvName } from '../../environment/model';
import { GenericContainer, StartedTestContainer } from 'testcontainers';
import { existsSync, readFileSync } from "fs";
import { join } from "path";


export type AddressType = 'shielded' | 'unshielded';

interface ToolkitConfig {
  containerName?: string;
  targetDir?: string;
  chain?: string;
  nodeTag?: string;
  syncCacheDir?: string;
  toolkitImage?: string
  nodeContainer?: string
  network?: string
  coinSeed?: string;
}

interface ToolkitTransactionResult {
  txHash: string;
  blockHash?: string;
  status: 'sent' | 'confirmed';
  rawOutput: string;
}

interface LogEntry {
  level: string;
  message: string;
  target: string;
  timestamp: number;
  tx_hash?: string;
  block_hash?: string;
}

class ToolkitWrapper {
  private container: GenericContainer;
  private startedContainer?: StartedTestContainer;
  private config: ToolkitConfig;
  public readonly runtime!: { toolkitImage: string; nodeContainer: string; network: string };

  private parseTransactionOutput(output: string): ToolkitTransactionResult {
    const lines = output.trim().split('\n');
    const jsonLines = lines.filter((line) => line.trim().startsWith('{'));

    let txHash = '';
    let blockHash: string | undefined;
    let status: 'sent' | 'confirmed' = 'sent';

    // Parse the JSON log entries
    for (const line of jsonLines) {
      try {
        const logEntry: LogEntry = JSON.parse(line);

        if (logEntry.tx_hash) {
          txHash = logEntry.tx_hash;
        }

        if (logEntry.block_hash) {
          blockHash = logEntry.block_hash;
          status = 'confirmed';
        }
      } catch (error) {
        // Skip lines that aren't valid JSON
        continue;
      }
    }

    if (!txHash) {
      throw new Error('Could not extract transaction hash from toolkit output');
    }

    return {
      txHash,
      blockHash,
      status,
      rawOutput: output,
    };
  }

  private parseContractAddress(raw: string): string {
  const s = (raw ?? "").trim();
  // If the CLI printed JSON (0.17+), prefer the typed field.
  if (s.startsWith("{")) {
    try {
      const j = JSON.parse(s);
      if (j && typeof j.tagged === "string" && j.tagged.trim()) {
        return this.parseContractAddress(j.tagged);
      }
      if (j && typeof j.untagged === "string" && j.untagged.trim()) {
        const u = j.untagged.trim();
        if (/^[0-9A-Fa-f]{64}$/.test(u)) return u.toLowerCase();
      }
    } catch {
      // fall through to regex
    }
  }

  // Robust fallback: find either typed or bare 64-hex anywhere in the string.
  const m =
    s.match(/midnight:contract-address(?:\[[vV]\d+\])?:([0-9A-Fa-f]{64})/) ||
    s.match(/([0-9A-Fa-f]{64})\b/);

  if (!m) throw new Error(`unexpected contract-address format: ${raw}`);
  return m[1].toLowerCase();
}

  private async resolveCoinPublic(network: string, seedFromConfig?: string): Promise<string> {
  if (!this.startedContainer) {
    throw new Error('Container is not started. Call start() first.');
  }

  // Derive it from a seed
  const seed =
    seedFromConfig?.trim() ||
    process.env.COIN_SEED?.trim() ||
    "0000000000000000000000000000000000000000000000000000000000000001";

  const r = await this.startedContainer.exec([
    "/midnight-node-toolkit",
    "show-address",
    "--network",
    network,
    "--seed",
    seed,
    "--coin-public",
  ]);

  if (r.exitCode !== 0) {
    const e = r.stderr || r.output || "Unknown error";
    throw new Error(`show-address --coin-public failed: ${e}`);
  }

  return (r.output || "").trim();
  }


  constructor(config: ToolkitConfig) {
    this.config = config;

    const randomId = Math.random().toString(36).slice(2, 12);

    this.config.containerName =
      config.containerName || `mn-toolkit-${env.getEnvName()}-${randomId}`;
    this.config.targetDir = config.targetDir || '/tmp/toolkit/';
    this.config.nodeTag = config.nodeTag || env.getNodeVersion();
    this.config.syncCacheDir = `${this.config.targetDir}/.sync_cache-${env.getEnvName()}-${randomId}`;

    const toolkitImage =
      config.toolkitImage ??
      process.env.TOOLKIT_IMAGE ??
      `ghcr.io/midnight-ntwrk/midnight-node-toolkit:${process.env.NODE_TAG ?? "0.17.0-rc.2"}`;

    const nodeContainer =
      config.nodeContainer ??
      process.env.NODE_CONTAINER ??
      "midnight-indexer-node-1";

    const network =
      (config.network ?? process.env.TARGET_ENV ?? "undeployed").toLowerCase();
      
    this.runtime = { toolkitImage, nodeContainer, network };

    log.debug(`Toolkit container name: ${this.config.containerName}`);
    log.debug(`Toolkit target dir: ${this.config.targetDir}`);
    log.debug(`Toolkit node tag: ${this.config.nodeTag}`);
    log.debug(`Toolkit sync cache dir: ${this.config.syncCacheDir}`);

    this.container = new GenericContainer(
      `ghcr.io/midnight-ntwrk/midnight-node-toolkit:${this.config.nodeTag}`,
    )
      .withName(this.config.containerName)
      .withNetworkMode('host') // equivalent to --network host
      .withEntrypoint([]) // equivalent to --entrypoint ""
      .withBindMounts([
        {
          source: this.config.targetDir,
          target: '/out',
        },
        {
          source: this.config.syncCacheDir,
          target: `/.sync_cache`,
        },
      ])
      .withCommand(['sleep', 'infinity']); // equivalent to sleep infinity
  }

  async start() {
    const image = this.runtime.toolkitImage;
    this.startedContainer = await this.container.start();
  }

  async stop() {
    if (this.startedContainer) {
      await this.startedContainer.stop();
    }
  }

  async showAddress(seed: string, addressType: AddressType): Promise<string> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    const result = await this.startedContainer.exec([
      '/midnight-node-toolkit',
      'show-address',
      '--network',
      env.getEnvName().toLowerCase(),
      '--seed',
      seed,
    ]);

    if (result.exitCode !== 0) {
      const errorMessage = result.stderr || result.output || 'Unknown error occurred';
      throw new Error(`Toolkit command failed with exit code ${result.exitCode}: ${errorMessage}`);
    }

    // Extract the json object and return it as is
    return JSON.parse(result.output.trim())[addressType];
  }

  async showViewingKey(seed: string): Promise<string> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    const result = await this.startedContainer.exec([
      '/midnight-node-toolkit',
      'show-viewing-key',
      '--network',
      env.getEnvName().toLowerCase(),
      '--seed',
      seed,
    ]);

    if (result.exitCode !== 0) {
      const errorMessage = result.stderr || result.output || 'Unknown error occurred';
      throw new Error(`Toolkit command failed with exit code ${result.exitCode}: ${errorMessage}`);
    }

    return result.output.trim();
  }

  async generateSingleTx(
    sourceSeed: string,
    addressType: AddressType,
    destinationAddress: string,
    amount: number,
  ): Promise<ToolkitTransactionResult> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    const sourceUrl = env.getNodeWebsocketBaseURL();
    const destUrl = env.getNodeWebsocketBaseURL();

    const result = await this.startedContainer.exec([
      '/midnight-node-toolkit',
      'generate-txs',
      '--src-url',
      sourceUrl,
      '--dest-url',
      destUrl,
      'single-tx',
      '--source-seed',
      sourceSeed,
      '--destination-address',
      destinationAddress,
      `--${addressType}-amount`,
      amount.toString(),
    ]);

    if (result.exitCode !== 0) {
      const errorMessage = result.stderr || result.output || 'Unknown error occurred';
      throw new Error(`Toolkit command failed with exit code ${result.exitCode}: ${errorMessage}`);
    }

    const rawOutput = result.output.trim();
    return this.parseTransactionOutput(rawOutput);
  }

  async deployContract(opts?: {
    contractConfigPath?: string;      
    compiledContractDir?: string;     
    network?: string;                 
  }): Promise<{
    addressRaw: string;
    addressHex: string;
    deployTxPath: string;
    statePath: string;
    outDir: string;
  }> {
  if (!this.startedContainer) {
    throw new Error("Container is not started. Call start() first.");
  }
  const outDir = this.config.targetDir!; '/tmp/toolkit/';
 
  const contractConfigPath = opts?.contractConfigPath ?? "/toolkit-js/test/contract/contract.config.ts";
  const compiledContractDir = opts?.compiledContractDir ?? "/toolkit-js/test/contract/managed/counter";
  const network = (opts?.network ?? this.runtime.network).toLowerCase();  

  const deployIntent = "deploy.bin";
  const deployTx = "deploy_tx.mn";
  const addressFile = "contract_address.mn";
  const stateFile = "contract_state.mn";
  const initialPrivateState = "initial_state.json";

  const outDeployIntent = join(outDir, deployIntent);
  const outDeployTx = join(outDir, deployTx);
  const outAddressFile = join(outDir, addressFile);
  const outStateFile = join(outDir, stateFile);
  const outInitialState = join(outDir, initialPrivateState);
  const zswapFile = "temp.json"; 
  const coinPublic = await this.resolveCoinPublic(network, this.config.coinSeed);
  let addressRaw = "";
  let addressHex = "";

  // 1) generate-intent deploy
  {
    const result = await this.startedContainer.exec([
      "/midnight-node-toolkit",
      "generate-intent",
      "deploy",
      "-c",
      contractConfigPath,
      "--output-intent",
      `/out/${deployIntent}`,
      "--output-private-state",
      `/out/${initialPrivateState}`,
      "--coin-public", coinPublic,
      "--output-zswap-state", `/out/${zswapFile}`,
    ]);
    if (result.exitCode !== 0) {
      const e = result.stderr || result.output || "Unknown error";
      throw new Error(`generate-intent deploy failed: ${e}`);
    }
    if (!existsSync(outDeployIntent) || !existsSync(outInitialState)) {
      throw new Error("generate-intent deploy did not produce expected outputs");
    }
  }

  // 2) send-intent -> bytes (.mn)
  {
    const result = await this.startedContainer.exec([
      "/midnight-node-toolkit",
      "send-intent",
      "--intent-file",
      `/out/${deployIntent}`,
      "--compiled-contract-dir",
      compiledContractDir,
      "--to-bytes",
      "--dest-file",
      `/out/${deployTx}`,
    ]);
    if (result.exitCode !== 0) {
      const e = result.stderr || result.output || "Unknown error";
      throw new Error(`send-intent failed: ${e}`);
    }
    if (!existsSync(outDeployTx)) {
      throw new Error("send-intent did not produce /out/deploy_tx.mn");
    }
  }

  // 3) generate-txs ... send
  {
    const result = await this.startedContainer.exec([
      "/midnight-node-toolkit",
      "generate-txs",
      "--src-files",
      `/out/${deployTx}`,
      "-r",
      "1",
      "send",
    ]);
    if (result.exitCode !== 0) {
      const e = result.stderr || result.output || "Unknown error";
      throw new Error(`generate-txs send failed: ${e}`);
    }
  }

  // 4) contract-address -> file
  {
    const result = await this.startedContainer.exec([
    "/midnight-node-toolkit",
    "contract-address",
    "--network",
    network,
    "--src-file",
    "/out/deploy_tx.mn",
  ]);
  if (result.exitCode !== 0) {
    const e = result.stderr || result.output || "Unknown error";
    throw new Error(`contract-address failed: ${e}`);
  }

  // The CLI may print JSON or a typed line — extract a typed string.
  const out = (result.output || "").trim();

  // If JSON, prefer 'tagged'; else find the typed address in free text.
  let typed = "";
  if (out.startsWith("{")) {
    try {
      const j = JSON.parse(out);
      if (j && typeof j.tagged === "string") typed = j.tagged.trim();
    } catch {
      // ignore and fall back to regex
    }
  }
  if (!typed) {
    const mm = out.match(/midnight:contract-address(?:\[[vV]\d+\])?:[0-9A-Fa-f]{64}/);
    if (mm) typed = mm[0];
  }
  if (!typed) {
    throw new Error(`unexpected contract-address output: ${out.slice(0, 200)}`);
  }

  // persist EXACTLY the typed string (no JSON)
  const fs = await import("node:fs");
  fs.writeFileSync(outAddressFile, typed + "\n", "utf8");

  // share with later steps
  addressRaw = typed;
  addressHex = this.parseContractAddress(typed);

    if (!existsSync(outAddressFile)) {
      throw new Error("contract-address did not produce /out/contract_address.mn");
    }
  }

  const raw = readFileSync(outAddressFile, "utf8").trim();
  const hex = this.parseContractAddress(raw);

  // 5) quick state read — pass the address exactly as written by the toolkit
  {
    const result = await this.startedContainer.exec([
      "/midnight-node-toolkit",
      "contract-state",
      "--contract-address",
      addressRaw,                      
      "--dest-file",
      "/out/contract_state.mn",
]);
    if (result.exitCode !== 0) {
      const e = result.stderr || result.output || "Unknown error";
      throw new Error(`contract-state failed: ${e}`);
    }
    if (!existsSync(outStateFile)) {
      throw new Error("contract-state did not produce /out/contract_state.mn");
    }
  }

  return {
    addressRaw: raw,
    addressHex: hex,
    deployTxPath: outDeployTx,
    statePath: outStateFile,
    outDir,   
  };
}

}

export { ToolkitWrapper, ToolkitConfig, ToolkitTransactionResult };
