import { jest } from '@jest/globals';
import WebSocket from 'ws';
import { type TestEnv } from '../../environment/ITestEnv';
import { getTestServerEnv } from '../../environment/ServerEnvFactory';
import { type Environment } from '../../environment/envConfig';
import { disconnectSession, getSessionId } from '../../helpers/SessionHelpers';
import { initConnection, walletSubscription } from '../../helpers/SubscriptionHelpers';

describe(`Indexer Wallet Subscription Tests - ${process.env.ENV_TYPE} ${process.env.DEPLOYMENT}`, () => {
  jest.setTimeout(120000);

  let client: WebSocket;
  let serverEnv: TestEnv;
  let envConfig: Environment;
  let sessionId: string;

  beforeAll(async () => {
    serverEnv = getTestServerEnv();
    await serverEnv.serverReady();
    envConfig = serverEnv.getEnvConfig();
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

  test('Verify that ProgressUpdate is sent early at subscription', async () => {

    allure.description(`Subscribes to wallet, then verifies that ProgressUpdate is being sent within 2 seconds.`);
    allure.tms('PM-9196', 'PM-9196');
    allure.severity('blocker');
    allure.tag('subscription');

    await initConnection(client);

    const walletDetails = envConfig.wallets[0];
    const sessionResponse = await getSessionId(client, walletDetails.viewingKey);
    const sessionId = sessionResponse.payload.data.connect as string;
    const subscriptionResponse = await walletSubscription(client, true, sessionId);

    if (subscriptionResponse.payload.data.wallet.__typename === 'ProgressUpdate') {
      return;
    }

    const receivedProgressUpdatePromise = new Promise((resolve) => {
      client.on('message', (data: WebSocket.Data) => {
        const message = typeof data === 'string' ? data : data.toString(); // eslint-disable-line @typescript-eslint/no-base-to-string
        const parsedMessage = JSON.parse(message);
        if (parsedMessage.payload.data.wallet.__typename === 'ProgressUpdate') {
          resolve(true);
        }
      });

    });

    // Await the receipt of ProgressUpdate within 2 seconds
    const receivedProgressUpdate = await Promise.race([
      receivedProgressUpdatePromise,
      new Promise((resolve) =>
        setTimeout(() => {
          resolve(false);
        }, 2000),
      ),
    ]);

    expect(receivedProgressUpdate).toBe(true);

  });

  test('Verify that wallet subscription works as expected (sessionID in message body)', async () => {

    allure.tms('PM-9349', 'PM-9349');
    allure.description(
      `Running on environment '${process.env.ENV_TYPE as string}'.
      Initialises WS connection, subscribes to wallet with sessionID in message body, then verifies the response.`,
    );
    allure.severity('blocker');
    allure.tag('subscription');

    await initConnection(client);
    const walletDetails = envConfig.wallets[0];
    const sessionResponse = await getSessionId(client, walletDetails.viewingKey);
    sessionId = sessionResponse.payload.data.connect as string;
    const subscriptionResponse = await walletSubscription(client, false, sessionId);
    const responseMessage = subscriptionResponse.payload.data.wallet.update[0];
    const index = subscriptionResponse.payload.data.wallet.index;
    expect(responseMessage).toBeDefined();
    expect(index?.toString()).toMatch(/^[0-9]+$/);

  });

  test('Verify that wallet subscription works as expected (sessionID in message body) for a bech32m viewingKey', async () => {

    allure.description(`Subscribes to wallet with sessionID in message body, then verifies the response.`);
    allure.tms('PM-13932', 'PM-13932');
    allure.severity('blocker');
    allure.tag('subscription');

    await initConnection(client);

    const walletDetails = envConfig.wallets[0];
    const sessionResponse = await getSessionId(client, walletDetails.viewingKeyBech32m);
    sessionId = sessionResponse.payload.data.connect as string;

    const subscriptionResponse = await walletSubscription(client, false, sessionId);
    const responseMessage = subscriptionResponse.payload.data.wallet.update[0];
    const index = subscriptionResponse.payload.data.wallet.index;
    expect(responseMessage).toBeDefined();
    expect(index?.toString()).toMatch(/^[0-9]+$/);

  });

  test('Verify that wallet subscription does not work with invalid sessionID', async () => {

    allure.description(`Subscribes to wallet with invalid sessionID, then verifies the error response.`);
    allure.tms('PM-9605', 'PM-9605');
    allure.severity('blocker');
    allure.tag('subscription');

    const testData = [
      { sessionId: 'invalidSessionId', expResponse: "invalid character 'i' at position 0" },
      { sessionId: '-1', expResponse: "invalid character '-' at position 0" },
      { sessionId: 'a', expResponse: 'odd number of digits' },
      // { sessionId: 'ab', expResponse: 'no_data' },
      // { sessionId: createHash('sha256').digest('hex'), expResponse: 'no_data' },
    ];
    await initConnection(client);
    for (const data of testData) {
      const subscriptionResponse = await walletSubscription(client, false, data.sessionId);
      const errorMessage = subscriptionResponse.payload.errors[0].message;
      expect(errorMessage).toEqual(data.expResponse);
    }

  });

  test('Verify that subscription to wallet stops after disconnect', async () => {

    allure.description(`Subscribes to wallet with valid sessionID, and verifies that no messages were received after session disconnect.`);
    allure.tms('PM-6416', 'PM-6416');
    allure.severity('blocker');
    allure.tag('subscription');

    await initConnection(client);
    const response = await getSessionId(client, envConfig.wallets[0].viewingKey);
    const sessionId = response.payload.data.connect as string;
    await walletSubscription(client, false, sessionId);
    let receivedNewMessages = false;
    client.on('message', (data: WebSocket.Data) => {
      // eslint-disable-next-line @typescript-eslint/no-base-to-string
      const message = typeof data === 'string' ? data : data.toString();
      const parsedMessage = JSON.parse(message);

      if (parsedMessage.type !== 'complete' && parsedMessage.type !== 'data') {
        receivedNewMessages = true;
      }
    });
    await disconnectSession(client, sessionId);
    await new Promise((resolve) => setTimeout(resolve, 4000));
    expect(receivedNewMessages).toBe(false);

  });

  test('Verify that reconnection is successful after disconnect', async () => {

    allure.description(`Subscribes to wallet with valid sessionID, and verifies that reconnection is possible after session disconnect.`);
    allure.tms('PM-6418', 'PM-6418');
    allure.severity('blocker');
    allure.tag('subscription');

    const walletDetails = envConfig.wallets[0];
    await initConnection(client);

    const sessionResponse = await getSessionId(client, walletDetails.viewingKey);
    const sessionId = sessionResponse.payload.data.connect as string;
    await walletSubscription(client, false, sessionId);
    await disconnectSession(client, sessionId);

    const subscriptionResponse = await walletSubscription(client, false, sessionId);
    const responseMessage = subscriptionResponse.payload.data.wallet.update[0];
    const index = subscriptionResponse.payload.data.wallet.index;
    expect(responseMessage).toBeDefined();
    expect(index?.toString()).toMatch(/^[0-9]+$/);

  });

  test('Verify that improperly formatted viewing keys are validated via WS', async () => {

    allure.description(`Trying to get sessionID with improperly formatted viewingKeys, and verifies error response.`);
    allure.tms('PM-8831', 'PM-8831');
    allure.severity('blocker');
    allure.tag('subscription');

    await initConnection(client);
    const viewingKeys: string[] = [
      '1',
      `${envConfig.wallets[0].viewingKey}1`,
      'non-hex-values',
      'mn_shield-esk_dev1qvqr3x9c7l',
      'bc_shield-esk_dev1qvqr3x9c7l4x0ndj6cwscu6cp05hn5x37te06r56g5v3mdyh7qug35rvs9nq298y324d8s38n8zhnah7qd6aaenkps63j8shdyczt',
      'mn_shield-esk_mychain1qvqr3x9c7l4x0ndj6cwscu6cp05hn5x37te06r56g5v3mdyh7qug35rvs9nq298y324d8s38n8zhnah7qd6aaenkps63j8shdyczt',
    ];

    for (const viewingKey of viewingKeys) {
      const sessionResponse = await getSessionId(client, viewingKey);
      expect(sessionResponse.payload.errors[0].message).toBe(
        'invalid viewing key format: failed both Bech32m and hex decoding',
      );
    }

  });

  test('Verify that properly formatted but invalid viewing keys are validated via WS', async () => {

    allure.description(`Initialises WS connection, trying to get sessionID with invalid viewingKeys, and verifies error response.`);
    allure.tms('PM-13929', 'PM-13929');
    allure.severity('blocker');
    allure.tag('subscription');

    await initConnection(client);
    const viewingKeys: string[] = [
      'mn_shield-cpk_dev1a3jj30d5pc8xznv8dlum0q4dfc3ehz9j3pvdltmrl5ghvqyajymqceg665',
      'mn_shield-addr_dev1a3jj30d5pc8xznv8dlum0q4dfc3ehz9j3pvdltmrl5ghvqyajymqxq84sp9hpq6cjqv80ejysdxn7j9s29angy2v7ltdeq8up4sfpf67verx3n238mmzqaushchntpdmk54jt0pwvraa3har2amacp',
    ];

    for (const viewingKey of viewingKeys) {
      const sessionResponse = await getSessionId(client, viewingKey);
      expect(sessionResponse.payload.errors[0].message).toBe('invalid viewing key');
    }

  });

  test('Verify that Merkle tree root is returned at wallet subscription', async () => {

    allure.description(`Subscribes to wallet and verifies that merkleTreeRoot is returned.`);
    allure.tms('PM-9347', 'PM-9347');
    allure.severity('blocker');
    allure.tag('subscription');

    await initConnection(client);
    const walletDetails = envConfig.wallets[0];
    const sessionResponse = await getSessionId(client, walletDetails.viewingKey);
    const sessionId = sessionResponse.payload.data.connect as string;
    const subscriptionResponse = await walletSubscription(client, false, sessionId);
    const walletUpdate = subscriptionResponse.payload.data.wallet.update;
    for (const item of walletUpdate) {
      if (item.__typename === 'RelevantTransaction') {
        expect(item.transaction.merkleTreeRoot).toMatch(/^[a-f0-9]+$/);
      }
    }
  });

  test('Verify that protocol version is returned for RelevantTransaction and MerkleTreeCollapsedUpdate', async () => {

    allure.description(`Subscribes to wallet and verifies that protocolVersion is returned for RelevantTransaction and MerkleTreeCollapsedUpdate.`);
    allure.tms('PM-12043', 'PM-12043');
    allure.severity('blocker');
    allure.tag('subscription');

    await initConnection(client);

    const walletDetails = envConfig.wallets[0];
    const sessionResponse = await getSessionId(client, walletDetails.viewingKey);
    const sessionId = sessionResponse.payload.data.connect as string;
    const subscriptionResponse = await walletSubscription(client, false, sessionId);
    const walletUpdate = subscriptionResponse.payload.data.wallet.update;

    for (const item of walletUpdate) {
      if (item.__typename === 'MerkleTreeCollapsedUpdate') {
        expect(item.protocolVersion.toString()).toMatch(/^[0-9]+$/);
      }
      if (item.__typename === 'RelevantTransaction') {
        expect(item.transaction.protocolVersion.toString()).toMatch(/^[0-9]+$/);
      }
    }

  });

});
