import { jest } from '@jest/globals';
import WebSocket from 'ws';
import { type TestEnv } from '../../environment/ITestEnv';
import { getTestServerEnv } from '../../environment/ServerEnvFactory';
import { initConnection } from '../../helpers/SubscriptionHelpers';

describe(`Indexer Websocket Connection Tests - ${process.env.ENV_TYPE} ${process.env.DEPLOYMENT}`, () => {
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

  test('Verify that WS connections are closed at disconnect', async () => {

    allure.description("Opens and closes the WS connection and waits for websocket connection to be closed.");
    allure.tms('PM-9188', 'PM-9188');
    allure.severity('blocker');
    allure.tag('connection');

    await initConnection(client);
    client.close();
    expect(client.readyState).toBe(WebSocket.CLOSING);
    await new Promise((resolve) => {
      const interval = setInterval(() => {
        if (client.readyState === WebSocket.CLOSED) {
          clearInterval(interval);
          resolve(client);
        }
      }, 100);
    });
    expect(client.readyState).toBe(WebSocket.CLOSED);
  });
});
