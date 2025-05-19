import { jest } from '@jest/globals';
import { type TestEnv } from '../../environment/ITestEnv';
import { getTestServerEnv } from '../../environment/ServerEnvFactory';
import { getResponse, getResponseForQuery } from '../../utils/HttpRequestUtils';

describe(`Indexer Metadata endpoint query tests  - ${process.env.ENV_TYPE} ${process.env.DEPLOYMENT}`, () => {
  jest.setTimeout(30000);

  let serverEnv: TestEnv;

  beforeAll(async () => {
    serverEnv = getTestServerEnv();
    await serverEnv.serverReady();
  });

  afterAll(async () => {
    await serverEnv.tearDownServer();
  });

  test('Verify that health endpoint returns 200 OK', async () => {

    allure.description("A request to the health metadata endpoint returns 200 OK");
    allure.tms('PM-7600', 'PM-7600');
    allure.severity('blocker');
    allure.tag('metadataApi');


    const response = await getResponse(`${serverEnv.getUrl()}/health`);
    expect(response.status).toBe(200);
    expect((await response.body) === 'OK, deprecated: use ../ready instead').toBe(true);
  });

  test('Verify that old API versions return 404', async () => {

    allure.description("A request to the version metadata endpoint returns an error");
    allure.tms('PM-7170', 'PM-7170');
    allure.severity('blocker');
    allure.tag('metadataApi');
    allure.tag('negative');

    const queryBody =
      'query { block ( offset: { height: 1 }) { transactions { block { timestamp hash } } timestamp hash } }';
    const responseV1 = await getResponseForQuery(`${serverEnv.getUrl()}/api/v0/graphql`, queryBody);
    expect(responseV1.status).toBe(404);
  });

  test('Verify that graphql-api exposes ready endpoint', async () => {

    allure.description(`A request to /ready endpoint returns the status code 200`);
    allure.tms('PM-10142', 'PM-10142');
    allure.severity('blocker');
    allure.tag('metadataApi');

    const response = await getResponse(`${serverEnv.getUrl()}/ready`);
    expect(response.status).toBe(200);
  });
});
