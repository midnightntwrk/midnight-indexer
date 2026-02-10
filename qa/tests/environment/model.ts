// This file is part of midnightntwrk/midnight-indexer.
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

import fs from 'fs';
import log from '@utils/logging/logger';

export enum EnvironmentName {
  MAINNET = 'mainnet',
  UNDEPLOYED = 'undeployed',
  NODEDEV01 = 'node-dev-01',
  DEVNET = 'devnet',
  QANET = 'qanet',
  QANET_DEV = 'qanet.dev',
  PREVIEW = 'preview',
  PREPROD = 'preprod',
  TESTNET = 'testnet',
  TESTNET02 = 'testnet02',
}

export enum CardanoNetwork {
  MAINNET = 'mainnet',
  PREVIEW = 'preview',
  PREPROD = 'preprod',
}

export enum CardanoNetworkType {
  MAINNET = 'mainnet',
  TESTNET = 'testnet',
}

type HostConfig = {
  indexerHost: string;
  nodeHost: string;
};

type HostEntry =
  | {
    env: EnvironmentName;
    indexerHost: string;
    nodeHost: string;
  }
  | {
    env: EnvironmentName;
    domain: string;
  };

const hostEntries: HostEntry[] = [
  {
    env: EnvironmentName.UNDEPLOYED,
    indexerHost: 'localhost:8088',
    nodeHost: 'localhost:9944',
  },
  { env: EnvironmentName.QANET, domain: 'qanet.midnight.network' },
  { env: EnvironmentName.QANET_DEV, domain: 'qanet.dev.midnight.network' },
  { env: EnvironmentName.NODEDEV01, domain: 'node-dev-01.dev.midnight.network' },
  { env: EnvironmentName.DEVNET, domain: 'devnet.midnight.network' },
  { env: EnvironmentName.PREVIEW, domain: 'preview.midnight.network' },
  { env: EnvironmentName.PREPROD, domain: 'preprod.midnight.network' },
  { env: EnvironmentName.TESTNET, domain: 'testnet.midnight.network' },
  { env: EnvironmentName.TESTNET02, domain: 'testnet-02.midnight.network' },
];

const hostConfigByEnvName: Record<EnvironmentName, HostConfig> = hostEntries.reduce(
  (config, entry) => {
    if ('domain' in entry) {
      config[entry.env] = {
        indexerHost: `indexer.${entry.domain}`,
        nodeHost: `rpc.${entry.domain}`,
      };
    } else {
      config[entry.env] = {
        indexerHost: entry.indexerHost,
        nodeHost: entry.nodeHost,
      };
    }
    return config;
  },
  {} as Record<EnvironmentName, HostConfig>,
);

/**
 * Resolves the supported node version for the current environment.
 *
 * This supports both NODE_VERSIONS (new, multi-version format) and
 * NODE_VERSION (legacy, single-version format), as different environments
 * still use different files.
 *
 * NOTE: Once all indexer environments are on >= 3.1.0-rc.1, support for
 * NODE_VERSION can be removed and this helper simplified.
 */
function readSupportedNodeVersion(): string {
  const versionsPath = '../../NODE_VERSIONS';
  const versionPath = '../../NODE_VERSION';

  if (fs.existsSync(versionsPath)) {
    const versions = fs
      .readFileSync(versionsPath, 'utf8')
      .split('\n')
      .map((v) => v.trim())
      .filter(Boolean);

    if (versions.length === 0) {
      throw new Error('NODE_VERSIONS file exists but is empty');
    }

    return versions[1]; // stable choice
  }

  if (fs.existsSync(versionPath)) {
    return fs.readFileSync(versionPath, 'utf8').trim();
  }

  throw new Error('Neither NODE_VERSIONS nor NODE_VERSION file found');
}

export class Environment {
  private readonly envName: EnvironmentName;
  private readonly isUndeployed: boolean;
  private readonly wsProtocol: string;
  private readonly httpProtocol: string;
  private readonly indexerHost: string;
  private readonly networkId: string;
  private readonly nodeHost: string;
  private readonly nodeTag: string;
  private readonly nodeToolkitTag: string;

