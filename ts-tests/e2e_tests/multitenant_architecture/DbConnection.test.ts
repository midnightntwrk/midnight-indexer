import { jest } from '@jest/globals';
import { DockerComposeEnvironment, Wait, type StartedDockerComposeEnvironment } from 'testcontainers';
import { sendQueryToPostgres } from '../../helpers/PostgresHelpers';
import { verifyWalletSubscription } from '../../helpers/SubscriptionHelpers';
import { Commons } from '../../utils/Commons';

describe('Losing DB Connection Test', () => {
  jest.setTimeout(2400000);

  let composeEnvironment: StartedDockerComposeEnvironment;

  beforeAll(async () => { });

  afterAll(async () => { });

  beforeEach(() => { });

  afterEach(async () => {
    if (composeEnvironment) await composeEnvironment.down();
  });

  test('Verify that the indexer returns indexing after database connection is restored', async () => {

    allure.description(`Stops the postgres service, then restarts it, finally it verifies that the Indexing continues.`);
    allure.tms('PM-9198', 'PM-9198');
    allure.severity('normal');
    allure.tag('multitenant_architecture');

    // Start up the stack
    composeEnvironment = await new DockerComposeEnvironment(
      './docker_composes/',
      'docker-compose-latest-cloud-indexer.yaml',
    )
      .withWaitStrategy('nats', Wait.forLogMessage('Listening for client connections on 0.0.0.0:4222'))
      .withWaitStrategy('chain-indexer', Wait.forLogMessage(/\"message\":\"block indexed\".*\"height\"\s*:\s*("?0"?)/))
      .up();
    // Get the number of blocks in the db
    const queryResultBeforeRestart1 = await sendQueryToPostgres(
      composeEnvironment.getContainer('postgres').getFirstMappedPort(),
      'SELECT COUNT(*) FROM blocks',
    );
    const numberOfBlocksBeforeRestart = parseInt(queryResultBeforeRestart1.rows[0].count as string);
    // Restart postgres
    await Commons.restartComponent(composeEnvironment, 'postgres', 10000);
    // Wait for postgres to be healthy
    await Commons.sleep(5000);
    // Get the number of blocks after restart
    const queryResultAfterRestart1 = await sendQueryToPostgres(
      composeEnvironment.getContainer('postgres').getFirstMappedPort(),
      'SELECT COUNT(*) FROM blocks',
    );
    
    const numberOfBlocksAfterRestart = parseInt(queryResultAfterRestart1.rows[0].count as string);
    // Verify that more blocks have been stored
    expect(numberOfBlocksAfterRestart).toBeGreaterThan(numberOfBlocksBeforeRestart);
    // Verify that wallet can connect
    await verifyWalletSubscription(composeEnvironment.getContainer('indexer-api').getFirstMappedPort());

  });
});
