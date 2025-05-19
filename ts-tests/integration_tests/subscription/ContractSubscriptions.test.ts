import { jest } from '@jest/globals';
import * as fs from 'fs';
import * as path from 'path';
import WebSocket from 'ws';
import { type TestEnv } from '../../environment/ITestEnv';
import { getTestServerEnv } from '../../environment/ServerEnvFactory';
import { type Environment } from '../../environment/envConfig';
import { getTxInfo } from '../../helpers/QueryHelpers';
import {
  contractSubscription,
  contractSubscriptionNoOffsets,
  contractSubscriptionWithTransactionOffset,
  emptyContractSubscription,
  initConnection,
} from '../../helpers/SubscriptionHelpers';
import { Commons } from '../../utils/Commons';
import { deployCounterContract, increaseCounterContract } from '../../utils/counter/contract-deployer';

describe.skip(`not able to deploy contract - Indexer Contract Subscription Tests - ${process.env.ENV_TYPE} ${process.env.DEPLOYMENT}`, () => {
  jest.setTimeout(1200000);

  let client: WebSocket;
  let serverEnv: TestEnv;
  let address: string;
  let height: string;
  let blockHash: string;
  let txHash: string;
  let identifiers: string[];
  let state: string;
  let envConfig: Environment;

  beforeAll(async () => {
    serverEnv = getTestServerEnv();
    await serverEnv.serverReady();
    envConfig = serverEnv.getEnvConfig();
    // Deploy counter contract
    const currentDir = path.resolve(new URL(import.meta.url).pathname, '..');
    const filePath = path.join(currentDir, '..', '..', 'utils', 'counter', 'deployedContractTxHash.txt');
    if (Commons.isFileEmpty(filePath))
      await deployCounterContract(
        envConfig.wallets[0].seed as string,
        serverEnv.getUrl(),
        serverEnv.getWsUrl(),
        envConfig.substrate_node_ws_url.replace(/^ws/, 'http'),
      );
    txHash = fs.readFileSync(filePath, 'utf8');
    console.log(`txHash: ${txHash}`);

  }, 1200000);

  afterAll(async () => {
    await serverEnv.tearDownServer();
  });

  beforeEach((done) => {
    client = new WebSocket(`${serverEnv.getWsUrl()}/api/v1/graphql/ws`, ['graphql-ws']);
    client.on('open', done);
  });

  afterEach((done) => {
    if (client?.readyState === WebSocket.OPEN) {
      client.terminate();
    }
    done();
  });

  test.skip('not able to deploy contract - Verify successful contract subscription with block height offset', async () => {

    allure.description(`Subscribes to contracts by height and verifies the response.`);
    allure.tms('PM-7263', 'PM-7263');
    allure.severity('blocker');
    allure.tag('subscription');

    await initConnection(client);
    // We retrieve the information needed from the deployed contract
    const txOffset = `(hash: "${txHash}")`;
    const txInfo = await getTxInfo(client, txOffset);
    address = txInfo.payload.data.transactions[0].contractCalls[0].address;
    state = txInfo.payload.data.transactions[0].contractCalls[0].state;
    height = txInfo.payload.data.transactions[0].block.height;
    blockHash = txInfo.payload.data.transactions[0].block.hash;
    identifiers = txInfo.payload.data.transactions[0].identifiers;

    const offset = `{height: ${height}}`;
    const response = await contractSubscription(client, '11', address, offset);
    expect(response.payload.data).toEqual({ contract: { address: `${address}`, state: `${state}` } });

  });

  test.skip('not able to deploy contract - Verify successful contract subscription with block hash offset', async () => {

    allure.description(`Subscribes to contracts by hash and verifies the response.`);
    allure.tms('PM-7265', 'PM-7265');
    allure.severity('blocker');
    allure.tag('subscription');

    await initConnection(client);
    const offset = `{hash: \"${blockHash}\"}`;
    const response = await contractSubscription(client, '12', address, offset);
    expect(response.payload.data).toEqual({ contract: { address: `${address}`, state: `${state}` } });

  });

  test.skip('not able to deploy contract - Verify contract subscription without offsets', async () => {

    allure.description(`Subscribes to contracts without offsets and verifies the response.`);
    allure.tms('PM-7271', 'PM-7271');
    allure.severity('blocker');
    allure.tag('subscription');

    await initConnection(client);
    const response = await contractSubscriptionNoOffsets(client, '13', address);
    expect(response.payload.data.contract.address).toEqual(`${address}`);
    expect(response.payload.data.contract.state).toMatch(/^[0-9a-f]+$/);

  });

  test.skip('not able to deploy contract - Verify that contracts subscription waits for data when contract is not at the specified block height', async () => {

    allure.description(`Subscribes to contracts with heights not containing contract, and waits for data to come through the WS connection.`);
    allure.tms('PM-7266', 'PM-7266');
    allure.severity('blocker');
    allure.tag('subscription');

    await initConnection(client);
    const heights = ['0', '9007199254740991'];
    for (const height of heights) {
      const offset = `{height: ${height}}`;
      const response = await emptyContractSubscription(client, '21', address, offset);
      expect(response).toEqual('no_data');
    }

  });

  test.skip('not able to deploy contract - Verify that contracts subscription waits for data when contract is not at the specified block hash', async () => {

    allure.description(`Subscribes to contracts with hash not containing contract, and waits for data to come through the WS connection.`);
    allure.tms('PM-7267', 'PM-7267');
    allure.severity('blocker');
    allure.tag('subscription');

    await initConnection(client);
    const hashValues = ['0', 'someHash'];
    for (const hash of hashValues) {
      const offset = `{hash: \"${hash}\"}`;
      const response = await emptyContractSubscription(client, '22', address, offset);
      expect(response).toEqual('no_data');
    }

  });

  test.skip('not able to deploy contract - Verify that subscribing with invalid contract address returns no data', async () => {

    allure.tms('PM-7270', 'PM-7270');
    allure.description(`Subscribes to contracts with invalid address, and waits for data to come through the WS connection.`);
    allure.severity('blocker');
    allure.tag('subscription');

    await initConnection(client);
    const invalidAddress = 'invalidAddress';
    const offset = `{height: 0}`;
    const response = await emptyContractSubscription(client, '25', invalidAddress, offset);
    expect(response).toEqual('no_data');

  });

  test.skip('not able to deploy contract - Verify that interacting with contract while subscribing with block hash returns new data', async () => {

    allure.tms('PM-9523', 'PM-9523');
    allure.description(`Subscribes to contract with block hash, increases counter contract and waits for new data to come through the WS connection.`);
    allure.severity('blocker');
    allure.tag('subscription');

    await initConnection(client);
    const offset = `{hash: \"${blockHash}\"}`;
    await contractSubscription(client, '13', address, offset);
    // Wait for new data to come through
    const receivedNewDataPromise = getNewDataPromise(client, address, '13');
    // Increase counter contract
    const increaseCounterContractPromise = getIncreaseCounterContractPromise();
    await Promise.all([receivedNewDataPromise, increaseCounterContractPromise]);

  });

  test.skip('not able to deploy contract - Verify that contract subscription works with transactionOffset', async () => {

    allure.description(`Subscribes to contract with transactionOffset, increases counter contract and waits for new data to come through the WS connection.`);
    allure.tms('PM-9523', 'PM-9523');
    allure.severity('blocker');
    allure.tag('subscription');

    await initConnection(client);
    const transactionOffset = `{identifier: \"${identifiers[0]}\"}`;
    await contractSubscriptionWithTransactionOffset(client, '14', address, transactionOffset);
    // Wait for new data to come through
    const receivedNewDataPromise = getNewDataPromise(client, address, '14');
    // Increase counter contract
    const increaseCounterContractPromise = getIncreaseCounterContractPromise();
    await Promise.all([receivedNewDataPromise, increaseCounterContractPromise]);

  });

  function getNewDataPromise(client: WebSocket, address: string, messageId: string) {
    return new Promise((resolve) => {
      client.on('message', (data: WebSocket.Data) => {
        const message = typeof data === 'string' ? data : data.toString(); // eslint-disable-line @typescript-eslint/no-base-to-string
        const parsedMessage = JSON.parse(message);
        if (parsedMessage.id === messageId) {
          expect(parsedMessage.payload.data.contract.address).toEqual(address);
          resolve(true);
        }
      });
    });
  }

  function getIncreaseCounterContractPromise() {
    return new Promise((resolve, reject) => {
      increaseCounterContract(
        address.substring(2),
        envConfig.wallets[0].seed as string,
        serverEnv.getUrl(),
        serverEnv.getWsUrl(),
        envConfig.substrate_node_ws_url.replace(/^ws/, 'http'),
      )
        .then(() => {
          resolve(true);
        })
        .catch((error) => {
          reject(error);
        });
    });
  }

});
