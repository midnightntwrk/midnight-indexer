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

import fs from 'fs';
import { join, resolve } from 'path';
import { retry } from '../retry-helper';
import log from '@utils/logging/logger';
import { env } from '../../environment/model';
import { GenericContainer, StartedTestContainer } from 'testcontainers';
import { getContractDeploymentHashes, resolveBlockHash } from '../../tests/e2e/test-utils';
import { ensureToolkitCachePostgres } from './toolkit-cache';
import { closeMothWallets, generateSingleTxViaMoth } from '../moth/moth-backend';
import { z } from 'zod';
import {
  Coin,
  DustBalance,
  DustBalanceSchema,
  DustOutput,
  PrivateWalletState,
  PrivateWalletStateSchema,
  PublicWalletState,
  PublicWalletStateSchema,
  Utxo,
} from './schemas';

export type AddressType = 'shielded' | 'unshielded';

export type ShowAddressOption =
  | 'shielded'
  | 'unshielded'
  | 'coin-public'
  | 'coin-public-tagged'
  | 'unshielded-user-address-untagged';

interface AddressInfo {
  shielded: string;
  unshielded: string;
  // `show-address` has always returned this; it was simply not declared here.
  // Verified against midnight-node-toolkit:1.0.0 on preview (2026-09-17).
  dust: string;
  coinPublic: string;
  coinPublicTagged: string;
  unshieldedUserAddressUntagged: string;
  userAddress: string;
}

interface ToolkitConfig {
  containerName?: string;
  targetDir?: string;
  chain?: string;
  nodeTag?: string;
  nodeToolkitTag?: string;
  coinSeed?: string;
  /**
   * Host directory holding a compiled Compact contract: its toolkit-js
   * `config.ts` plus the compactc output directory. Mounted into the
   * toolkit-js tree so the custom-contract commands can reach it.
   */
  customContractDir?: string;
}

/**
 * Identifies which contract inside `customContractDir` to drive. Paths are
 * relative to that directory.
 */
export interface CustomContractSpec {
  /** The toolkit-js config file, e.g. `segment-split.config.ts`. */
  configFile: string;
  /** The compactc output directory. Defaults to `managed`. */
  managedDir?: string;
}

/** A built-but-not-yet-submitted custom contract call. */
export interface CustomContractCall {
  /** Caller-supplied name, used to keep per-call files distinct and to label errors. */
  label: string;
  /** Generated transaction file, relative to the container's `/out`. */
  txFileName: string;
}

export interface ToolkitTransactionResult {
  txHash: string;
  blockHash: string;
  status: 'sent' | 'confirmed';
  rawOutput: string;
}

interface LogEntry {
  level: string;
  message: string;
  target: string;
  timestamp: number;
  midnight_tx_hash?: string;
  block_hash?: string;
}

export interface DeployContractResult {
  'contract-address-untagged': string;
  'contract-address-tagged': string;
  'coin-public': string;
  'deploy-tx-hash': string;
  'deploy-block-hash': string;
}

const TOOLKIT_BIN = '/midnight-node-toolkit';
const CONTRACT_SIMPLE = 'contract-simple';
const CONTRACT_CUSTOM = 'contract-custom';
const TOOLKIT_JS_PATH = '/toolkit-js';
/**
 * Where a custom contract's compiled assets are mounted. It has to live inside
 * the toolkit-js tree: the contract's `config.ts` imports
 * `@midnight-ntwrk/compact-js`, which only resolves from toolkit-js'
 * own node_modules.
 */
const CUSTOM_CONTRACT_MOUNT = `${TOOLKIT_JS_PATH}/test/custom-contract`;
const DEFAULT_MANAGED_DIR = 'managed';
/**
 * Lists every `@midnight-ntwrk/compact-runtime` the toolkit-js tree carries.
 *
 * The tree has held its runtime variants under different names across
 * releases — `v7`/`v8` on toolkit 1.x, `compact-0.30` on 2.0.x,
 * `compact-0.30.0` on 2.1.x — so this globs one level down rather than
 * assuming any of them, and includes the hoisted root copy. The image has no
 * `find`, hence the shell loop.
 */
const TOOLKIT_JS_RUNTIME_PROBE = [
  'for f in',
  `${TOOLKIT_JS_PATH}/node_modules/@midnight-ntwrk/compact-runtime/package.json`,
  `${TOOLKIT_JS_PATH}/*/node_modules/@midnight-ntwrk/compact-runtime/package.json;`,
  'do [ -f "$f" ] && grep -m1 \'"version"\' "$f"; done; true',
].join(' ');
const DEFAULT_COIN_PUBLIC_SEED = '0000000000000000000000000000000000000000000000000000000000000001';
const DEFAULT_RNG_SEED = '0000000000000000000000000000000000000000000000000000000000000037';
// Default coin/funding seed used by the toolkit minter e2e (matches the node-repo
// scripts/tests/toolkit-tokens-minter-e2e.sh). Used by deployMintSendUnshielded (#1253).
const DEFAULT_FUNDING_SEED = '0000000000000000000000000000000000000000000000000000000000000001';
const DEFAULT_NEW_AUTHORITY_SEED =
  '1000000000000000000000000000000000000000000000000000000000000001';

