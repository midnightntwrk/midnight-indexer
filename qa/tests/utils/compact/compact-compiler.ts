// This file is part of midnightntwrk/midnight-indexer
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

import { execFile } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import log from '@utils/logging/logger';

const execFileAsync = promisify(execFile);

const HERE = path.dirname(fileURLToPath(import.meta.url));
const DOCKERFILE = path.join(HERE, 'compact-toolchain.Dockerfile');

/**
 * The compactc release fixtures are compiled with.
 *
 * Pinned rather than floating: compiled output declares the
 * `@midnight-ntwrk/compact-runtime` version it needs, and the toolkit image
 * ships only a fixed set of those runtimes. Use
 * {@link ToolkitWrapper.assertCompactCompilerSupported} to check the pin
 * against the toolkit image actually in use — otherwise a mismatch surfaces
 * much later as an opaque `Version mismatch: compiled code expects X,
 * runtime is Y`.
 *
 * A pre-release is a valid pin (`0.33.0-rc.2`): the toolchain image falls back
 * to fetching one when the toolchain manager does not offer the version, and a
 * newer toolkit can need a runtime no stable compiler emits yet.
 */
export const COMPACT_COMPILER_VERSION = process.env.COMPACT_COMPILER_VERSION ?? '0.30.0';

/** The `compact` toolchain-manager release providing the `compact update` that fetches the compiler. */
const COMPACT_MANAGER_VERSION = process.env.COMPACT_MANAGER_VERSION ?? '0.5.1';

/**
 * Built from {@link DOCKERFILE} on first use and reused from the local Docker
 * image cache afterwards. Set `COMPACT_TOOLCHAIN_IMAGE` to use a pre-built
 * image instead (it must already be present locally; nothing is built).
 */
const TOOLCHAIN_IMAGE =
  process.env.COMPACT_TOOLCHAIN_IMAGE ?? `compact-toolchain:${COMPACT_COMPILER_VERSION}`;

/** The toolchain image build downloads a Debian base plus the compiler. */
const IMAGE_BUILD_TIMEOUT_MS = 600_000;
const COMPILE_TIMEOUT_MS = 300_000;

/** Directory name compactc writes its output into, and what a fixture config imports from. */
const MANAGED_DIR = 'managed';

export interface CompileCompactOptions {
  /** Directory holding the Compact source and everything listed in {@link stage}. */
  sourceDir: string;
  /** The Compact source to compile, relative to {@link sourceDir}. */
  sourceFile: string;
  /**
   * Files copied next to the compiled output, relative to {@link sourceDir} —
   * in practice the toolkit-js `config.ts`, which imports
   * `./managed/contract/index.js` and so has to sit beside it.
   */
  stage?: string[];
}

/**
 * Compile a Compact fixture and return the directory to hand to
 * `ToolkitWrapper`'s `customContractDir`.
 *
 * Only the `.compact` source and its `config.ts` are committed; the compiled
 * output (prover keys, ZKIR, generated JS — near a megabyte of unreviewable
 * binary) is produced here instead. The compiler runs in a container, so the
 * host needs nothing but Docker.
 *
 * Output is cached under `.tmp/compact/<name>-<digest>`, where the digest
 * covers the compiler pin and the bytes of every input file. Editing the
 * source or moving the pin therefore lands in a fresh directory rather than
 * silently reusing stale output; an unchanged fixture recompiles zero times.
 *
 * @param options - The fixture's source directory and the files it is made of.
 * @returns Absolute path of the staged directory: the staged sources plus `managed/`.
 */
