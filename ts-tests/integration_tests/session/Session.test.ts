import { jest } from '@jest/globals';
import WebSocket from 'ws';
import { environments, type Environment } from '../../environment/envConfig';
import { IntegratedEnvServer } from '../../environment/IntegratedEnvServer';
import { type TestEnv } from '../../environment/ITestEnv';
import { getTestServerEnv } from '../../environment/ServerEnvFactory';
import { TestContainersComposeEnvironment } from '../../environment/TestContainersComposeEnvironment';
import { disconnectSession, getSessionId } from '../../helpers/SessionHelpers';
import { initConnection } from '../../helpers/SubscriptionHelpers';

describe(`Indexer Session Tests - ${process.env.ENV_TYPE} ${process.env.DEPLOYMENT}`, () => {
  jest.setTimeout(120000);

  let client: WebSocket;
  let serverEnv: TestEnv;
  let envConfig: Environment;

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

  test('Verify that session ID can be requested', async () => {

    allure.description(`Sends the connect mutation via WS, and finally verifies the response.`);
    allure.tms('PM-6415', 'PM-6415');
    allure.severity('blocker');
    allure.tag('session');

    await initConnection(client);
    const response = await getSessionId(client, envConfig.wallets[0].viewingKey);
    expect(response.payload.data.connect).toEqual(expect.any(String));

  });

  test('Verify that the app handles non-initialised connection', async () => {

    allure.description(`Without WS connection init, sends the connect request via WS, and verifies the error response.`);
    allure.tms('PM-9585', 'PM-9585');
    allure.severity('blocker');
    allure.tag('session');
    let errorReceived = null;
    try {
      await getSessionId(client, 'invalid key');
    } catch (error) {
      errorReceived = error.message;
    }
    expect(errorReceived).toBe('Connection closed with code: 1011, reason: The handshake is not completed.');

  });

  test('Verify that session can be disconnected', async () => {

    allure.description(`Sends the connect request via WS, sends the disconnect request and verifies the response.`);
    allure.tms('PM-9588', 'PM-9588');
    allure.severity('blocker');
    allure.tag('session');

    await initConnection(client);
    const sessionIdResponse = await getSessionId(client, envConfig.wallets[0].viewingKey);
    const responseDisconnect = await disconnectSession(client, sessionIdResponse.payload.data.connect as string);
    expect(responseDisconnect.payload.data.disconnect).toBeNull();

  });

  test('viewingKey from different network accepted - Verify that Indexer rejects connecting with a viewing key from another network id', async () => {

    allure.tms('PM-9296', 'PM-9296');
    allure.description(`Sends the connect request using incorrect viewingKey from different network and verifies the error response.`);
    allure.severity('blocker');
    allure.tag('session');

    await initConnection(client);
    let viewingKey: string = '';
    if (getTestServerEnv() instanceof TestContainersComposeEnvironment)
      viewingKey = environments.testnet.wallets[0].viewingKey;
    else if (getTestServerEnv() instanceof IntegratedEnvServer) viewingKey = environments.compose.wallets[0].viewingKey;
    else throw new Error('Unexpected environment type');
    const response = await getSessionId(client, viewingKey);
    expect(response.payload.errors[0].message).toBe('invalid viewing key');

  });
});