/** Strip terminal colour codes, so parsing never depends on whether the toolkit colorizes. */
// eslint-disable-next-line no-control-regex
const stripAnsi = (value: string): string => value.replace(/\x1b\[[0-9;]*m/g, '');

// Human-readable description of the schema `getDustBalance` accepts, reused in its error
// messages so a mismatch reports expected-vs-actual rather than a cryptic "structure not found".
const DUST_BALANCE_EXPECTED = [
  'Expected one of:',
  '  (1) full DustBalance object: { generation_infos: Array<…>, source: Record<hexKey, number>, total: number }',
  '  (2) source-only object:      Record<hexKey, number>',
  '  where hexKey is an even-length lowercase hex string (whole bytes, any length).',
].join('\n');

class ToolkitWrapper {
  private container: GenericContainer;
  private startedContainer?: StartedTestContainer;
  private config: ToolkitConfig;

  private getRpcUrl(): string {
    return env.getNodeWebsocketBaseURL();
  }

  /**
   * Run a toolkit command and throw on non-zero exit. Returns exec result for further use.
   */
  private async execToolkit(
    args: string[],
    errorContext: string,
  ): Promise<{ output: string; exitCode: number }> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }
    const result = await this.startedContainer.exec(args);
    if (result.exitCode !== 0) {
      const msg = result.stderr || result.output || 'Unknown error';
      throw new Error(`${errorContext}: ${msg}`);
    }
    return { output: result.output, exitCode: result.exitCode };
  }

  /**
   * Build base args for generate-txs (src-url, dest-file, to-bytes contract-simple <subcommand>).
   */
  private buildGenerateTxBase(destFile: string, subcommand: string): string[] {
    return [
      TOOLKIT_BIN,
      'generate-txs',
      '--src-url',
      this.getRpcUrl(),
      '--dest-file',
      destFile,
      CONTRACT_SIMPLE,
      subcommand,
    ];
  }

  /**
   * Submit a generated tx file to the network. Returns raw output for parsing.
   */
  private async sendGeneratedTx(txFileName: string): Promise<string> {
    const result = await this.execToolkit(
      [
        TOOLKIT_BIN,
        'generate-txs',
        '--src-file',
        `/out/${txFileName}`,
        'send',
        '-d',
        this.getRpcUrl(),
      ],
      'generate-txs send failed',
    );
    return result.output.trim();
  }

  private parseTransactionOutput(output: string): ToolkitTransactionResult {
    const lines = stripAnsi(output).trim().split('\n');
    const jsonLines = lines.filter((line) => line.trim().startsWith('{'));

    let txHash = '';
    let blockHash: string | undefined;
    let status: 'sent' | 'confirmed' = 'sent';

    // Parse the JSON log entries
    for (const line of jsonLines) {
      try {
        const logEntry: LogEntry = JSON.parse(line);

        if (logEntry.midnight_tx_hash) {
          txHash = logEntry.midnight_tx_hash;
        }

        if (logEntry.block_hash) {
          blockHash = logEntry.block_hash;
          status = 'confirmed';
        }
      } catch (_error) {
        continue;
      }
    }

    // Fallback: parse key=value structured log lines (newer toolkit format)
    if (!txHash) {
      for (const line of lines) {
        const txMatch = line.match(/midnight_tx_hash="?([^"\s]+)"?/);
        if (txMatch) {
          txHash = txMatch[1];
        }

        const blockMatch = line.match(/block_hash="?([^"\s]+)"?/);
        if (blockMatch) {
          blockHash = blockMatch[1];
        }

        if (line.includes('FINALIZED')) {
          status = 'confirmed';
        }
      }
    }

    if (!txHash) {
      throw new Error('Could not extract transaction hash from toolkit output');
    }

    // Remove 0x prefix if present to match indexer API format (which doesn't use 0x prefix)
    const removeHexPrefix = (hash: string) => (hash.startsWith('0x') ? hash.slice(2) : hash);

    return {
      txHash: removeHexPrefix(txHash),
      blockHash: blockHash ? removeHexPrefix(blockHash) : '',
      status,
      rawOutput: output,
    };
  }

  /**
   * Parse and validate the first valid JSON object from an array of JSON strings using a Zod schema.
   *
   * @param jsonObjects - Array of JSON strings to parse and validate
   * @param schema - Zod schema to validate against
   * @returns The first valid parsed object, or null if none match
   */
  private parseFirstValid<T>(jsonObjects: string[], schema: z.ZodSchema<T>): T | null {
    for (const jsonString of jsonObjects) {
      try {
        const parsed: unknown = JSON.parse(jsonString);
        const result = schema.safeParse(parsed);
        if (result.success) {
          return result.data;
        }
      } catch {
        // Invalid JSON or schema validation failed, try next object
      }
    }
    return null;
  }

  /**
   * Parse wallet state from toolkit output.
   * This helper method extracts JSON objects and validates the wallet state structure.
   *
   * @param output - The raw output from the toolkit command
   * @param stateType - The type of wallet state being parsed ('private' or 'public')
   * @returns The parsed wallet state object
   * @throws Error if no valid wallet state structure is found
   */
  private parseWalletState(
    output: string,
    stateType: 'private' | 'public',
  ): PrivateWalletState | PublicWalletState {
    const jsonObjects = this.extractJsonObjects(output);

    if (jsonObjects.length === 0) {
      throw new Error(
        `Could not find any JSON objects in show-wallet output. Output: ${output.substring(0, 500)}...`,
      );
    }

    const schema = stateType === 'private' ? PrivateWalletStateSchema : PublicWalletStateSchema;
    const walletState = this.parseFirstValid(jsonObjects, schema);

    if (!walletState) {
      throw new Error(
        `Could not find expected ${stateType} wallet state structure in output. Found ${jsonObjects.length} JSON object(s).`,
      );
    }

    return walletState;
  }

  /**
   * Extract all JSON objects from a string that may contain text and multiple JSON objects.
   * This helper method finds complete JSON objects by matching braces.
   *
   * @param output - The output string that may contain JSON objects
   * @returns An array of JSON strings, each representing a complete JSON object
   */
  private extractJsonObjects(output: string): string[] {
    const jsonObjects: string[] = [];
    let startIndex = 0;

    while (startIndex < output.length) {
      const braceIndex = output.indexOf('{', startIndex);
      if (braceIndex === -1) break;

      // Extract from this '{' and find the matching closing brace
      let braceCount = 0;
      let endIndex = -1;
      for (let i = braceIndex; i < output.length; i++) {
        if (output[i] === '{') {
          braceCount++;
        } else if (output[i] === '}') {
          braceCount--;
          if (braceCount === 0) {
            endIndex = i + 1;
            break;
          }
        }
      }

      if (endIndex > 0) {
        const jsonString = output.substring(braceIndex, endIndex);
        jsonObjects.push(jsonString);
        startIndex = endIndex;
      } else {
        break;
      }
    }

    return jsonObjects;
  }

  constructor(config: ToolkitConfig) {
    this.config = config;

    const randomId = Math.random().toString(36).slice(2, 12);

    const envName = env.getCurrentEnvironmentName();

    this.config.containerName = config.containerName || `mn-toolkit-${envName}-${randomId}`;
    this.config.targetDir = config.targetDir || resolve(`./.tmp/toolkit/${envName}-${randomId}`);
    this.config.nodeTag = config.nodeTag || env.getNodeVersion();
    this.config.nodeToolkitTag =
      config.nodeToolkitTag || process.env.NODE_TOOLKIT_TAG || 'latest-main';

    // Shared, env-specific ledger state cache — persists across runs so the toolkit can restore
    // from a snapshot rather than replaying the full chain on every warmup.
    const ledgerCacheDir = resolve(`./.tmp/toolkit-ledger-cache/${envName}`);

    // Shared ZK params cache — scoped by toolkit tag so different versions don't overwrite each
    // other's circuit parameters. Kept outside targetDir so it is never deleted between runs
    // (root-owned files written by the container would prevent host-side cleanup of per-run
    // targetDirs otherwise).
    const zkCacheDir = resolve(`./.tmp/toolkit-zk-cache/${this.config.nodeToolkitTag}`);

    fs.mkdirSync(this.config.targetDir, { recursive: true });
    fs.mkdirSync(ledgerCacheDir, { recursive: true });
    fs.mkdirSync(zkCacheDir, { recursive: true });

    log.debug(`NODE_TAG         : ${this.config.nodeTag}`);
    log.debug(`NODE_TOOLKIT_TAG : ${this.config.nodeToolkitTag}`);
    log.debug(`Toolkit target dir     : ${this.config.targetDir}`);
    log.debug(`Toolkit container name : ${this.config.containerName}`);
    log.debug(`Toolkit ledger cache   : ${ledgerCacheDir}`);
    log.debug(`Toolkit ZK cache       : ${zkCacheDir}`);

    this.container = new GenericContainer(
      `ghcr.io/midnight-ntwrk/midnight-node-toolkit:${this.config.nodeToolkitTag}`,
    )
      .withName(this.config.containerName)
      .withNetworkMode('host')
      .withEntrypoint([])
      .withBindMounts([
        {
          source: this.config.targetDir,
          target: '/out',
        },
        {
          source: zkCacheDir,
          target: '/.cache',
        },
        {
          source: ledgerCacheDir,
          target: '/ledger-cache',
        },
        ...(this.config.customContractDir
          ? [{ source: this.config.customContractDir, target: CUSTOM_CONTRACT_MOUNT }]
          : []),
      ])
      .withEnvironment({ MN_LEDGER_CACHE_DB: '/ledger-cache' })
      .withCommand(['sleep', 'infinity']);
  }

  /**
   * Start the toolkit container
   * This method starts the Docker container with retry logic to handle transient failures.
   *
   * @returns A promise that resolves when the container has successfully started
   *
   * @throws Error if the container fails to start after the maximum number of retries
   */
  async start() {
    const cache = await ensureToolkitCachePostgres();
    log.debug(`Toolkit fetch cache    : ${cache.host}:${cache.port}/${cache.database}`);
    this.container.withEnvironment({ MN_FETCH_CACHE: cache.fetchCacheUrl });

    this.startedContainer = await retry(async () => this.container.start(), {
      maxRetries: 2,
      delayMs: 2_000,
      retryLabel: 'start toolkit container',
    });
  }

  async stop() {
    // Release any moth wallet opened by the moth backend. Safe when unused —
    // it clears an empty set.
    await closeMothWallets();
    if (this.startedContainer) {
      // Make /out world-writable before stopping so the host process can delete root-owned
      // files that the container wrote there (e.g. transaction files).
      try {
        await this.startedContainer.exec(['chmod', '-R', '777', '/out']);
      } catch {
        // Best-effort; cleanup below may still warn if files remain root-owned.
      }
      await this.startedContainer.stop();
    }
    if (this.config.targetDir) {
      try {
        fs.rmSync(this.config.targetDir, { recursive: true, force: true });
        log.debug(`Cleaned up toolkit target dir: ${this.config.targetDir}`);
      } catch (error) {
        log.warn(`Failed to clean up toolkit target dir: ${error}`);
      }
    }
  }

  /**
   * Returns true when the toolkit error message indicates an RPC-level request timeout,
   * regardless of how it is spelled (camelCase "RequestTimeout" from the substrate client
   * or spaced "Request timeout" from the compute-task error path).
   */
  private isRpcTimeoutError(error: unknown): boolean {
    // Match both the camelCase "RequestTimeout" from the substrate client and the
    // spaced "Request timeout" from the compute-task error path. A plain
    // toLowerCase().includes('request timeout') misses the camelCase form, which
    // would silently treat a mid-sync timeout as a completed warmup.
    return /request[\s_]?timeout/i.test(String(error));
  }

  /**
   * Warm up the cache by generating a single unshielded transaction, retrying on RPC timeouts.
   *
   * The toolkit syncs the postgres fetch-cache before attempting the tx. If it hits an
   * RPC timeout mid-sync it exits with code 1 without writing highest_verified, so the
   * next run replays from block 0 (cache hits are fast). We retry until the toolkit exits
   * for a non-timeout reason, which means the sync completed and the tx failed as expected
   * (invalid seed / insufficient funds).
   */
  async warmupCache() {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    const RETRY_DELAY_MS = 5_000;
    const MAX_ATTEMPTS = 20;
    // Resolve destination address once — it is stable across retries.
    const destinationAddress = (await this.showAddress('0'.repeat(63) + '9')).unshielded;

    for (let attempt = 1; attempt <= MAX_ATTEMPTS; attempt++) {
      try {
        const output = await this.generateSingleTx(
          '0'.repeat(64), // Invalid seed — forces a full cache sync before the tx is attempted
          'unshielded',
          destinationAddress,
          1,
        );
        console.debug(`[SETUP] Warmup cache output:\n${JSON.stringify(output, null, 2)}`);
        return;
      } catch (error) {
        if (this.isRpcTimeoutError(error) && attempt < MAX_ATTEMPTS) {
          console.log(
            `[SETUP] Cache sync interrupted by RPC timeout (attempt ${attempt}/${MAX_ATTEMPTS}), ` +
              `retrying in ${RETRY_DELAY_MS / 1_000}s…`,
          );
          await new Promise((res) => setTimeout(res, RETRY_DELAY_MS));
          continue;
        }
        if (this.isRpcTimeoutError(error)) {
          // Persistent timeout (node down / wedged): give up with a clear error rather
          // than looping until the CI job-level timeout kills the run.
          throw new Error(
            `[SETUP] Cache warmup exhausted ${MAX_ATTEMPTS} RPC-timeout retries; ` +
              `node RPC appears unreachable. Last error: ${error}`,
            { cause: error },
          );
        }
        // Any non-timeout error means the sync completed and the tx failed for an expected
        // reason (invalid seed, insufficient funds, etc.) — warmup is done.
        log.debug('Warmup completed — expected toolkit error after cache sync');
        console.debug(`${error}`);
        return;
      }
    }
  }

  /**
   * Show address information from a seed
   *
   * @param seed - The seed to use
   * @param networkId - The network ID to use (default: current target environment)
   *
   * @returns The address information as a JSON object
   */
  async showAddress(seed: string, networkId?: string): Promise<AddressInfo> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    const response = await this.startedContainer.exec([
      TOOLKIT_BIN,
      'show-address',
      '--network',
      networkId ?? env.getNetworkId().toLowerCase(),
      '--seed',
      seed,
    ]);

    if (response.exitCode !== 0) {
      const errorMessage = response.stderr || response.output || 'Unknown error occurred';
      throw new Error(
        `Toolkit command failed with exit code ${response.exitCode}: ${errorMessage}`,
      );
    }

    return JSON.parse(response.output);
  }

  /**
   * Show viewing key information from a seed
   *
   * @param seed - The seed to use
   * @param networkId - The network ID to use (default: current target environment)
   *
   * @returns The viewing key as a string
   */
  async showViewingKey(seed: string, networkId?: string): Promise<string> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    const result = await this.startedContainer.exec([
      TOOLKIT_BIN,
      'show-viewing-key',
      '--network',
      networkId ?? env.getNetworkId().toLowerCase(),
      '--seed',
      seed,
    ]);

    if (result.exitCode !== 0) {
      const errorMessage = result.stderr || result.output || 'Unknown error occurred';
      throw new Error(`Toolkit command failed with exit code ${result.exitCode}: ${errorMessage}`);
    }

    return result.output.trim();
  }

  /**
   * Execute show-wallet command and parse the result.
   * This helper method handles the common logic for both private and public wallet state queries.
   *
   * @param flag - The flag to use ('--seed' or '--address')
   * @param value - The value for the flag (seed or address)
   * @param stateType - The type of wallet state ('private' or 'public')
   * @param logPrefix - Prefix for log messages
   * @returns The parsed wallet state object
   * @throws Error if the container is not started or if the command fails
   */
  private async executeShowWallet(
    flag: '--seed' | '--address',
    value: string,
    stateType: 'private' | 'public',
    logPrefix: string,
  ): Promise<PrivateWalletState | PublicWalletState> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    log.debug(`${logPrefix}: ${value.substring(0, flag === '--seed' ? 8 : 20)}...`);

    const result = await this.startedContainer.exec([
      TOOLKIT_BIN,
      'show-wallet',
      '--src-url',
      env.getNodeWebsocketBaseURL(),
      flag,
      value,
    ]);

    if (result.exitCode !== 0) {
      const errorMessage = result.stderr || result.output || 'Unknown error occurred';
      throw new Error(
        `Toolkit show-wallet command failed with exit code ${result.exitCode}: ${errorMessage}`,
      );
    }

    // Parse the output to extract the JSON object(s)
    // The output may contain text before the JSON (e.g., "fetching 0x...", "sync cache...")
    const output = result.output.trim();
    return this.parseWalletState(output, stateType);
  }

  /**
   * Show private wallet state from a wallet seed.
   * This method queries the private wallet state including coins, UTXOs, and dust UTXOs.
   *
   * @param walletSeed - The wallet seed to query private wallet state for (required)
   *
   * @returns A promise that resolves to the private wallet state object containing coins, utxos, and dust_utxos.
   * @throws Error if the container is not started or if the show-wallet command fails.
   */
  async showPrivateWalletState(walletSeed: string): Promise<PrivateWalletState> {
    return (await this.executeShowWallet(
      '--seed',
      walletSeed,
      'private',
      'Querying private wallet state for wallet seed',
    )) as PrivateWalletState;
  }

  /**
   * Show public wallet state from a wallet address.
   * This method queries the public wallet state for the specified address.
   *
   * @param walletAddress - The wallet address to query public wallet state for (required)
   *
   * @returns A promise that resolves to the public wallet state object.
   * @throws Error if the container is not started or if the show-wallet command fails.
   */
  async showPublicWalletState(walletAddress: string): Promise<PublicWalletState> {
    return (await this.executeShowWallet(
      '--address',
      walletAddress,
      'public',
      'Querying public wallet state for wallet address',
    )) as PublicWalletState;
  }

  /**
   * Get DUST balance for a wallet seed.
   * This method queries the current DUST balance and generation information for the specified wallet.
   * The toolkit output may contain a full structure with generation_infos, source, and total,
   * or only a source object (map of nonces to values). In the latter case, the method constructs
   * a DustBalance object with empty generation_infos and calculates the total from source values.
   *
   * @param walletSeed - The wallet seed to query DUST balance for (required)
   *
   * @returns A promise that resolves to the dust balance object containing generation_infos, source, and total.
   *          The total field can be accessed directly: `const balance = await toolkit.getDustBalance(seed); const total = balance.total;`
   * @throws Error if the container is not started or if the dust-balance command fails.
   */
  async getDustBalance(walletSeed: string): Promise<DustBalance> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    log.debug(`Querying dust balance for wallet seed: ${walletSeed.substring(0, 8)}...`);

    const result = await this.startedContainer.exec([
      TOOLKIT_BIN,
      'dust-balance',
      '--src-url',
      env.getNodeWebsocketBaseURL(),
      '--seed',
      walletSeed,
    ]);

    if (result.exitCode !== 0) {
      const errorMessage = result.stderr || result.output || 'Unknown error occurred';
      throw new Error(
        `Toolkit dust-balance command failed with exit code ${result.exitCode}: ${errorMessage}`,
      );
    }

    // Parse the output to extract the JSON object(s)
    // The output may contain text before the JSON, and may have multiple JSON objects
    const output = result.output.trim();
    const jsonObjects = this.extractJsonObjects(output);

    if (jsonObjects.length === 0) {
      throw new Error(
        'Could not parse dust-balance output: no JSON object found ' +
          '(the toolkit usually emits this when it could not reach the node / produce a balance).\n' +
          `${DUST_BALANCE_EXPECTED}\n` +
          `Actual toolkit output:\n${output.substring(0, 1000)}`,
      );
    }

    // Try to find the JSON object matching the full schema, capturing per-object validation
    // errors so a mismatch can be reported precisely (expected vs actual) instead of cryptically.
    const fullSchemaErrors: string[] = [];
    for (const jsonString of jsonObjects) {
      let parsed: unknown;
      try {
        parsed = JSON.parse(jsonString);
      } catch {
        continue; // not valid JSON (e.g. a Rust Debug-formatted struct), skip
      }
      const validation = DustBalanceSchema.safeParse(parsed);
      if (validation.success) {
        return validation.data;
      }
      fullSchemaErrors.push(this.formatZodIssues(validation.error));
    }

    // Fallback: the toolkit may emit only the `source` map. Accept that shape and synthesise
    // the rest. Capture why it failed too, for the error message below.
    const lastJsonString = jsonObjects[jsonObjects.length - 1];
    let sourceOnlyError = 'last JSON object was not parseable as JSON';
    try {
      const parsed: unknown = JSON.parse(lastJsonString);
      const sourceValidation = DustBalanceSchema.shape.source.safeParse(parsed);

      if (sourceValidation.success && sourceValidation.data) {
        const total = Object.values(sourceValidation.data).reduce((sum, val) => sum + val, 0);
        return {
          generation_infos: [],
          source: sourceValidation.data,
          total: total,
        };
      }
      sourceOnlyError = this.formatZodIssues(sourceValidation.error);
    } catch {
      // keep the default sourceOnlyError
    }

    // Nothing matched — build an actionable expected-vs-actual error.
    const actual = jsonObjects
      .map((j, i) => `  [${i}] ${j.length > 1000 ? `${j.slice(0, 1000)}… (truncated)` : j}`)
      .join('\n');
    const fullErrText = fullSchemaErrors.length
      ? fullSchemaErrors.map((e, i) => `    object[${i}]: ${e}`).join('\n')
      : '    (no JSON object was parseable)';
    throw new Error(
      `dust-balance output did not match the expected schema (found ${jsonObjects.length} JSON object(s)).\n` +
        `${DUST_BALANCE_EXPECTED}\n` +
        `Why it failed:\n` +
        `  - full-schema validation errors (per object):\n${fullErrText}\n` +
        `  - source-only fallback error: ${sourceOnlyError}\n` +
        `Actual JSON object(s) received:\n${actual}`,
    );
  }

  /**
   * Format Zod validation issues as a compact, readable `path: message` list, e.g.
   * `source.6fa1…: Invalid`.
   */
  private formatZodIssues(error: z.ZodError): string {
    return error.issues
      .map((issue) => `${issue.path.length ? issue.path.join('.') : '<root>'}: ${issue.message}`)
      .join('; ');
  }

  /**
   * Generate and submit a single shielded or unshielded transaction
   *
   * @param sourceSeed - The source seed to use
   * @param addressType - The address type to use
   * @param destinationAddress - The destination address to use
   * @param amount - The amount to use
   * @param tokenType - Optional 32-byte hex token type to transfer. Omit for the
   *                    toolkit default, which is the all-zeros native token (NIGHT).
   *
   * @returns The transaction result
   */
  async generateSingleTx(
    sourceSeed: string,
    addressType: AddressType,
    destinationAddress: string,
    amount: number,
    tokenType?: string,
  ): Promise<ToolkitTransactionResult> {
    // When TX_BACKEND=moth, build and submit through moth's sync engine instead
    // of the toolkit container. Transfers only; every other operation stays on
    // the toolkit. The container need not be started for this path.
    if (env.getTxBackend() === 'moth') {
      return generateSingleTxViaMoth(
        sourceSeed,
        addressType,
        destinationAddress,
        amount,
        tokenType,
      );
    }

    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    const result = await this.startedContainer.exec([
      TOOLKIT_BIN,
      'generate-txs',
      '--src-url',
      env.getNodeWebsocketBaseURL(),
      '--dest-url',
      env.getNodeWebsocketBaseURL(),
      'single-tx',
      '--source-seed',
      sourceSeed,
      '--destination-address',
      destinationAddress,
      `--${addressType}-amount`,
      amount.toString(),
      ...(tokenType ? [`--${addressType}-token-type`, tokenType] : []),
    ]);

    log.debug(`Generate single transaction output:\n${result.output}`);

    if (result.exitCode !== 0) {
      const errorMessage = result.stderr || result.output || 'Unknown error occurred';
      throw new Error(`Toolkit command failed with exit code ${result.exitCode}: ${errorMessage}`);
    }

    const rawOutput = result.output.trim();
    return this.parseTransactionOutput(rawOutput);
  }

  /**
   * Extract the contract address from a deployed transaction file.
   * This method uses the toolkit's contract-address command to retrieve either a tagged
   * or untagged contract address.
   *
   * @param contractFile - The name of the contract transaction file (e.g., 'deploy_tx.mn')
   *                       located in the toolkit's output directory (/out/).
   * @param tagType - The format of the address to retrieve: 'tagged' includes the prefix,
   *                  'untagged' returns only the hex address.
   * @returns A promise that resolves to the contract address string in the requested format.
   * @throws Error if the container is not started or if the contract-address command fails.
   */
  async getContractAddress(contractFile: string, tagType: 'tagged' | 'untagged'): Promise<string> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }
    const addressResult = await this.startedContainer.exec([
      TOOLKIT_BIN,
      'contract-address',
      ...(tagType === 'tagged' ? ['--tagged'] : []),
      '--src-file',
      `/out/${contractFile}`,
    ]);
    log.debug(`contract-address taggedAddress:\n${JSON.stringify(addressResult, null, 2)}`);
    if (addressResult.exitCode !== 0) {
      const e = addressResult.stderr || addressResult.output || 'Unknown error';
      throw new Error(`contract-address failed: ${e}`);
    }

    return addressResult.output.trim();
  }

  /**
   * Call a smart contract function by generating and submitting a circuit transaction.
   * This method retrieves the current contract state, generates a circuit intent for the specified
   * contract call, converts it to a transaction, and submits it to the network.
   *
   * Matches all-contract-actions.test.ts: uses --src-url for chain context and optional --funding-seed.
   *
   * @param callKey - The contract function to call (e.g., 'store', 'increment'). Defaults to 'increment'.
   * @param deploymentResult - The deployment result object from deployContract. The contract-address-untagged will be extracted.
   * @param rngSeed - The random number generator seed for the transaction. Defaults to a fixed seed.
   * @param fundingSeed - Optional funding seed for the call wallet. When provided, uses --funding-seed (matches all-contract-actions).
   * @returns A promise that resolves to the transaction result containing the transaction hash,
   *          optional block hash, and submission status.
   * @throws Error if the container is not started or if any step in the contract call process fails.
   */
  async callContract(
    callKey: string = 'increment',
    deploymentResult: DeployContractResult,
    rngSeed: string = DEFAULT_RNG_SEED,
    fundingSeed?: string,
  ): Promise<ToolkitTransactionResult> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    if (!deploymentResult?.['contract-address-untagged']) {
      log.error('Deployment result is missing or has no contract address.');
      throw new Error(
        'Deployment result with contract-address-untagged is required. Ensure deployContract() succeeded before calling callContract().',
      );
    }

    const contractAddressUntagged = deploymentResult['contract-address-untagged'];
    const txFileName = `${callKey}_tx.mn`;
    const txFile = `/out/${txFileName}`;

    const callGenerateArgs = [
      ...this.buildGenerateTxBase(txFile, 'call'),
      '--call-key',
      callKey,
      '--contract-address',
      contractAddressUntagged,
      '--rng-seed',
      rngSeed,
    ];
    if (fundingSeed != null && fundingSeed !== '') {
      callGenerateArgs.push('--funding-seed', fundingSeed);
    }

    log.info(`Generating ${callKey} contract call...`);
    await this.execToolkit(callGenerateArgs, 'Failed to generate contract call');

    log.info('Submitting transaction to network...');
    const rawOutput = await this.sendGeneratedTx(txFileName);
    const result = this.parseTransactionOutput(rawOutput);
    await resolveBlockHash(result);
    return result;
  }

  /**
   * Run contract maintenance (update): change contract authority and submit in one toolkit command.
   * Uses execToolkit and parseTransactionOutput; maintenance does not use a separate generate-then-send step.
   *
   * @param deploymentResult - From deployContract; provides contract-address-untagged.
   * @param fundingSeed - Optional funding seed. When provided, uses --funding-seed (required on preprod/qanet).
   * @param newAuthoritySeed - Seed for the new authority. Defaults to DEFAULT_NEW_AUTHORITY_SEED.
   * @returns Transaction result (txHash, blockHash, status).
   */
  async updateContract(
    deploymentResult: DeployContractResult,
    fundingSeed?: string,
    newAuthoritySeed: string = DEFAULT_NEW_AUTHORITY_SEED,
  ): Promise<ToolkitTransactionResult> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    if (!deploymentResult?.['contract-address-untagged']) {
      log.error('Deployment result is missing or has no contract address.');
      throw new Error(
        'Deployment result with contract-address-untagged is required. Ensure deployContract() succeeded before calling updateContract().',
      );
    }

    const contractAddressUntagged = deploymentResult['contract-address-untagged'];
    const rpcUrl = this.getRpcUrl();

    const maintenanceArgs = [
      TOOLKIT_BIN,
      'generate-txs',
      CONTRACT_SIMPLE,
      'maintenance',
      '--contract-address',
      contractAddressUntagged,
      '--new-authority-seed',
      newAuthoritySeed,
      '--src-url',
      rpcUrl,
      '--dest-url',
      rpcUrl,
    ];
    if (fundingSeed != null && fundingSeed !== '') {
      maintenanceArgs.push('--funding-seed', fundingSeed);
    }

    log.info('Running contract maintenance (update)...');
    const execResult = await this.execToolkit(maintenanceArgs, 'contract maintenance failed');
    const result = this.parseTransactionOutput(execResult.output.trim());
    await resolveBlockHash(result);
    return result;
  }

  /**
   * Deploy a smart contract to the network.
   * This method generates a deployment intent, converts it to a transaction, submits it to the network,
   * and retrieves both tagged and untagged contract addresses.
   *
   * When running against preprod/qanet (or any env where the default wallet is not funded), pass a
   * funding seed via dataProvider.getFundingSeed() so the deploy uses a funded wallet.
   *
   * @param fundingSeed - Optional funding seed for the deploy wallet. When provided, uses --funding-seed
   *                      (matches all-contract-actions.test.ts). When omitted, uses --rng-seed only (backward compat).
   * @returns A promise that resolves to the deployment result containing untagged address, tagged address, and coin public key.
   * @throws Error if the container is not started or if any step in the deployment process fails.
   */
  async deployContract(fundingSeed?: string): Promise<DeployContractResult> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }

    const deployTxFileName = 'deploy_tx.mn';
    const deployTxFile = `/out/${deployTxFileName}`;
    const outDir = this.config.targetDir!;
    const outDeployTx = join(outDir, deployTxFileName);

    const coinPublicSeed = '0000000000000000000000000000000000000000000000000000000000000001';
    const addressInfo = await this.showAddress(coinPublicSeed);
    const coinPublic = addressInfo.coinPublic;

    const deployGenerateArgs = [
      ...this.buildGenerateTxBase(deployTxFile, 'deploy'),
      ...(fundingSeed != null && fundingSeed !== ''
        ? ['--funding-seed', fundingSeed]
        : ['--rng-seed', DEFAULT_RNG_SEED]),
    ];

    await this.execToolkit(deployGenerateArgs, 'contract-simple deploy failed');

    log.debug(`Checking for output files: ${outDeployTx} exists: ${fs.existsSync(outDeployTx)}`);
    if (!fs.existsSync(outDeployTx)) {
      throw new Error('contract-simple deploy did not produce expected output file');
    }

    await this.sendGeneratedTx(deployTxFileName);

    const contractAddressTagged = await this.getContractAddress(deployTxFileName, 'tagged');
    const contractAddressUntagged = await this.getContractAddress(deployTxFileName, 'untagged');
    const { txHash, blockHash } = await getContractDeploymentHashes(contractAddressUntagged);

    const deploymentResult = {
      'contract-address-untagged': contractAddressUntagged,
      'contract-address-tagged': contractAddressTagged,
      'coin-public': coinPublic,
      'deploy-tx-hash': txHash,
      'deploy-block-hash': blockHash,
    };

    log.debug(`Contract address info:\n${JSON.stringify(deploymentResult, null, 2)}`);

    return deploymentResult;
  }

  // SCAFFOLD for #1253 (for @whankinsiv). Mirrors midnight-node toolkit-tokens-minter-e2e.sh
  // + minter.compact (mintUnshieldedToSelfTest = the #1245 reporter's scenario).
  // TODO(#1253): make the compiled minter contract reachable in the container (set the
  // MinterContract paths) and validate on ledger-8 (ledger-9 needs a v9 compactc).

  /** Deploy minter, mint `mintAmount` to self, send `sendAmount` (< mintAmount) out; the
   * contract keeps `mintAmount - sendAmount`. */
  async deployMintSendUnshielded(opts: MinterFlowOptions): Promise<MinterFlowResult> {
    if (!this.startedContainer) {
      throw new Error('Container is not started. Call start() first.');
    }
    if (opts.sendAmount >= opts.mintAmount) {
      throw new Error('sendAmount must be < mintAmount to leave a non-zero contract remainder');
    }

    const seed = opts.fundingSeed ?? DEFAULT_FUNDING_SEED;
    const network = opts.network ?? 'undeployed'; // TODO(#1253): use the env's network for deployed runs.
    const { compiledContractDir, configFile, toolkitJsPath } = opts.contract;
    const out = (file: string) => `/out/${file}`;
    const rpcUrl = this.getRpcUrl();

    const { coinPublic } = await this.showAddress(seed);

    // 1. Deploy the minter: generate deploy intent → send-intent → submit.
    await this.execToolkit(
      [
        TOOLKIT_BIN,
        'generate-intent',
        'deploy',
        '-c',
        configFile,
        '--toolkit-js-path',
        toolkitJsPath,
        '--coin-public',
        coinPublic,
        '--output-intent',
        out('deploy.bin'),
        '--output-private-state',
        out('initial_state.json'),
        '--output-zswap-state',
        out('deploy_zswap.json'),
      ],
      'minter generate-intent deploy failed',
    );
    await this.execToolkit(
      [
        TOOLKIT_BIN,
        'send-intent',
        '--intent-file',
        out('deploy.bin'),
        '--compiled-contract-dir',
        compiledContractDir,
        '--dest-file',
        out('deploy.mn'),
      ],
      'minter send-intent (deploy) failed',
    );
    await this.execToolkit(
      [TOOLKIT_BIN, 'generate-txs', '--src-file', out('deploy.mn'), 'send', '-d', rpcUrl],
      'minter deploy submit failed',
    );

    // 2. Resolve the contract address, its on-chain state, and the unshielded token type.
    const contractAddress = (
      await this.execToolkit(
        [TOOLKIT_BIN, 'contract-address', '--src-file', out('deploy.mn')],
        'minter contract-address failed',
      )
    ).output.trim();
    await this.execToolkit(
      [
        TOOLKIT_BIN,
        'contract-state',
        '--contract-address',
        contractAddress,
        '--dest-file',
        out('state.mn'),
      ],
      'minter contract-state failed',
    );
    const tokenType = (
      await this.execToolkit(
        [
          TOOLKIT_BIN,
          'show-token-type',
          '--contract-address',
          contractAddress,
          '--domain-sep',
          opts.domainSep,
          '--unshielded',
        ],
        'minter show-token-type failed',
      )
    ).output.trim();
    const userAddress = (
      await this.execToolkit(
        [TOOLKIT_BIN, 'show-address', '--network', network, '--seed', seed, '--unshielded'],
        'minter show-address (unshielded) failed',
      )
    ).output.trim();

    // 3. Mint to self, then send a portion out — threading on-chain/private state across
    //    the two intents exactly as the shell script does.
    await this.execToolkit(
      [
        TOOLKIT_BIN,
        'generate-intent',
        'circuit',
        '-c',
        configFile,
        '--toolkit-js-path',
        toolkitJsPath,
        '--coin-public',
        coinPublic,
        '--input-onchain-state',
        out('state.mn'),
        '--input-private-state',
        out('initial_state.json'),
        '--contract-address',
        contractAddress,
        '--output-intent',
        out('mint_unshielded.bin'),
        '--output-onchain-state',
        out('state_after_mint.mn'),
        '--output-private-state',
        out('private_after_mint.json'),
        '--output-zswap-state',
        out('mint_zswap.json'),
        'mintUnshieldedToSelfTest',
        opts.domainSep,
        String(opts.mintAmount),
      ],
      'minter mintUnshieldedToSelfTest failed',
    );
    await this.execToolkit(
      [
        TOOLKIT_BIN,
        'generate-intent',
        'circuit',
        '-c',
        configFile,
        '--toolkit-js-path',
        toolkitJsPath,
        '--coin-public',
        coinPublic,
        '--input-onchain-state',
        out('state_after_mint.mn'),
        '--input-private-state',
        out('private_after_mint.json'),
        '--contract-address',
        contractAddress,
        '--output-intent',
        out('send_unshielded.bin'),
        '--output-onchain-state',
        out('state_after_send.mn'),
        '--output-private-state',
        out('private_after_send.json'),
        '--output-zswap-state',
        out('send_zswap.json'),
        'sendUnshieldedToUser',
        tokenType,
        userAddress,
        String(opts.sendAmount),
      ],
      'minter sendUnshieldedToUser failed',
    );

    // 4. Submit the combined mint+send tx; reuse the existing parser for hash/block.
    const submit = await this.execToolkit(
      [
        TOOLKIT_BIN,
        'send-intent',
        '--intent-file',
        out('mint_unshielded.bin'),
        '--intent-file',
        out('send_unshielded.bin'),
        '--compiled-contract-dir',
        compiledContractDir,
        '--dest-url',
        rpcUrl,
      ],
      'minter send-intent (mint+send) failed',
    );
    const mintSendTx = this.parseTransactionOutput(submit.output.trim());

    return {
      contractAddress,
      tokenType,
      mintAmount: opts.mintAmount,
      sendAmount: opts.sendAmount,
      expectedRemainder: opts.mintAmount - opts.sendAmount,
      mintSendTx,
    };
  }

  /**
   * Deploy a custom compiled Compact contract.
   *
   * Unlike {@link deployContract}, which drives the toolkit's built-in
   * `contract-simple`, this goes through the lower-level custom-contract path:
   * `generate-intent deploy` produces an intent, `generate-txs contract-custom`
   * turns it into a transaction, and the shared `sendGeneratedTx` submits it —
   * so contract-address extraction and hash parsing stay identical to the
   * built-in path.
   *
   * Requires `customContractDir` to have been passed to the constructor.
   *
   * @param contract - Which config/managed pair inside `customContractDir` to use.
   * @param constructorArgs - Arguments forwarded to the Compact constructor.
   * @param fundingSeed - Seed of the wallet that funds the deployment.
   * @returns The deployed contract's tagged/untagged addresses and deploy hashes.
   */
  async deployCustomContract(
    contract: CustomContractSpec,
    constructorArgs: string[] = [],
    fundingSeed?: string,
  ): Promise<DeployContractResult> {
    this.assertCustomContractMounted();

    // Pinned, not derived from `fundingSeed` — same rule as `deployContract`. A
    // compiled contract's toolkit-js config hard-codes the coin public key it
    // was built for, so deriving this from whatever seed happens to be funding
    // the deployment would silently disagree with the fixture the moment
    // FUNDING_SEED_<ENV> is set.
    const { coinPublic } = await this.showAddress(DEFAULT_COIN_PUBLIC_SEED);

    // Namespaced per contract so two custom contracts driven by one wrapper
    // never overwrite each other's intent, transaction or private state.
    const prefix = this.customFilePrefix(contract);
    const intentFile = `/out/${prefix}_deploy_intent.mn`;
    const deployTxFileName = `${prefix}_deploy_tx.mn`;
    const privateStateFile = this.customPrivateStateFile(contract);

    await this.execToolkit(
      [
        TOOLKIT_BIN,
        'generate-intent',
        'deploy',
        '--toolkit-js-path',
        TOOLKIT_JS_PATH,
        '--config',
        this.customConfigPath(contract),
        '--coin-public',
        coinPublic,
        '--network',
        env.getNetworkId().toLowerCase(),
        '--output-intent',
        intentFile,
        '--output-private-state',
        `/out/${privateStateFile}`,
        '--output-zswap-state',
        `/out/${prefix}_deploy_zswap.state`,
        ...constructorArgs,
      ],
      'custom contract deploy intent generation failed',
    );

    this.assertOutputFile(privateStateFile, 'custom contract deploy intent');

    await this.execToolkit(
      [
        ...this.buildCustomContractTxBase(`/out/${deployTxFileName}`, contract),
        '--intent-file',
        intentFile,
        '--zswap-state-file',
        `/out/${prefix}_deploy_zswap.state`,
        ...(fundingSeed != null && fundingSeed !== '' ? ['--funding-seed', fundingSeed] : []),
      ],
      'custom contract deploy tx generation failed',
    );

    this.assertOutputFile(deployTxFileName, 'custom contract deploy');

    await this.sendGeneratedTx(deployTxFileName);

    const contractAddressTagged = await this.getContractAddress(deployTxFileName, 'tagged');
    const contractAddressUntagged = await this.getContractAddress(deployTxFileName, 'untagged');
    const { txHash, blockHash } = await getContractDeploymentHashes(contractAddressUntagged);

    return {
      'contract-address-untagged': contractAddressUntagged,
      'contract-address-tagged': contractAddressTagged,
      'coin-public': coinPublic,
      'deploy-tx-hash': txHash,
      'deploy-block-hash': blockHash,
    };
  }

  /**
   * Snapshot a contract's current on-chain state to a file inside the container.
   *
   * The returned path is what {@link generateCustomContractCall} takes as its
   * `onchainStateFile`. Reusing one snapshot for two calls is what makes the
   * second call *stale*: the ledger re-runs its transcript against the state as
   * it is at apply time, not the state the proof was built against.
   *
   * @param contractAddressUntagged - Untagged hex contract address.
   * @param fileName - Name of the snapshot file to write under `/out`.
   * @returns The in-container path of the snapshot.
   */
  async snapshotContractState(
    contractAddressUntagged: string,
    fileName = 'contract_state.bin',
  ): Promise<string> {
    await this.execToolkit(
      [
        TOOLKIT_BIN,
        'contract-state',
        '--src-url',
        this.getRpcUrl(),
        '--contract-address',
        contractAddressUntagged,
        '--dest-file',
        `/out/${fileName}`,
      ],
      'contract-state snapshot failed',
    );
    this.assertOutputFile(fileName, 'contract-state snapshot');
    return `/out/${fileName}`;
  }

  /**
   * Build (but do not submit) a call to a circuit of a custom contract.
   *
   * Generation is split from submission so a test can build several calls
   * against the *same* on-chain state snapshot before any of them is applied.
   *
   * @param options - Circuit, deployment, contract spec and state snapshot.
   * @returns A handle to pass to {@link sendCustomContractCall}.
   */
  async generateCustomContractCall(options: {
    circuitId: string;
    deploymentResult: DeployContractResult;
    contract: CustomContractSpec;
    onchainStateFile: string;
    label: string;
    callArgs?: string[];
    fundingSeed?: string;
  }): Promise<CustomContractCall> {
    this.assertCustomContractMounted();

    const {
      circuitId,
      deploymentResult,
      contract,
      onchainStateFile,
      label,
      callArgs = [],
      fundingSeed,
    } = options;

    const contractAddress = deploymentResult['contract-address-untagged'];
    if (!contractAddress) {
      throw new Error('Deployment result is missing contract-address-untagged');
    }

    const prefix = this.customFilePrefix(contract);
    const intentFile = `/out/${prefix}_${label}_intent.mn`;
    const zswapStateFile = `/out/${prefix}_${label}_zswap.state`;
    const txFileName = `${prefix}_${label}_tx.mn`;

    await this.execToolkit(
      [
        TOOLKIT_BIN,
        'generate-intent',
        'circuit',
        '--src-url',
        this.getRpcUrl(),
        '--toolkit-js-path',
        TOOLKIT_JS_PATH,
        '--config',
        this.customConfigPath(contract),
        '--contract-address',
        contractAddress,
        '--coin-public',
        deploymentResult['coin-public'],
        '--network',
        env.getNetworkId().toLowerCase(),
        '--input-onchain-state',
        onchainStateFile,
        '--input-private-state',
        this.customPrivateStateFilePath(contract),
        '--output-intent',
        intentFile,
        // Every call reads the deploy-time private state and writes its own
        // successor, which nothing then consumes — the private-state chain is
        // deliberately not carried forward. That is only sound for a contract
        // with vacant witnesses, like the fixtures this path currently drives;
        // a contract with real witness state would need the output of one call
        // fed in as the input of the next.
        '--output-private-state',
        `/out/${prefix}_${label}_priv.state`,
        '--output-zswap-state',
        zswapStateFile,
        circuitId,
        ...callArgs,
      ],
      `custom contract circuit intent generation failed (${label})`,
    );

    await this.execToolkit(
      [
        ...this.buildCustomContractTxBase(`/out/${txFileName}`, contract),
        '--intent-file',
        intentFile,
        '--zswap-state-file',
        zswapStateFile,
        ...(fundingSeed != null && fundingSeed !== '' ? ['--funding-seed', fundingSeed] : []),
      ],
      `custom contract call tx generation failed (${label})`,
    );

    this.assertOutputFile(txFileName, `custom contract call (${label})`);

    return { label, txFileName };
  }

  /**
   * Submit a call previously built by {@link generateCustomContractCall}.
   *
   * @param call - Handle returned by {@link generateCustomContractCall}.
   * @returns The transaction hash and block hash reported by the toolkit.
   */
  async sendCustomContractCall(call: CustomContractCall): Promise<ToolkitTransactionResult> {
    const rawOutput = await this.sendGeneratedTx(call.txFileName);
    const result = this.parseTransactionOutput(rawOutput);
    await resolveBlockHash(result);
    return result;
  }

  /**
   * Decode a generated transaction with the toolkit's own deserializer.
   *
   * This is deliberately independent of the indexer: it is how a test confirms
   * the fixture really produced the transcript shape it intended (for example a
   * non-empty guaranteed transcript) before asserting anything about what the
   * indexer reports.
   *
   * @param call - Handle returned by {@link generateCustomContractCall}.
   * @returns The decoded transaction as text.
   */
  async showTransaction(call: CustomContractCall): Promise<string> {
    const result = await this.execToolkit(
      [TOOLKIT_BIN, 'show-transaction', '--src-file', `/out/${call.txFileName}`],
      `show-transaction failed (${call.label})`,
    );
    return result.output;
  }

  /**
   * True when the decoded transaction carries a non-empty guaranteed transcript,
   * i.e. the ledger's partition algorithm kept a guaranteed prefix for the call.
   *
   * Throws when the field is absent altogether. A caller asserting "no
   * guaranteed transcript" cannot tell a genuine `None` from output this
   * function failed to understand, so an unrecognised shape has to be loud:
   * silently returning `false` would let that assertion pass for the wrong
   * reason.
   *
   * @param decodedTransaction - Output of {@link showTransaction}.
   * @throws Error if the output carries no `guaranteed_transcript` field.
   */
  static hasGuaranteedTranscript(decodedTransaction: string): boolean {
    // `Some()` is matched ahead of `Some(` so an empty transcript is not read as a present one.
    const match = /guaranteed_transcript:\s*(None|Some\(\s*\)|Some\()/.exec(
      stripAnsi(decodedTransaction),
    );
    if (!match) {
      throw new Error(
        'show-transaction output carries no guaranteed_transcript field. The toolkit output ' +
          'format has changed; this check would otherwise report "no guaranteed transcript" ' +
          'for every call.',
      );
    }
    return match[1] === 'Some(';
  }

  /**
   * Fail early when the running toolkit image cannot execute a contract
   * compiled against `requiredRuntime`.
   *
   * Compiled Compact code declares the `@midnight-ntwrk/compact-runtime`
   * version it needs, and the toolkit image bundles a fixed set of them.
   * Compiling against a version the image does not carry still produces a
   * working-looking contract — the mismatch only surfaces much later, deep in
   * a call, as `Version mismatch: compiled code expects X, runtime is Y`.
   * Checking up front turns that into an actionable message naming what the
   * image does provide.
   *
   * @param requiredRuntime - Runtime version the compiled contract declares.
   * @throws Error if the image provides runtimes and this one is not among them.
   */
  async assertCompactRuntimeSupported(requiredRuntime: string): Promise<void> {
    const { output } = await this.execToolkit(
      ['sh', '-c', TOOLKIT_JS_RUNTIME_PROBE],
      'probing the toolkit-js compact runtimes failed',
    );

    const provided = [...stripAnsi(output).matchAll(/"version":\s*"([^"]+)"/g)].map(
      (match) => match[1],
    );

    if (provided.length === 0) {
      log.warn(
        `Toolkit image ${this.config.nodeToolkitTag} exposes no compact-runtime package; ` +
          `cannot verify that it can run a contract built for runtime ${requiredRuntime}`,
      );
      return;
    }

    if (provided.includes(requiredRuntime)) {
      log.debug(`Toolkit image provides compact-runtime ${requiredRuntime}`);
      return;
    }

    throw new Error(
      `Toolkit image ${this.config.nodeToolkitTag} cannot run a contract built for ` +
        `compact-runtime ${requiredRuntime}. It provides: ${[...new Set(provided)].sort().join(', ')}. ` +
        'Set COMPACT_COMPILER_VERSION to a compactc release targeting one of those, or use a ' +
        'toolkit image that carries this runtime.',
    );
  }

  private assertCustomContractMounted(): void {
    if (!this.config.customContractDir) {
      throw new Error(
        'ToolkitWrapper was constructed without customContractDir; ' +
          'custom-contract commands need the compiled contract mounted into the toolkit-js tree.',
      );
    }
  }

  /**
   * A filename prefix unique to one contract, derived from its config file, so
   * files two contracts write under `/out` never collide.
   */
  private customFilePrefix(contract: CustomContractSpec): string {
    return contract.configFile.replace(/\.config\.ts$/, '').replace(/[^A-Za-z0-9_-]/g, '_');
  }

  private customPrivateStateFile(contract: CustomContractSpec): string {
    return `${this.customFilePrefix(contract)}_private.state`;
  }

  private customPrivateStateFilePath(contract: CustomContractSpec): string {
    return `/out/${this.customPrivateStateFile(contract)}`;
  }

  /**
   * The config file must be addressed *inside* the toolkit-js tree: it imports
   * `@midnight-ntwrk/compact-js`, which only resolves from toolkit-js' own
   * node_modules.
   */
  private customConfigPath(contract: CustomContractSpec): string {
    return `${CUSTOM_CONTRACT_MOUNT}/${contract.configFile}`;
  }

  private buildCustomContractTxBase(destFile: string, contract: CustomContractSpec): string[] {
    return [
      TOOLKIT_BIN,
      'generate-txs',
      '--src-url',
      this.getRpcUrl(),
      '--dest-file',
      destFile,
      CONTRACT_CUSTOM,
      '--compiled-contract-dir',
      `${CUSTOM_CONTRACT_MOUNT}/${contract.managedDir ?? DEFAULT_MANAGED_DIR}`,
    ];
  }

  private assertOutputFile(fileName: string, context: string): void {
    const hostPath = join(this.config.targetDir!, fileName);
    if (!fs.existsSync(hostPath)) {
      throw new Error(`${context} did not produce expected output file: ${fileName}`);
    }
  }
}

/** Paths (as seen inside the toolkit container) to the compiled minter contract assets. */
interface MinterContract {
  compiledContractDir: string;
  configFile: string;
  toolkitJsPath: string;
}

interface MinterFlowOptions {
  contract: MinterContract;
  domainSep: string;
  mintAmount: number;
  sendAmount: number;
  fundingSeed?: string;
  network?: string;
}

interface MinterFlowResult {
  contractAddress: string;
  tokenType: string;
  mintAmount: number;
  sendAmount: number;
  expectedRemainder: number;
  mintSendTx: ToolkitTransactionResult;
}

export { ToolkitWrapper, ToolkitConfig };
export type {
  Coin,
  DustBalance,
  DustOutput,
  PrivateWalletState,
  PublicWalletState,
  Utxo,
  MinterContract,
  MinterFlowOptions,
  MinterFlowResult,
};
