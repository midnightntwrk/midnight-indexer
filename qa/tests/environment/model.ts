import log from '@utils/logging/logger';

export enum EnvironmentName {
  UNDEPLOYED = 'undeployed',
  QANET = 'qanet',
  NODEDEV01 = 'nodedev01',
  DEVNET = 'devnet',
  TESTNET = 'testnet',
  TESTNET02 = 'testnet02',
}

export const networkIdByEnvName: Record<string, string> = {
  undeployed: 'Undeployed',
  qanet: 'Devnet',
  nodedev01: 'Devnet',
  devnet: 'Devnet',
  testnet: 'Testnet',
  testnet02: 'Testnet',
};

const indexerHostByEnvName: Record<string, string> = {
  undeployed: 'localhost:8088',
  qanet: 'indexer.qanet.dev.midnight.network',
  nodedev01: 'indexer.node-dev-01.dev.midnight.network',
  devnet: 'indexer.devnet.midnight.network',
  testnet: 'indexer.testnet.midnight.network',
  testnet02: 'indexer.testnet-02.midnight.network',
};

const nodeHostByEnvName: Record<string, string> = {
  undeployed: 'localhost:9944',
  qanet: 'rpc.qanet.dev.midnight.network',
  nodedev01: 'rpc.node-dev-01.dev.midnight.network',
  devnet: 'rpc.devnet.midnight.network',
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
    this.nodeTag = process.env.NODE_TAG || '0.16.3-72d4ac2e';
    log.debug(`Using NODE_TAG: ${this.nodeTag}`);
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

  getNodeVersion(): string {
    return this.nodeTag;
  }
}

export const env = new Environment();
