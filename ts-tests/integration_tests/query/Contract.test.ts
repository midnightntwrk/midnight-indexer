import { jest } from '@jest/globals';
import * as fs from 'fs';
import * as path from 'path';
import process from 'process';
import WebSocket from 'ws';
import { type TestEnv } from '../../environment/ITestEnv';
import { getTestServerEnv } from '../../environment/ServerEnvFactory';
import { type Environment } from '../../environment/envConfig';
import { getTxInfo } from '../../helpers/QueryHelpers';
import { initConnection } from '../../helpers/SubscriptionHelpers';
import { Commons } from '../../utils/Commons';
import { getResponseForQuery } from '../../utils/HttpRequestUtils';
import { deployCounterContract } from '../../utils/counter/contract-deployer';

describe.skip(`not able to deploy contract - Indexer Contract Query Tests - ${process.env.ENV_TYPE} ${process.env.DEPLOYMENT}`, () => {
  jest.setTimeout(1200000);

  let serverEnv: TestEnv;
  let client: WebSocket;
  let address: string;
  let blockHash: string;
  let txHash: string;
  let blockHeight: string;
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

  test('not able to deploy contract - Verify that querying a contract with a valid address returns correct response', async () => {

    allure.description(`Queries contract by address and verifies the response.`);
    allure.tms('PM-6138', 'PM-6138');
    allure.severity('blocker');
    allure.tag('query');

    await initConnection(client);
    const txOffset = `(hash: "${txHash}")`;
    const txInfo = await getTxInfo(client, txOffset);
    address = txInfo.payload.data.transactions[0].contractCalls[0].address;
    blockHash = txInfo.payload.data.transactions[0].block.hash;
    blockHeight = txInfo.payload.data.transactions[0].block.height;
    const queryBody = `query { contract ( address: "${address}" ) { address state transaction { hash } } }`;
    const response = await getResponseForQuery(`${serverEnv.getUrl()}/api/v1/graphql`, queryBody);

    expect(response.status).toBe(200);
    const responseBody = await response.body;
    const data = responseBody.data;
    expect(data.contract.address).toBe(address);
    expect(data.contract.state).toBeDefined();

  });

  test.skip('not able to deploy contract - Verify that querying contract by address and block hash returns correct response', async () => {

    allure.description(`Queries contract by address and offset hash expecting the contract address and state in response`);
    allure.tms('PM-9580', 'PM-9580');
    allure.severity('blocker');
    allure.tag('query');

    const queryBody = `query { contract ( address: "${address}", offset: { hash: "${blockHash}" } ) { address state } }`;
    const response = await getResponseForQuery(`${serverEnv.getUrl()}/api/v1/graphql`, queryBody);
    expect(response.status).toBe(200);
    const responseBody = await response.body;
    const data = responseBody.data;
    expect(data.contract.address).toBe(address);
    expect(data.contract.state).toBeDefined();

  });

  test.skip('not able to deploy contract - Verify that querying contract by address and block height returns correct response', async () => {

    allure.description(`Queries contract by address and block height, expecting the contract address and state in response`);
    allure.tms('PM-9581', 'PM-9581');
    allure.severity('blocker');
    allure.tag('query');

    const queryBody = `query { contract ( address: "${address}", offset: { height: ${blockHeight} } ) { address state } }`;
    const response = await getResponseForQuery(`${serverEnv.getUrl()}/api/v1/graphql`, queryBody);
    expect(response.status).toBe(200);
    const responseBody = await response.body;
    const data = responseBody.data;
    expect(data.contract.address).toBe(address);
    expect(data.contract.state).toBeDefined();

  });

  test.skip('not able to deploy contract - Verify that querying contract by block hash and block height simultaneously returns an error', async () => {

    allure.description(`Queries contract by address plus block height and hash expecting an error`);
    allure.tms('PM-7125', 'PM-7125');
    allure.severity('blocker');
    allure.tag('query');

    const queryBody = 'query { contract ( address: "0x10", offset: { height: 1, hash: "1" } ) { address state } }';
    const response = await getResponseForQuery(`${serverEnv.getUrl()}/api/v1/graphql`, queryBody);
    expect(response.status).toBe(200);
    const responseBody = await response.body;
    expect(responseBody.data.contract).toBeNull();
    expect(responseBody.errors[0].message).toMatch(
      'Query must provide either block or transaction properly configured offsets and not both',
    );

  });

});
