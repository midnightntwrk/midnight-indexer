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
  UNDEPLOYED = 'undeployed',
  QANET = 'qanet',
  NODEDEV01 = 'nodedev01',
  DEVNET = 'devnet',
  PREVIEW = 'preview',
  TESTNET = 'testnet',
  TESTNET02 = 'testnet02',
}

export enum LedgerNetworkId {
  UNDEPLOYED = 'undeployed',
  DEVNET = 'devnet',
  TESTNET = 'testnet',
}

export const networkIdByEnvName: Record<string, string> = {
  undeployed: 'Undeployed',
  qanet: 'Devnet',
  nodedev01: 'Devnet',
  devnet: 'Devnet',
  preview: 'Devnet',
  testnet: 'Testnet',
  testnet02: 'Testnet',
};

export const bech32mTagsByLedgerNetworkId: Record<string, string> = {
  undeployed: 'undeployed',
  devnet: 'dev',
  testnet: 'test',
};

const indexerHostByEnvName: Record<string, string> = {
  undeployed: 'localhost:8088',
  qanet: 'indexer.qanet.dev.midnight.network',
  nodedev01: 'indexer.node-dev-01.dev.midnight.network',
  devnet: 'indexer.devnet.midnight.network',
  preview: 'indexer.preview.midnight.network',
  testnet: 'indexer.testnet.midnight.network',
  testnet02: 'indexer.testnet-02.midnight.network',
};

const nodeHostByEnvName: Record<string, string> = {
  undeployed: 'localhost:9944',
  qanet: 'rpc.qanet.dev.midnight.network',
  nodedev01: 'rpc.node-dev-01.dev.midnight.network',
  devnet: 'rpc.devnet.midnight.network',
  preview: 'rpc.preview.midnight.network',
  testnet: 'rpc.testnet.midnight.network',
  testnet02: 'rpc.testnet-02.midnight.network',
};

export class Environment {
  private readonly envName: string;
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
    this.networkId = networkIdByEnvName[this.envName];
    this.indexerHost = indexerHostByEnvName[this.envName];
    this.nodeHost = nodeHostByEnvName[this.envName];

    // What we are actually doing here is the following:
    // 1. If the NODE_TAG is specified as an environment variable, use it. otherwise
    // we read the NODE_VERSION file and use the version from the file.
    // 2. If the NODE_TOOLKIT_VERSION is specified as an environment variable, use it. otherwise
    // we use the same version as the NODE_TAG.
    const supportedNodeVersion = fs.readFileSync('../../NODE_VERSION', 'utf8').trim();
    this.nodeTag = process.env.NODE_TAG || supportedNodeVersion;
    this.nodeToolkitTag = process.env.NODE_TOOLKIT_TAG || supportedNodeVersion;
    log.debug(`Using NODE_TAG: ${this.nodeTag}`);
    log.debug(`Using NODE_TOOLKIT_TAG: ${this.nodeTag}`);
  }

  isUndeployedEnv(): boolean {
    return this.isUndeployed;
  }

  getEnvName(): string {
    return this.envName;
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

  getBech32mTagByLedgerNetworkId(networkId: string): string {
    return bech32mTagsByLedgerNetworkId[networkId];
  }

  getNodeVersion(): string {
    return this.nodeTag;
  }

  getNodeToolkitVersion(): string {
    return this.nodeToolkitTag;
  }
}

export const env = new Environment();
