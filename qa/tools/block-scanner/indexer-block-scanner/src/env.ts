export const INDEXER_WS_URL: Record<string, string> = {
  undeployed: 'ws://localhost:8088/api/v1/graphql/ws',
  nodedev01: 'wss://indexer.node-dev-01.dev.midnight.network/api/v1/graphql/ws',
  devnet: 'wss://indexer.devnet.midnight.network/api/v1/graphql/ws',
  qanet: 'wss://indexer.qanet.dev.midnight.network/api/v1/graphql/ws',
  testnet02: 'wss://indexer.testnet-02.midnight.network/api/v1/graphql/ws',
};

export let TARGET_ENV: string;

if (process.env.TARGET_ENV === undefined || process.env.TARGET_ENV === '') {
  console.warn('TARGET_ENV not set, default to undeployed environment');
  TARGET_ENV = 'undeployed';
} else {
  TARGET_ENV = process.env.TARGET_ENV;
  console.info(`TARGET_ENV=${TARGET_ENV}`);
}
