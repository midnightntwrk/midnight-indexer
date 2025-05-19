import { jest } from '@jest/globals';
import WebSocket from 'ws';
import { type TestEnv } from '../../environment/ITestEnv';
import { getTestServerEnv } from '../../environment/ServerEnvFactory';
import { getBlockInfo, getTxInfo } from '../../helpers/QueryHelpers';
import { initConnection } from '../../helpers/SubscriptionHelpers';
import { getResponseForQuery } from '../../utils/HttpRequestUtils';

describe(`Indexer Transaction Query Tests - ${process.env.ENV_TYPE} ${process.env.DEPLOYMENT}`, () => {
  jest.setTimeout(120000);

  let serverEnv: TestEnv;
  let txHash: string;
  let client: WebSocket;
  let blockOffsetInput: string;
  let txIdentifier: string;
  let txOffset: string;

  beforeAll(async () => {
    serverEnv = getTestServerEnv();
    await serverEnv.serverReady();
  });

  afterAll(async () => {
    await serverEnv.tearDownServer();
  });

  beforeEach((done) => {
    client = new WebSocket(`${serverEnv.getWsUrl()}/api/v1/graphql/ws`, ['graphql-ws']);
    client.on('open', () => {
      Promise.all([initConnection(client)]);
      done();
    });
  });

  afterEach((done) => {
    if (client?.readyState === WebSocket.OPEN) {
      client.terminate();
    }
    done();
  });

  test('Verify that querying transactions with a valid hash returns correct response', async () => {

    allure.description("A transaction query by hash with an existing hash returns the transaction");
    allure.tms('PM-6136', 'PM-6136');
    allure.label("environment", `${process.env.ENV_TYPE}`);
    allure.severity('blocker');
    allure.tag('query');
    allure.tag('smoke');

    // Request the genesis block
    blockOffsetInput = '(offset: {height: 0})';
    const blockInfo = await getBlockInfo(client, blockOffsetInput);

    // Extract the hash of the first transaction from that block
    txHash = blockInfo.payload.data.block.transactions[0].hash;
    const transactionHash = `(hash: "${txHash}")`;

    // Use that hash to perform a request of transaction by hash
    const txInfo = await getTxInfo(client, transactionHash);

    // Extracting the hash of the transaction and check it matches the requested on
    const data = txInfo.payload.data.transactions[0].hash;
    expect(data).toBe(txHash);

  });

  test('Verify that querying transactions by identifier returns correct response', async () => {

    allure.description("A transactions query with an existing tx identifier returns the transaction");
    allure.tms('PM-9584', 'PM-9584');
    allure.label("environment", `${process.env.ENV_TYPE}`);
    allure.severity('blocker');
    allure.tag('query');
    allure.tag('smoke');

    // Request the genesis block
    blockOffsetInput = '(offset: {height: 0})';
    const blockInfo = await getBlockInfo(client, blockOffsetInput);

    // Extract the hash of the first transaction from that block
    txHash = blockInfo.payload.data.block.transactions[0].hash;

    // Extract the first identifier of the first transaction from that block
    txIdentifier = blockInfo.payload.data.block.transactions[0].identifiers[0];
    txOffset = `(identifier: "${txIdentifier}")`;

    // Use that hash to perform a request of transaction by identifier
    const txInfo = await getTxInfo(client, txOffset);

    // Extracting the hash of the transaction and check it matches the requested on
    const data = txInfo.payload.data.transactions[0].hash;
    expect(data).toBe(txHash);

  });

  test('Verify that querying transactions by hash with malformed hash returns no transactions', async () => {

    allure.description("A transaction query with malformed hash returns an error");
    allure.tms('PM-6137', 'PM-6137');
    allure.label("environment", `${process.env.ENV_TYPE}`);
    allure.severity('blocker');
    allure.tag('query');
    allure.tag('negative');

    // Perform a transaction query by hash using a malformed hash
    const txOffset = `(hash: "non-existent-hash")`;
    const txInfo = await getTxInfo(client, txOffset);

    // Checking we get the expected error message
    expect(txInfo.payload.errors[0].message).toEqual('decode hash');

  });

  test('Verify that querying transactions by id with a malformed identifier returns no transactions', async () => {

    allure.description("A transaction query with malformed tx identifier returns an error");
    allure.tms('PM-14233', 'PM-14233');
    allure.label("environment", `${process.env.ENV_TYPE}`);
    allure.severity('blocker');
    allure.tag('query');
    allure.tag('negative');

    // Perform a transaction query by identifier using a malformed identifier
    const txOffset = `(identifier: "non-existent-identifier")`;
    const txInfo = await getTxInfo(client, txOffset);

    // Checking we get the expected error message
    expect(txInfo.payload.errors[0].message).toEqual('decode identifier');

  });

  test('Verify that querying transaction by hash and identifier simultaneously returns an error', async () => {

    allure.description("A transaction query with tx identifier and hash in the same request returns an error");
    allure.tms('PM-7126', 'PM-7126');
    allure.label("environment", `${process.env.ENV_TYPE}`);
    allure.severity('blocker');
    allure.tag('query');
    allure.tag('negative');

    // Perform a transaction query by identifier and hash
    const queryBody = 'query { transactions ( identifier: "Id-Tx1", hash: "Tx1" ) { hash } }';
    const response = await getResponseForQuery(`${serverEnv.getUrl()}/api/v1/graphql`, queryBody);

    // Checking we get the expected error message with no data in the payload
    expect(response.status).toBe(200);
    const responseBody = await response.body;
    expect(responseBody.data).toBeNull();
    expect(responseBody.errors[0].message).toMatch('either hash or identifier must be given and not both');

  });

});
