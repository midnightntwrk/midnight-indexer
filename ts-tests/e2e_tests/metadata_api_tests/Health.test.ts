import { jest } from '@jest/globals';
import { DockerComposeEnvironment, Wait, type StartedDockerComposeEnvironment } from 'testcontainers';
import { getResponse } from '../../utils/HttpRequestUtils';

describe('Health Endpoint For Multi-tenant architecture', () => {
  jest.setTimeout(120000);

  let composeEnvironment: StartedDockerComposeEnvironment;

  beforeAll(async () => { });

  afterAll(async () => { });

  beforeEach(() => { });

  afterEach(async () => {
    if (composeEnvironment) await composeEnvironment.down();
  });

  test('Verify that indexer reports health correctly in multi-tenant architecture', async () => {

    allure.description(`Verifies that 'graphql-api' service reports health properly via /health endpoint.`);
    allure.tms('PM-8855', 'PM-8855');
    allure.severity('normal');
    allure.tag('metadata_api');

    composeEnvironment = await new DockerComposeEnvironment(
      './docker_composes/',
      'docker-compose-latest-cloud-indexer.yaml',
    )
      .withWaitStrategy(`proof-server`, Wait.forLogMessage('Actix runtime found; starting in Actix runtime'))
      .withWaitStrategy(`node`, Wait.forListeningPorts())
      .withWaitStrategy('chain-indexer', Wait.forLogMessage(/block indexed.*\"height\"\s*:\s*("?0"?)/))
      .withWaitStrategy('nats', Wait.forLogMessage('Listening for client connections on 0.0.0.0:4222'))
      .withWaitStrategy('indexer-api', Wait.forLogMessage('listening to TCP connections'))
      .up();

    const portGraphqlApi = composeEnvironment.getContainer('indexer-api').getFirstMappedPort();
    const responseGraphqlApi = await getResponse(`http://localhost:${portGraphqlApi}/health`);
    expect(responseGraphqlApi.status).toBe(200);
    expect(await responseGraphqlApi.body).toBe('OK, deprecated: use ../ready instead');
  });
});
