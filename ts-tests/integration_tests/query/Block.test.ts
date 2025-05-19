import { jest } from '@jest/globals';
import WebSocket from 'ws';
import { type TestEnv } from '../../environment/ITestEnv';
import { getTestServerEnv } from '../../environment/ServerEnvFactory';
import { getBlockInfo } from '../../helpers/QueryHelpers';
import { initConnection } from '../../helpers/SubscriptionHelpers';
import { getResponseForQuery } from '../../utils/HttpRequestUtils';

describe(`Indexer Block Query via Websocket Tests - ${process.env.ENV_TYPE} ${process.env.DEPLOYMENT}`, () => {
  jest.setTimeout(120000);

  let serverEnv: TestEnv;
  let blockHash: string;
  let client: WebSocket;

  beforeAll(async () => {
    serverEnv = getTestServerEnv();
    await serverEnv.serverReady();
  });

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

  test('Verify that querying a block with a valid height works as expected', async () => {

    allure.description(`Queries block on 'height: 1' and verifies the response.`);
    allure.tms('PM-6134', 'PM-6134');
    allure.severity('blocker');
    allure.tag('query');

    await initConnection(client);
    const blockOffsetHeight = '(offset: {height: 0})';
    const blockInfo = await getBlockInfo(client, blockOffsetHeight);
    blockHash = blockInfo.payload.data.block.hash;
    expect(blockInfo).toBeDefined();
    expect(blockInfo.payload.data.block.height).toBe(0);
    expect(blockHash).toMatch(/^[a-f0-9]{64}$/);

  });

  test('Verify that querying a block with a valid hash works as expected', async () => {

    allure.description(`Queries block by 'hash: ${blockHash}' and verifies the response.`);
    allure.tms('PM-9577', 'PM-9577');
    allure.severity('blocker');
    allure.tag('query');

    await initConnection(client);
    const blockOffsetHash = `(offset: {hash: "${blockHash}"})`;
    const blockInfo = await getBlockInfo(client, blockOffsetHash);
    const blockByHash: string = blockInfo.payload.data.block.hash;
    expect(blockInfo).toBeDefined();
    expect(blockInfo).not.toBe('');
    expect(blockHash).toMatch(blockByHash);

  });

  test('Verify that querying a block by latest works as expected', async () => {

    allure.description(`Queries block by latest and verifies the response`);
    allure.tms('PM-9578', 'PM-9578');
    allure.severity('blocker');
    allure.tag('query');

    await initConnection(client);
    const blockInfo = await getBlockInfo(client);
    expect(blockInfo).toBeDefined();
    expect(blockInfo.payload.data.block.hash).toBeDefined();
    expect(blockInfo.payload.data.block.height).toBeDefined();

  });

  test('Verify that querying block by hash and height simultaneously returns an error', async () => {

    allure.description(`Queries block by hash and height within one HTTP request and verifies the response contains an error.`);
    allure.tms('PM-7124', 'PM-7124');
    allure.severity('blocker');
    allure.tag('query');

    const queryBody = 'query { block ( offset: { height: 0, hash: "0" } ) { hash } }';
    const response = await getResponseForQuery(`${serverEnv.getUrl()}/api/v1/graphql`, queryBody);
    expect(response.status).toBe(200);
    const responseBody = await response.body;
    expect(responseBody.data).toBe(null);
    expect(responseBody.errors[0].message).toEqual(
      'Invalid value for argument "offset", Oneof input objects requires have exactly one field',
    );
    
  });

  test('Verify that querying a block with height: -1 returns error', async () => {
    
    allure.description(`Queries block by height: -1 via HTTP request and verifies the response contains an error.`);
    allure.tms('PM-6135', 'PM-6135');
    allure.severity('blocker');
    allure.tag('query');

    const queryBody = 'query { block ( offset: { height: -1 } ) { hash } }';
    const response = await getResponseForQuery(`${serverEnv.getUrl()}/api/v1/graphql`, queryBody);
    expect(response.status).toBe(200);
    const responseBody = await response.body;
    expect(responseBody.data).toBe(null);
    expect(responseBody.errors[0].message).toEqual(
      'Failed to parse "Int": Invalid number (occurred while parsing "BlockOffsetInput")',
    );
  });

  test('Verify that querying a block with large height returns null for block', async () => {
    
    allure.description(`Queries block by a valid, but large height via HTTP request and verifies the response.`);
    allure.tms('PM-9592', 'PM-9592');
    allure.severity('blocker');
    allure.tag('query');

    // Highest accepted value
    const queryBody1 = 'query { block ( offset: { height: 4294967295 } ) { hash } }';
    const response1 = await getResponseForQuery(`${serverEnv.getUrl()}/api/v1/graphql`, queryBody1);
    expect(response1.status).toBe(200);
    const responseBody1 = await response1.body;
    expect(responseBody1).toEqual({ data: { block: null } });
    
    // Highest + 1
    const queryBody2 = 'query { block ( offset: { height: 4294967296 } ) { hash } }';
    const response2 = await getResponseForQuery(`${serverEnv.getUrl()}/api/v1/graphql`, queryBody2);
    expect(response2.status).toBe(200);
    const responseBody2 = await response2.body;
    expect(responseBody2.data).toBe(null);
    expect(responseBody2.errors[0].message).toEqual(
      'Failed to parse "Int": Only integers from 0 to 4294967295 are accepted. (occurred while parsing "BlockOffsetInput")',
    );

  });

});
