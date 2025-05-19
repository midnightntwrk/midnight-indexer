import { jest } from '@jest/globals';
import WebSocket from 'ws';
import { type TestEnv } from '../../environment/ITestEnv';
import { getTestServerEnv } from '../../environment/ServerEnvFactory';
import { getBlockInfo } from '../../helpers/QueryHelpers';
import { blocksSubscription, initConnection } from '../../helpers/SubscriptionHelpers';

describe(`Indexer Blocks Subscription Tests  - ${process.env.ENV_TYPE} ${process.env.DEPLOYMENT}`, () => {
  jest.setTimeout(120000);

  let client: WebSocket;
  let serverEnv: TestEnv;

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

  test('Verify that blocks subscription by block height returns correct data', async () => {

    allure.description(`Subscribes to blocks by latest and verifies the response.`);
    allure.tms('PM-7272', 'PM-7272');
    allure.severity('blocker');
    allure.tag('subscription');
    await initConnection(client);
    const blockOffsetInput = '(offset: {height: 0})';
    const response = await blocksSubscription(client, blockOffsetInput);
    const blockHash = response.payload.data.blocks.hash;
    const blockHeight = response.payload.data.blocks.height;
    expect(blockHash).toMatch(/^[a-f0-9]{64}$/);
    expect(blockHeight).toEqual(0);

  });

  test('Verify that blocks subscription by block hash returns correct data', async () => {

    allure.description(`Subscribes to blocks by a valid hash and verifies the response.`);
    allure.tms('PM-7273', 'PM-7273');
    allure.severity('blocker');
    allure.tag('subscription');

    await initConnection(client);
    const blockOffsetHeight = '(offset: {height: 0})';
    const blockInfo = await getBlockInfo(client, blockOffsetHeight);
    const blockOffsetInput: string = blockInfo.payload.data.block.hash;
    const txHash = `(offset: {hash: \"${blockOffsetInput}"\})`;
    const response = await blocksSubscription(client, txHash);
    const responseMessage = response.payload.data.blocks.hash;
    expect(responseMessage).toBeDefined();
    expect(responseMessage).toMatch(/^[a-f0-9]{64}$/);

  });

  test('Verify that blocks subscription by latest returns correct data', async () => {

    allure.description(`Subscribes to blocks by latest and verifies the response.`);
    allure.tms('PM-7274', 'PM-7274');
    allure.severity('blocker');
    allure.tag('subscription');

    await initConnection(client);
    const response = await blocksSubscription(client);
    const responseHash = response.payload.data.blocks.hash;
    expect(responseHash).toBeDefined();

  });

  test('Verify that subscribing to blocks with both hash and height specified returns an error', async () => {

    allure.description(`Subscribes to blocks by both block has and height and verifies the error response.`);
    allure.tms('PM-7275', 'PM-7275');
    allure.severity('blocker');
    allure.tag('subscription');

    await initConnection(client);
    const blockOffsetInput = '(offset: {hash:"0", height: 0})';
    const response = await blocksSubscription(client, blockOffsetInput);
    const errorMessage = response.payload.errors[0].message;
    expect(errorMessage).toEqual(
      'Invalid value for argument "offset", Oneof input objects requires have exactly one field',
    );

  });

  test('Verify that subscribing to blocks with nonexistent hash returns an error', async () => {

    allure.description(`Subscribes to blocks by a non-existent hash and verifies the error response.`);
    allure.tms('PM-7276', 'PM-7276');
    allure.severity('blocker');
    allure.tag('subscription');

    await initConnection(client);
    const nonExistentHash = 'abc';
    const blockOffsetInput = `(offset: {hash: \"${nonExistentHash}\"})`;
    const response = await blocksSubscription(client, blockOffsetInput);
    const errorMessage = response.payload.errors[0].message;
    expect(errorMessage).toEqual('decode hash');

  });

  test('Verify that subscribing to blocks with nonexistent height returns an error', async () => {

    allure.description(`Subscribes to blocks by a non-existent height and verifies the error response.`);
    allure.tms('PM-7277', 'PM-7277');
    allure.severity('blocker');
    allure.tag('subscription');

    await initConnection(client);
    const nonExistentHeight = 4294967295; // Some really high value
    const blockOffsetInput = `(offset: {height: ${nonExistentHeight}})`;
    const response = await blocksSubscription(client, blockOffsetInput);
    const errorMessage = response.payload.errors[0].message;
    expect(errorMessage).toEqual(`block with height ${nonExistentHeight} not found`);

  });

});
