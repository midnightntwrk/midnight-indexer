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

// An on-demand proof server for the moth transaction backend.
//
// WHY NOT docker-compose. The proof server is a dependency of *our wallet*, not
// of the chain stack: it proves transactions we build locally. Deployed runs
// (preview, preprod, …) never bring up compose at all, yet they still need it,
// so it cannot live in `docker-compose.yaml`. This mirrors
// `utils/toolkit/toolkit-cache.ts`: a named container, started once, reused by
// whichever worker gets there first.
//
// Only started when TX_BACKEND=moth. The toolkit backend proves inside its own
// container and never calls in here.

import { execFile } from 'child_process';
import { createServer } from 'net';
import { promisify } from 'util';

const execFileAsync = promisify(execFile);

const CONTAINER_NAME = 'midnight-proof-server';
const IMAGE_REPO = 'midnightntwrk/proof-server';
const INTERNAL_PORT = 6300;
const READY_TIMEOUT_MS = 120_000;
const READY_POLL_INTERVAL_MS = 1_000;

/**
 * Ledger train this test suite's wallet stack is built against.
 *
 * It is NOT guessed from NODE_TAG: nothing in this repo maps a node tag to a
 * ledger version, and a guess that is wrong fails deep inside proving. It is
 * read from the one thing we do control — the `@midnight-ntwrk/ledger-v8`
 * override pinned in `package.json`, which is also what keeps a single physical
 * ledger copy in `node_modules`. If the target node runs a different ledger
 * train, that override has to move too, and `PROOF_SERVER_TAG` is the escape
 * hatch in the meantime.
 */
const DEFAULT_PROOF_SERVER_TAG = '8.1.0';

const proofServerTag = (): string =>
  process.env.PROOF_SERVER_TAG?.trim() || DEFAULT_PROOF_SERVER_TAG;

/** Set when this process started the container, so teardown only stops its own. */
let startedByUs = false;
let ensured: Promise<string> | undefined;

/**
 * Return a usable proof server URL, starting a container only if needed.
 *
 * `PROOF_SERVER_URL` short-circuits everything: if it is set we start nothing
 * and stop nothing. That is how a manually-run proof server keeps working, and
 * how CI will point at a service container.
 */
export async function ensureProofServer(): Promise<string> {
  const explicit = process.env.PROOF_SERVER_URL?.trim();
  if (explicit) {
    await assertReachable(explicit);
    console.log(`[PROOF] Using PROOF_SERVER_URL=${explicit} (no container started)`);
    return explicit;
  }

  if (!ensured) {
    ensured = bootstrap().catch((err) => {
      ensured = undefined;
      throw err;
    });
  }
  return ensured;
}

/**
 * Stop the proof server, but only if this process started it. A server the
 * caller supplied through PROOF_SERVER_URL, or one another worker already had
 * running, is left alone.
 */
export async function stopProofServer(): Promise<void> {
  if (!startedByUs) return;
  startedByUs = false;
  ensured = undefined;
  try {
    await execFileAsync('docker', ['rm', '-f', CONTAINER_NAME]);
    console.log(`[PROOF] Stopped ${CONTAINER_NAME}`);
  } catch {
    // Already gone, or removed by another worker — nothing to do.
  }
}

async function bootstrap(): Promise<string> {
  const existing = await inspectContainer();
  if (existing) {
    if (!existing.running) await execFileAsync('docker', ['start', CONTAINER_NAME]);
    const port = existing.port ?? (await inspectContainer())?.port;
    if (!port) {
      throw new Error(`Could not determine host port for the existing ${CONTAINER_NAME} container`);
    }
    const url = `http://127.0.0.1:${port}`;
    await waitForReady(url);
    console.log(`[PROOF] Reusing existing ${CONTAINER_NAME} at ${url}`);
    return url;
  }

  const tag = proofServerTag();
  const port = await getFreePort();
  try {
    await execFileAsync('docker', [
      'run',
      '-d',
      '--name',
      CONTAINER_NAME,
      '-p',
      `127.0.0.1:${port}:${INTERNAL_PORT}`,
      // No command: the image is entrypointed to start the server on its own
      // port 6300. It is `bash -c`, so an argument appended here would be
      // taken as $0 rather than a flag.
      `${IMAGE_REPO}:${tag}`,
    ]);
    startedByUs = true;
  } catch (err) {
    const message = errorMessage(err);
    // Race: another worker started it between our inspect and our run.
    if (isNameConflict(message)) {
      const raced = await inspectContainer();
      if (raced?.port) {
        if (!raced.running) await execFileAsync('docker', ['start', CONTAINER_NAME]);
        const url = `http://127.0.0.1:${raced.port}`;
        await waitForReady(url);
        console.log(`[PROOF] Adopted ${CONTAINER_NAME} at ${url} after a start race`);
        return url;
      }
    }
    throw new Error(
      `Could not start ${IMAGE_REPO}:${tag}. Check the tag exists, or set PROOF_SERVER_URL ` +
        `to a proof server you are running yourself. Docker said: ${message}`,
    );
  }

  const url = `http://127.0.0.1:${port}`;
  await waitForReady(url);
  console.log(`[PROOF] Started ${CONTAINER_NAME} (${IMAGE_REPO}:${tag}) at ${url}`);
  return url;
}

