import { jest } from '@jest/globals';
import { DockerComposeEnvironment, Wait, type StartedDockerComposeEnvironment } from 'testcontainers';
import { verifyThatLogIsPresent, waitForLogAndVerifyIfLogPresent } from '../../helpers/LoggingHelpers';
import { sendQueryToPostgres } from '../../helpers/PostgresHelpers';
import { Commons } from '../../utils/Commons';

describe('Parent Block Hash Test', () => {
  jest.setTimeout(120000);

  let composeEnvironment: StartedDockerComposeEnvironment;

  beforeAll(async () => {
    composeEnvironment = await new DockerComposeEnvironment(
      './docker_composes/',
      'docker-compose-latest-cloud-indexer.yaml',
    )
      .withWaitStrategy('nats', Wait.forLogMessage('Listening for client connections on 0.0.0.0:4222'))
      .up();
  });

  afterAll(async () => {
    if (composeEnvironment) await composeEnvironment.down();
  });

  beforeEach(() => { });

  afterEach(async () => { });

  test('Verify that chain-indexer stops when block parent hash is incorrect', async () => {

    allure.tms('PM-10101', 'PM-10101');
    allure.description(`Restarts the chain-indexer service and changes the last hash.
      Finally it verifies that when chain-indexer returns, it fails during block parent verification.`,
    );
    allure.severity('normal');
    allure.tag('multitenant_architecture');

    await waitForLogAndVerifyIfLogPresent(
      composeEnvironment,
      'chain-indexer',
      /\"message\":\"block indexed\".*\"height\"\s*:\s*("?1"?)/,
      30,
      2000,
    );

    // generate a 32 byte hash
    await sendQueryToPostgres(
      composeEnvironment.getContainer('postgres').getFirstMappedPort(),
      `UPDATE blocks
      SET hash = decode(substring(repeat(md5(random()::text), 2), 1, 64), 'hex')
      WHERE id = (SELECT MAX(id) FROM blocks);`,
    );

    await composeEnvironment.getContainer('chain-indexer').restart();
    await Commons.sleep(5000);
    await verifyThatLogIsPresent(
      await composeEnvironment.getContainer('chain-indexer').logs(),
      /\"level\":\"WARN\",\"message\":\"unexpected block\"/,
      5000,
    );
  });
});
