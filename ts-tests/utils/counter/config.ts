import { NetworkId, setNetworkId } from '@midnight-ntwrk/midnight-js-network-id';
import path from 'node:path';
export const currentDir = path.resolve(new URL(import.meta.url).pathname, '..');

export const contractConfig = {
  privateStateStoreName: 'counter-private-state',
  zkConfigPath: path.resolve(currentDir, '..', '..', 'utils', 'counter', 'contract', 'managed', 'counter'),
};

export interface Config {
  readonly logDir: string;
  indexer: string;
  indexerWS: string;
  node: string;
  proofServer: string;
  proofServerNetworkId: string;
}

export class DevnetLocalConfig implements Config {
  logDir = path.resolve(currentDir, '..', 'logs', 'devnet-local', `${new Date().toISOString()}.log`);
  indexer = 'http://127.0.0.1:8088/api/v1/graphql';
  indexerWS = 'ws://127.0.0.1:8088/api/v1/graphql/ws';
  node = 'http://127.0.0.1:9944';
  proofServer = 'http://127.0.0.1:6300';
  proofServerNetworkId = NetworkId.DevNet.toLowerCase();
  constructor() {
    setNetworkId(NetworkId.DevNet);
  }
}

export class TestnetLocalConfig implements Config {
  logDir = path.resolve(currentDir, '..', 'logs', 'testnet-local', `${new Date().toISOString()}.log`);
  indexer = 'http://127.0.0.1:8088/api/v1/graphql';
  indexerWS = 'ws://127.0.0.1:8088/api/v1/graphql/ws';
  node = 'http://127.0.0.1:9944';
  proofServer = 'http://127.0.0.1:6300';
  proofServerNetworkId = NetworkId.TestNet.toLowerCase();
  constructor() {
    setNetworkId(NetworkId.TestNet);
  }
}

export class StandaloneConfig implements Config {
  logDir = path.resolve(currentDir, '..', 'logs', 'standalone', `${new Date().toISOString()}.log`);
  indexer = 'http://127.0.0.1:8088/api/v1/graphql';
  indexerWS = 'ws://127.0.0.1:8088/api/v1/graphql/ws';
  node = 'http://127.0.0.1:9944';
  proofServer = 'http://127.0.0.1:6300';
  proofServerNetworkId = NetworkId.Undeployed.toLowerCase();
  constructor() {
    setNetworkId(NetworkId.Undeployed);
  }
}

export class TestnetRemoteConfig implements Config {
  logDir = path.resolve(currentDir, '..', 'logs', 'testnet-remote', `${new Date().toISOString()}.log`);
  indexer = 'https://indexer.testnet.midnight.network/api/v1/graphql';
  indexerWS = 'wss://indexer.testnet.midnight.network/api/v1/graphql/ws';
  node = 'https://rpc.testnet.midnight.network';
  proofServer = 'http://127.0.0.1:6300';
  proofServerNetworkId = NetworkId.TestNet.toLowerCase();
  constructor() {
    setNetworkId(NetworkId.TestNet);
  }
}

export class Testnet02RemoteConfig implements Config {
  logDir = path.resolve(currentDir, '..', 'logs', 'testnet-02-remote', `${new Date().toISOString()}.log`);
  indexer = 'https://indexer.testnet-02.midnight.network/api/v1/graphql';
  indexerWS = 'wss://indexer.testnet-02.midnight.network/api/v1/graphql/ws';
  node = 'https://rpc.testnet-02.midnight.network';
  proofServer = 'http://127.0.0.1:6300';
  proofServerNetworkId = NetworkId.TestNet.toLowerCase();
  constructor() {
    setNetworkId(NetworkId.TestNet);
  }
}

export class QanetRemoteConfig implements Config {
  logDir = path.resolve(currentDir, '..', 'logs', 'qanet-remote', `${new Date().toISOString()}.log`);
  indexer = 'https://indexer.qanet.dev.midnight.network/api/v1/graphql';
  indexerWS = 'wss://indexer.qanet.dev.midnight.network/api/v1/graphql/ws';
  node = 'https://rpc.qanet.dev.midnight.network';
  proofServer = 'http://127.0.0.1:6300';
  proofServerNetworkId = NetworkId.DevNet.toLowerCase();
  constructor() {
    setNetworkId(NetworkId.DevNet);
  }
}