/**
 * A proof server whose ledger train does not match the node's cannot prove a
 * transaction for it — an 8.x server against a ledger-9 node fails inside
 * proving, with a stack trace that reads like a wallet bug. Say it plainly.
 */
export function proofServerMismatchHint(): string {
  return (
    `The proof server must match the node's ledger train (currently ${proofServerTag()}). ` +
    'If the target environment runs a different ledger version, set PROOF_SERVER_TAG ' +
    '(and the @midnight-ntwrk/ledger-v8 override in package.json) to that train.'
  );
}

async function assertReachable(url: string): Promise<void> {
  try {
    await probe(url);
  } catch (err) {
    throw new Error(
      `PROOF_SERVER_URL=${url} is not reachable. Start a proof server there, or unset ` +
        `PROOF_SERVER_URL to have one started for you. (${errorMessage(err)})`,
    );
  }
}

async function waitForReady(url: string): Promise<void> {
  const deadline = Date.now() + READY_TIMEOUT_MS;
  for (;;) {
    try {
      await probe(url);
      return;
    } catch (err) {
      if (Date.now() >= deadline) {
        throw new Error(
          `Proof server at ${url} did not become ready within ${READY_TIMEOUT_MS / 1000}s. ` +
            `${proofServerMismatchHint()} (${errorMessage(err)})`,
        );
      }
      await sleep(READY_POLL_INTERVAL_MS);
    }
  }
}

/** The proof server answers /health with a JSON status once it is serving. */
async function probe(url: string): Promise<void> {
  const res = await fetch(`${url.replace(/\/+$/, '')}/health`, {
    signal: AbortSignal.timeout(5_000),
  });
  if (!res.ok) throw new Error(`health returned HTTP ${res.status}`);
}

interface ContainerInfo {
  running: boolean;
  port?: number;
}

async function inspectContainer(): Promise<ContainerInfo | null> {
  try {
    const { stdout } = await execFileAsync('docker', [
      'inspect',
      '--format',
      `{{.State.Running}}|{{with index .NetworkSettings.Ports "${INTERNAL_PORT}/tcp"}}{{(index . 0).HostPort}}{{end}}`,
      CONTAINER_NAME,
    ]);
    const [runningStr, portStr] = stdout.trim().split('|');
    const port = portStr ? parseInt(portStr, 10) : undefined;
    return { running: runningStr === 'true', port: Number.isFinite(port) ? port : undefined };
  } catch {
    return null;
  }
}

async function getFreePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const srv = createServer();
    srv.unref();
    srv.on('error', reject);
    srv.listen(0, '127.0.0.1', () => {
      const addr = srv.address();
      if (addr && typeof addr === 'object') {
        const { port } = addr;
        srv.close(() => resolve(port));
      } else {
        srv.close();
        reject(new Error('Failed to allocate a free port'));
      }
    });
  });
}

function sleep(ms: number): Promise<void> {
  return new Promise((res) => setTimeout(res, ms));
}

function errorMessage(err: unknown): string {
  if (err && typeof err === 'object') {
    const e = err as { stderr?: string | Buffer; message?: string };
    return String(e.stderr ?? e.message ?? '').trim();
  }
  return String(err);
}

function isNameConflict(message: string): boolean {
  return (
    message.includes('is already in use by container') ||
    message.includes('Conflict. The container name') ||
    message.includes('already exists')
  );
}
