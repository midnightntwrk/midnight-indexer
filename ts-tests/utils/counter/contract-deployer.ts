import * as fs from 'fs';
import * as path from 'path';
import { GenericContainer, Wait } from 'testcontainers';
import * as api from './api';
import {
  QanetRemoteConfig,
  StandaloneConfig,
  Testnet02RemoteConfig,
  TestnetRemoteConfig,
  currentDir,
  type Config,
} from './config';
import { createLogger } from './logger-utils';

const logDir = path.resolve(currentDir, '..', 'logs', 'tests', `${new Date().toISOString().replaceAll(':', '_')}.log`);
const logger = createLogger(logDir);
api.setLogger(logger);

export async function deployCounterContract(
  testWalletSeed: string,
  indexerUrl?: string,
  indexerWsUrl?: string,
  nodeUrl?: string,
) {
  const { providers, wallet, container } = await interactionSetup(testWalletSeed, indexerUrl, indexerWsUrl, nodeUrl);
  try {
    const startTime = performance.now();
    logger.info(`Deploying contract`);
    const deployedContract = await api.deploy(providers);
    const endTime = performance.now();
    const executionTime = endTime - startTime;
    logger.info(`Execution time: ${executionTime}`);
    // Save txHash into a file
    const deployedContractTxHash = deployedContract.deployTxData.public.txHash;
    fs.writeFileSync(path.join(currentDir, 'deployedContractTxHash.txt'), deployedContractTxHash);
  } catch (e) {
    logger.error(e);
  } finally {
    await api.saveState(wallet, `${testWalletSeed}-${process.env.ENV_TYPE}.state`);
    if (wallet !== null) await wallet.close();
    if (container) await container.stop();
  }
}

export async function increaseCounterContract(
  contractAddress: string,
  testWalletSeed: string,
  indexerUrl?: string,
  indexerWsUrl?: string,
  nodeUrl?: string,
) {
  const { providers, wallet, container } = await interactionSetup(testWalletSeed, indexerUrl, indexerWsUrl, nodeUrl);
  try {
    const startTime = performance.now();
    logger.info(`Increase contract`);
    const deployedContract = await api.joinContract(providers, contractAddress);
    await api.increment(deployedContract);
    const endTime = performance.now();
    const executionTime = endTime - startTime;
    logger.info(`Execution time: ${executionTime}`);
  } catch (e) {
    logger.error(e);
  } finally {
    await api.saveState(wallet, `${testWalletSeed}-${process.env.ENV_TYPE}.state`);
    if (wallet !== null) await wallet.close();
    if (container) await container.stop();
  }
}

async function interactionSetup(
  testWalletSeed: string,
  indexerUrl: string | undefined,
  indexerWsUrl: string | undefined,
  nodeUrl: string | undefined,
) {
  logger.info(`Test wallet seed: ${testWalletSeed}`);
  // Set up endpoints and config
  const dappConfig = getDappConfig();
  if (indexerUrl !== undefined) dappConfig.indexer = `${indexerUrl}/api/v1/graphql`;
  if (indexerWsUrl !== undefined) dappConfig.indexerWS = `${indexerWsUrl}/api/v1/graphql/ws`;
  if (nodeUrl !== undefined) dappConfig.node = nodeUrl;
  logger.info('Proof server starting...');
  const container = await new GenericContainer('ghcr.io/midnight-ntwrk/proof-server:3.0.6')
    .withName('proof-server')
    .withNetworkMode('midnight-net')
    .withExposedPorts(6300)
    .withCommand([`midnight-proof-server --network ${dappConfig.proofServerNetworkId} --verbose`])
    .withEnvironment({ RUST_BACKTRACE: '1' })
    .withWaitStrategy(Wait.forLogMessage('Actix runtime found; starting in Actix runtime', 1))
    .start();
  dappConfig.proofServer = `http://${container.getHost()}:${container.getMappedPort(6300).toString()}`;
  logger.info(`${JSON.stringify(dappConfig)}`);
  logger.info('Started');
  logger.info('Setting up wallet');
  const wallet = await api.buildWalletAndWaitForFunds(
    dappConfig,
    testWalletSeed,
    `${testWalletSeed}-${process.env.ENV_TYPE}.state`,
  );
  const providers = await api.configureProviders(wallet, dappConfig);
  return { providers, wallet, container };
}

export function getDappConfig() {
  let cfg: Config = new QanetRemoteConfig();
  let env = '';
  if (process.env.ENV_TYPE !== undefined) {
    env = process.env.ENV_TYPE;
  } else {
    env = 'qanet';
    logger.warn(`ENV_TYPE environment variable is not defined. Defaults to: ${env}`);
  }
  switch (env) {
    case 'qanet':
      cfg = new QanetRemoteConfig();
      break;
    case 'testnet':
      cfg = new TestnetRemoteConfig();
      break;
    case 'testnet02':
      cfg = new Testnet02RemoteConfig();
      break;
    case 'docker':
      cfg = new StandaloneConfig();
      break;
    case 'compose':
      cfg = new StandaloneConfig();
      break;
    default:
      throw new Error(`Unknown env value=${env}`);
  }

  return cfg;
}
