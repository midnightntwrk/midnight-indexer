import { jest } from '@jest/globals';
import { type TestEnv } from '../../environment/ITestEnv';
import { getTestServerEnv } from '../../environment/ServerEnvFactory';
import { type Environment } from '../../environment/envConfig';
import { getResponseForQuery } from '../../utils/HttpRequestUtils';

describe(`Indexer Connect Query Tests - ${process.env.ENV_TYPE} ${process.env.DEPLOYMENT}`, () => {
  jest.setTimeout(120000);

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

  beforeEach(() => { });

  afterEach((done) => {
    done();
  });

  test('Verify that connection with correct viewingKey in hex format is possible via http', async () => {

    allure.description(`Sends a connect request with a correct viewingKey in hex format and verifies the response.`);
    allure.tms('PM-9579', 'PM-9579');
    allure.severity('blocker');
    allure.tag('query');

    const viewingKey: string = envConfig.wallets[0].viewingKey;
    const mutation = `mutation { connect ( viewingKey: "${viewingKey}") }`;
    const response = await getResponseForQuery(`${serverEnv.getUrl()}/api/v1/graphql`, mutation);
    expect(response.status).toBe(200);
    const responseBody = await response.body;
    expect(responseBody.data.connect).toMatch(/^[a-f0-9]{64}$/);

  });

  test('Verify that connection with correct viewingKey in bech32m format is possible via http', async () => {

    allure.description(`Sends a connect request with a correct viewingKey in bech32m format and verifies the response.`);
    allure.tms('PM-13928', 'PM-13928');
    allure.severity('blocker');
    allure.tag('query');

    const viewingKey: string = envConfig.wallets[0].viewingKeyBech32m;
    const mutation = `mutation { connect ( viewingKey: "${viewingKey}") }`;
    const response = await getResponseForQuery(`${serverEnv.getUrl()}/api/v1/graphql`, mutation);
    expect(response.status).toBe(200);
    const responseBody = await response.body;
    expect(responseBody.data.connect).toMatch(/^[a-f0-9]{64}$/);

  });

  test('Verify that improperly formatted viewing keys are validated via http', async () => {

    allure.description(`Sends a connect request with a incorrect viewingKeys expecting an error.`);
    allure.tms('PM-8831', 'PM-8831');
    allure.severity('blocker');
    allure.tag('query');

    const viewingKeys: string[] = [
      '1',
      `${envConfig.wallets[0].viewingKey}1`,
      'non-hex-values',
      'mn_shield-esk_dev1qvqr3x9c7l',
      'bc_shield-esk_dev1qvqr3x9c7l4x0ndj6cwscu6cp05hn5x37te06r56g5v3mdyh7qug35rvs9nq298y324d8s38n8zhnah7qd6aaenkps63j8shdyczt',
      'mn_shield-esk_mychain1qvqr3x9c7l4x0ndj6cwscu6cp05hn5x37te06r56g5v3mdyh7qug35rvs9nq298y324d8s38n8zhnah7qd6aaenkps63j8shdyczt',
    ];

    for (const viewingKey of viewingKeys) {
      const mutation = `mutation { connect ( viewingKey: "${viewingKey}") }`;
      const response = await getResponseForQuery(`${serverEnv.getUrl()}/api/v1/graphql`, mutation);
      expect(response.status).toBe(200);
      const responseBody = await response.body;
      expect(responseBody.data).toBe(null);
      expect(responseBody.errors[0].message).toBe('invalid viewing key format: failed both Bech32m and hex decoding');
    }

  });

  test('Verify that properly formatted invalid viewing keys are validated via http', async () => {

    allure.description(`Sends a connect request with a incorrect viewingKeys and verifies the responses.`);
    allure.tms('PM-13929', 'PM-13929');
    allure.severity('blocker');
    allure.tag('query');

    const viewingKeys: string[] = [
      'mn_shield-cpk_dev1a3jj30d5pc8xznv8dlum0q4dfc3ehz9j3pvdltmrl5ghvqyajymqceg665',
      'mn_shield-addr_dev1a3jj30d5pc8xznv8dlum0q4dfc3ehz9j3pvdltmrl5ghvqyajymqxq84sp9hpq6cjqv80ejysdxn7j9s29angy2v7ltdeq8up4sfpf67verx3n238mmzqaushchntpdmk54jt0pwvraa3har2amacp',
    ];
    for (const viewingKey of viewingKeys) {
      const mutation = `mutation { connect ( viewingKey: "${viewingKey}") }`;
      const response = await getResponseForQuery(`${serverEnv.getUrl()}/api/v1/graphql`, mutation);
      expect(response.status).toBe(200);
      const responseBody = await response.body;
      expect(responseBody.data).toBe(null);
      expect(responseBody.errors[0].message).toBe('invalid viewing key');
    }

  });

});
