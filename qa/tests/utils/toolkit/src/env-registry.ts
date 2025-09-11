export type ChainId = "undeployed" | "nodedev01" | "devnet" | "qanet" | "testnet02";

export interface EnvConfig {
  // default RPCs (this is overridable)
  srcUrl: string;      // for toolkit -s
  destUrl: string;     // for toolkit -d
  networkId: "undeployed" | "devnet" | "testnet";
  // container we keep alive for this env (from scripts/toolkit-start)
  containerName: string;   // e.g. "toolkit-devnet"
  // optional docker network to join when talking to local node
  dockerNetwork?: string;  // e.g. "midnight-net" for undeployed
}

export const ENV: Record<ChainId, EnvConfig> = {
  undeployed: {
    // use container-to-container hostname, not localhost
    srcUrl:  "ws://midnight-node:9944",
    destUrl: "ws://midnight-node:9944",
    networkId: "undeployed",
    containerName: "toolkit-undeployed",
    dockerNetwork: "midnight-net",
  },
  nodedev01: {
    srcUrl:  "wss://rpc.node-dev-01.dev.midnight.network",
    destUrl: "wss://rpc.node-dev-01.dev.midnight.network",
    networkId: "devnet",
    containerName: "toolkit-nodedev01",
  },
  devnet: {
    srcUrl:  "wss://rpc.devnet.midnight.network",
    destUrl: "wss://rpc.devnet.midnight.network",
    networkId: "devnet",
    containerName: "toolkit-devnet",
  },
  qanet: {
    srcUrl:  "wss://rpc.qanet.dev.midnight.network",
    destUrl: "wss://rpc.qanet.dev.midnight.network",
    networkId: "devnet",
    containerName: "toolkit-qanet",
  },
  testnet02: {
    srcUrl:  "wss://rpc.testnet02.midnight.network",
    destUrl: "wss://rpc.testnet02.midnight.network",
    networkId: "testnet",
    containerName: "toolkit-testnet02",
  },
};