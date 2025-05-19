import { jest } from '@jest/globals';
import { DockerComposeEnvironment, Wait, type StartedDockerComposeEnvironment } from 'testcontainers';
import { waitForLogAndVerifyIfLogPresent } from '../../helpers/LoggingHelpers';
import { Commons } from '../../utils/Commons';
import { getResponse } from '../../utils/HttpRequestUtils';

describe('Metadata Api Test', () => {
  jest.setTimeout(120000);

  let composeEnvironment: StartedDockerComposeEnvironment;

  beforeAll(async () => { });

  afterAll(async () => { });

  beforeEach(async () => { });

  afterEach(async () => {
    if (composeEnvironment) await composeEnvironment.down();
  });

  test('Verify that when api server is up /ready endpoint returns 200', async () => {

    allure.description(`Starts up latest Indexer locally, sends a request to /ready and verifies that it returns 200.`);
    allure.tms('PM-10142', 'PM-10142');
    allure.severity('normal');
    allure.tag('metadata_api');

    composeEnvironment = await new DockerComposeEnvironment(
      './docker_composes/',
      'docker-compose-latest-cloud-indexer.yaml',
    )
      .withWaitStrategy('indexer-api', Wait.forLogMessage('listening to TCP connections'))
      .withWaitStrategy('nats', Wait.forLogMessage('Listening for client connections on 0.0.0.0:4222'))
      .up();

    const apiPort = composeEnvironment.getContainer('indexer-api').getFirstMappedPort();
    const apiResponse = await getResponse(`http://localhost:${apiPort}/ready`);
    expect(apiResponse.status).toBe(200);

  });

  test('Verify that when chain indexer is not caught up /ready endpoint returns 503', async () => {

    allure.description(`Given node has lots of blocks to index. Sends a request to /ready and /health and verifies that it returns 503.`);
    allure.tms('PM-10155', 'PM-10155');
    allure.severity('normal');
    allure.tag('metadata_api');

    composeEnvironment = await new DockerComposeEnvironment(
      './docker_composes/',
      'docker-compose-latest-benchmark-rust.yml',
    )
      .withWaitStrategy('nats', Wait.forLogMessage('Listening for client connections on 0.0.0.0:4222'))
      .withWaitStrategy('indexer-api', Wait.forLogMessage('listening to TCP connections'))
      .up();

    // /ready
    const indexerApiPort = composeEnvironment.getContainer('indexer-api').getFirstMappedPort();
    const readyResponseNotCaughtUp = await getResponse(`http://localhost:${indexerApiPort}/ready`);
    expect(readyResponseNotCaughtUp.status).toBe(503);
    expect(await readyResponseNotCaughtUp.body).toBe('indexer has not yet caught up with the node');

    // /health
    const healthResponseNotCaughtUp = await getResponse(`http://localhost:${indexerApiPort}/health`);
    expect(healthResponseNotCaughtUp.status).toBe(503);
    expect(await healthResponseNotCaughtUp.body).toBe(
      'indexer has not yet caught up with the node; deprecated: use ../ready instead',
    );
    await waitForLogAndVerifyIfLogPresent(
      composeEnvironment,
      'chain-indexer',
      /\"message\":\"caught-up status changed\",\"caught_up\":true/,
      30,
      5000,
    );

    await Commons.sleep(5000);

    // /ready
    const readyResponseCaughtUp = await getResponse(`http://localhost:${indexerApiPort}/ready`);
    expect(readyResponseCaughtUp.status).toBe(200);

    // /health
    const healthResponseCaughtUp = await getResponse(`http://localhost:${indexerApiPort}/health`);
    expect(healthResponseCaughtUp.status).toBe(200);
    expect(await healthResponseCaughtUp.body).toBe('OK, deprecated: use ../ready instead');

  });

});
