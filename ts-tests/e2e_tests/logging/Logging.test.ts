import { jest } from '@jest/globals';
import * as fs from 'fs';
import * as path from 'path';
import { DockerComposeEnvironment, Wait, type StartedDockerComposeEnvironment } from 'testcontainers';
import WebSocket from 'ws';
import { environments, type Environment } from '../../environment/envConfig';
import { verifyThatLogIsPresent } from '../../helpers/LoggingHelpers';
import { getTxInfo } from '../../helpers/QueryHelpers';
import { getSessionId } from '../../helpers/SessionHelpers';
import {
  contractSubscriptionNoOffsets,
  emptyContractSubscription,
  initConnection,
} from '../../helpers/SubscriptionHelpers';
import { Commons } from '../../utils/Commons';
import { deployCounterContract } from '../../utils/counter/contract-deployer';

describe('Logging Test', () => {
  jest.setTimeout(120000);

  let composeEnvironment: StartedDockerComposeEnvironment;
  let client: WebSocket;
  let environment: Environment;
  const currentDir = path.resolve(new URL(import.meta.url).pathname, '..');
  const filePath = path.join(currentDir, '..', '..', 'utils', 'counter', 'deployedContractTxHash.txt');

  beforeAll(async () => {
    environment = environments.compose;
    composeEnvironment = await new DockerComposeEnvironment(
      './docker_composes/',
      'docker-compose-latest-cloud-indexer.yaml',
    )
      .withWaitStrategy('chain-indexer', Wait.forLogMessage(/block indexed.*\"height\"\s*:\s*("?0"?)/))
      .withWaitStrategy('indexer-api', Wait.forLogMessage('listening to TCP connections'))
      .withWaitStrategy('nats', Wait.forLogMessage('Listening for client connections on 0.0.0.0:4222'))
      .up();
  });

  afterAll(async () => {
    if (composeEnvironment) await composeEnvironment.down();
    if (!Commons.isFileEmpty(filePath)) fs.writeFileSync(filePath, '');
  });

  beforeEach((done) => {
    const portGraphqlApi = composeEnvironment.getContainer('indexer-api').getFirstMappedPort();
    client = new WebSocket(`${environment.indexer_ws_url}:${portGraphqlApi}/api/v1/graphql/ws`, ['graphql-ws']);
    client.on('open', done);
  });

  afterEach(() => {
    if (client?.readyState === WebSocket.OPEN) {
      client.terminate();
    }
  });

  test('Verify that block storage is logged correctly', async () => {

    allure.description(`Starts up latest Indexer locally, then verifies that the logs include block storage.`);
    allure.tms('PM-6573', 'PM-6573');
    allure.severity('normal');
    allure.tag('logging');
    const stream = await composeEnvironment.getContainer('chain-indexer').logs();

    await verifyThatLogIsPresent(
      stream,
      /\"message\":\"block indexed\",\"hash\":\"[a-f0-9]+\",\"height\"\s*:\s*("?[0-9]+"?),\"parent_hash\":\"[a-f0-9]+\"/,
      5000,
    );

  });

  test('Verify that session creation is logged correctly', async () => {

    allure.description(`Starts up latest Indexer locally, then verifies that the logs include session creation.`);
    allure.tms('PM-6574', 'PM-6574');
    allure.severity('normal');
    allure.tag('logging');

    await initConnection(client);

    await getSessionId(client, environment.wallets[0].viewingKey);
    await verifyThatLogIsPresent(
      await composeEnvironment.getContainer('wallet-indexer').logs(),
      /\"message\":\"wallet indexed\",\"session_id\":"SessionId\([a-f0-9]+…\)\",\"from\":1/,
      5000,
    );

  });

  test.skip('cannot deploy contract - Verify that contract subscription attempt are logged', async () => {

    allure.description(`Verifies that the logs include incorrect contract subscription.`);
    allure.tms('PM-6575', 'PM-6575');
    allure.severity('normal');
    allure.tag('logging');

    await initConnection(client);
    await emptyContractSubscription(client, '1', '0x00', '{height: 1}');
    await verifyThatLogIsPresent(
      await composeEnvironment.getContainer('indexer-api').logs(),
      /Contract retrieved by block height '0x00' at 1: empty/,
      5000,
    );

  });

  test.skip('cannot deploy contract - Verify that contract subscription is logged correctly', async () => {

    allure.description(`Deploys a contract, subscribes to the contract and then verifies that the logs include contract subscription.`);
    allure.tms('PM-6576', 'PM-6576');
    allure.severity('normal');
    allure.tag('logging');

    if (Commons.isFileEmpty(filePath))
      await deployCounterContract(environment.wallets[0].seed as string);

    const txHash = fs.readFileSync(filePath, 'utf8');

    await initConnection(client);
    const txInfo = await getTxInfo(client, `(hash: "${txHash}")`);
    const address = txInfo.payload.data.transactions[0].contractCalls[0].address as string;
    const response = await contractSubscriptionNoOffsets(client, '13', address);

    expect(response.payload.data.contract.address).toEqual(`${address}`);
    expect(response.payload.data.contract.state).toMatch(/^[0-9a-f]+$/);
    const streamGraphqlApi = await composeEnvironment.getContainer('indexer-api').logs();
    await verifyThatLogIsPresent(
      streamGraphqlApi,
      new RegExp(`Contract state stream started for address '${address}'`),
      5000,
    );

  });

  test('Verify that requests/responses are logged as json', async () => {

    allure.description(`Starts up latest Indexer locally, verifies that the logs are saved in json format.`);
    allure.tms('PM-9345', 'PM-9345');
    allure.severity('normal');
    allure.tag('logging');

    const expectedKeys = ['timestamp', 'level', 'message', 'target'];
    const containers = ['indexer-api', 'chain-indexer', 'wallet-indexer'];
    for (const containerName of containers) {
      const stream = await composeEnvironment.getContainer(containerName).logs();
      let allLinesAreJson = true;
      let allLinesAreValid = true;
      stream
        .on('data', (line) => {
          if (/cannot (touch|remove)/.test(line as string)) return;
          try {
            const logEntry = JSON.parse(line as string);
            const hasAllKeys = expectedKeys.every((key) => key in logEntry);
            if (!hasAllKeys) {
              console.log(`Invalid log format: ${line}`);
              allLinesAreValid = false;
            }
          } catch (error) {
            console.log(`Invalid JSON detected: ${line}`);
            allLinesAreJson = false;
          }
        })
        .on('end', () => {
          expect(allLinesAreJson).toBe(true);
          expect(allLinesAreValid).toBe(true);
        });
    }
  });

});