export async function compileCompactContract(options: CompileCompactOptions): Promise<string> {
  const { sourceDir, sourceFile, stage = [] } = options;
  const inputs = [sourceFile, ...stage].sort();

  const digest = createHash('sha256');
  digest.update(`${COMPACT_COMPILER_VERSION}\0${COMPACT_MANAGER_VERSION}\0${TOOLCHAIN_IMAGE}`);
  for (const input of inputs) {
    const absolute = path.join(sourceDir, input);
    if (!fs.existsSync(absolute)) {
      throw new Error(`Compact fixture is missing ${input}: ${absolute} does not exist`);
    }
    digest.update(`\0${input}\0`);
    digest.update(fs.readFileSync(absolute));
  }

  const name = path.basename(sourceFile, '.compact');
  const outputDir = path.resolve(`./.tmp/compact/${name}-${digest.digest('hex').slice(0, 12)}`);
  const marker = path.join(outputDir, '.compiled');

  if (fs.existsSync(marker)) {
    log.debug(`Compact fixture ${name} already compiled: ${outputDir}`);
    return outputDir;
  }

  // A directory without the marker is a partial run (interrupted, or a failed
  // compile). Its contents are root-owned, so hand ownership back first.
  if (fs.existsSync(outputDir)) {
    await relaxOwnership(outputDir);
    fs.rmSync(outputDir, { recursive: true, force: true });
  }

  fs.mkdirSync(outputDir, { recursive: true });
  for (const input of inputs) {
    fs.copyFileSync(path.join(sourceDir, input), path.join(outputDir, input));
  }

  await ensureToolchainImage();

  log.info(`Compiling Compact fixture ${name} with compactc ${COMPACT_COMPILER_VERSION}`);
  await dockerRun(
    ['-v', `${outputDir}:/work`, TOOLCHAIN_IMAGE, `/work/${sourceFile}`, `/work/${MANAGED_DIR}`],
    `compiling ${sourceFile}`,
    COMPILE_TIMEOUT_MS,
  );

  // compactc runs as root in the container; without this the host process
  // cannot clean the cache directory up again.
  await relaxOwnership(outputDir);

  const contractEntry = path.join(outputDir, MANAGED_DIR, 'contract', 'index.js');
  if (!fs.existsSync(contractEntry)) {
    throw new Error(
      `compactc produced no contract entry point for ${sourceFile}: ${contractEntry}`,
    );
  }

  fs.writeFileSync(marker, `${TOOLCHAIN_IMAGE}\n`);
  log.debug(`Compiled Compact fixture ${name}: ${outputDir}`);
  return outputDir;
}

/**
 * The `@midnight-ntwrk/compact-runtime` version the compiled contract asks
 * for, as recorded in its generated entry point.
 *
 * @param compiledDir - A directory returned by {@link compileCompactContract}.
 * @returns The runtime version, or `undefined` if the generated code does not declare one.
 */
export function compiledRuntimeVersion(compiledDir: string): string | undefined {
  const entry = path.join(compiledDir, MANAGED_DIR, 'contract', 'index.js');
  const match = /checkRuntimeVersion\('([^']+)'\)/.exec(fs.readFileSync(entry, 'utf8'));
  return match?.[1];
}

async function ensureToolchainImage(): Promise<void> {
  if (await dockerImageExists(TOOLCHAIN_IMAGE)) return;

  if (process.env.COMPACT_TOOLCHAIN_IMAGE) {
    throw new Error(
      `COMPACT_TOOLCHAIN_IMAGE=${TOOLCHAIN_IMAGE} is not present locally. ` +
        'Pull or build it first, or unset the variable to let the fixture build its own.',
    );
  }

  log.info(`Building ${TOOLCHAIN_IMAGE} (first use only; cached as a Docker image afterwards)`);
  await runDocker(
    [
      'build',
      '--file',
      DOCKERFILE,
      '--build-arg',
      `COMPACT_VERSION=${COMPACT_COMPILER_VERSION}`,
      '--build-arg',
      `COMPACT_MANAGER_VERSION=${COMPACT_MANAGER_VERSION}`,
      '--tag',
      TOOLCHAIN_IMAGE,
      HERE,
    ],
    `building ${TOOLCHAIN_IMAGE}`,
    IMAGE_BUILD_TIMEOUT_MS,
  );
}

async function dockerImageExists(image: string): Promise<boolean> {
  try {
    await execFileAsync('docker', ['image', 'inspect', image]);
    return true;
  } catch {
    return false;
  }
}

/** Give the compiled output back to the host user, so the cache stays removable. */
async function relaxOwnership(dir: string): Promise<void> {
  const uid = process.getuid?.();
  const gid = process.getgid?.();
  if (uid === undefined || gid === undefined) return;
  try {
    await dockerRun(
      [
        '--entrypoint',
        'chown',
        '-v',
        `${dir}:/work`,
        TOOLCHAIN_IMAGE,
        '-R',
        `${uid}:${gid}`,
        '/work',
      ],
      `restoring ownership of ${dir}`,
      COMPILE_TIMEOUT_MS,
    );
  } catch (error) {
    // Best effort: the compile itself succeeded, and a stale root-owned cache
    // directory is a cleanup nuisance rather than a test failure.
    log.warn(`Could not restore host ownership of ${dir}: ${error}`);
  }
}

function dockerRun(args: string[], context: string, timeout: number): Promise<void> {
  return runDocker(['run', '--rm', ...args], context, timeout);
}

async function runDocker(args: string[], context: string, timeout: number): Promise<void> {
  try {
    await execFileAsync('docker', args, { timeout, maxBuffer: 16 * 1024 * 1024 });
  } catch (error) {
    const details = error instanceof Error && 'stderr' in error ? String(error.stderr) : `${error}`;
    throw new Error(`docker failed while ${context}: ${details.trim() || error}`);
  }
}