  constructor() {
    // Setting up environment with error checking
    const rawEnv = process.env.TARGET_ENV;
    const validEnvNames = Object.values(EnvironmentName);

    if (!rawEnv || !validEnvNames.includes(rawEnv as EnvironmentName)) {
      throw new Error(
        `Invalid or missing TARGET_ENV: "${rawEnv}". ` +
        `Expected one of: \n  ${validEnvNames.map((name) => `"${name}"`).join(',\n  ')}`,
      );
    }
    this.envName = rawEnv as EnvironmentName;

    // Setting up the rest of the properties
    this.isUndeployed = this.envName === 'undeployed';
    if (this.isUndeployed) {
      this.wsProtocol = 'ws';
      this.httpProtocol = 'http';
    } else {
      this.wsProtocol = 'wss';
      this.httpProtocol = 'https';
    }

    // These should be now safe to assign as we already
    // checked envName
    this.networkId = this.envName;
    this.indexerHost = hostConfigByEnvName[this.envName].indexerHost;
    this.nodeHost = hostConfigByEnvName[this.envName].nodeHost;

    // What we are actually doing here is the following:
    // 1. If the NODE_TAG is specified as an environment variable, use it. otherwise
    // we read the NODE_VERSION file and use the version from the file.
    // 2. If the NODE_TOOLKIT_VERSION is specified as an environment variable, use it. otherwise
    // we use the same version as the NODE_TAG.
    const supportedNodeVersion = readSupportedNodeVersion();
    this.nodeTag = process.env.NODE_TAG || supportedNodeVersion;
    this.nodeToolkitTag = process.env.NODE_TOOLKIT_TAG || supportedNodeVersion;
    log.debug(`Using NODE_TAG: ${this.nodeTag}`);
    log.debug(`Using NODE_TOOLKIT_TAG: ${this.nodeToolkitTag}`);
  }

  isUndeployedEnv(): boolean {
    return this.isUndeployed;
  }

  getCurrentEnvironmentName(): EnvironmentName {
    return this.envName;
  }

  /**
   * Get the Cardano network connected to a given Midnight environment.
   * @param envName - The Midnight environment name to get the Cardano network for.
   *                  If not provided, the current environment name will be used.
   * @returns The Cardano network.
   */
  getCardanoNetwork(envName: EnvironmentName | undefined = undefined): CardanoNetwork {
    const targetenv: EnvironmentName = envName || this.getCurrentEnvironmentName();
    switch (targetenv) {
      case EnvironmentName.MAINNET:
        return CardanoNetwork.MAINNET;
      case EnvironmentName.PREPROD:
        return CardanoNetwork.PREPROD;
      case EnvironmentName.PREVIEW:
      case EnvironmentName.NODEDEV01:
      case EnvironmentName.QANET:
      case EnvironmentName.QANET_DEV:
        return CardanoNetwork.PREVIEW;
      default:
        throw new Error(`Unsupported environment name: ${this.envName}`);
    }
  }

  /**
   * Get the Cardano network type for a given Cardano network.
   * @param network - The Cardano network to get the type for.
   *                  If not provided, the current Cardano network will be used.
   * @returns The Cardano network type.
   */
  getCardanoNetworkType(network: CardanoNetwork | undefined = undefined): CardanoNetworkType {
    const cardanoNetwork = network || this.getCardanoNetwork();
    return cardanoNetwork === 'mainnet' ? CardanoNetworkType.MAINNET : CardanoNetworkType.TESTNET;
  }

  /**
   * Get all the known/supported Midnightenvironment names.
   * @returns All the environment names currently known/supported.
   */
  getAllEnvironmentNames(): string[] {
    return Object.values(EnvironmentName);
  }

  getNetworkId(): string {
    return this.networkId;
  }

  getIndexerHost(): string {
    return this.indexerHost;
  }

  getIndexerHttpBaseURL(): string {
    return `${this.httpProtocol}://${this.indexerHost}`;
  }

  getIndexerWebsocketBaseURL(): string {
    return `${this.wsProtocol}://${this.indexerHost}`;
  }

  getNodeWebsocketBaseURL(): string {
    return `${this.wsProtocol}://${this.nodeHost}`;
  }

  getNodeVersion(): string {
    return this.nodeTag;
  }

  getNodeToolkitVersion(): string {
    return this.nodeToolkitTag;
  }
}

export const env = new Environment();
