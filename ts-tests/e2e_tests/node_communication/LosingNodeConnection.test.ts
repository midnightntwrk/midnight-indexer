import { jest } from '@jest/globals';
import { DockerComposeEnvironment, Wait, type StartedDockerComposeEnvironment } from 'testcontainers';
import { waitForLogAndVerifyIfLogPresent } from '../../helpers/LoggingHelpers';
import { sendQueryToPostgres } from '../../helpers/PostgresHelpers';
import { Commons } from '../../utils/Commons';

describe('Losing Node Connection Test', () => {
  jest.setTimeout(2400000);

  let composeEnvironment: StartedDockerComposeEnvironment;

  beforeAll(async () => { });

  afterAll(async () => { });

  beforeEach(() => { });

  afterEach(async () => {
    if (composeEnvironment) await composeEnvironment.down();
  });

  test('Verify that Indexer reconnects after losing connection to node', async () => {
    
    allure.tms('PM-9410', 'PM-9410');
    allure.description(
      `Starts up latest Indexer and stack locally, restarts the node service, finally it verifies that the Indexing continues.`,
    );
    allure.severity('normal');
    allure.tag('node_communication');

    // Start up the stack
    composeEnvironment = await new DockerComposeEnvironment(
      './docker_composes/',
      'docker-compose-latest-cloud-indexer.yaml',
    )
      .withWaitStrategy('nats', Wait.forLogMessage('Listening for client connections on 0.0.0.0:4222'))
      .withWaitStrategy('chain-indexer', Wait.forLogMessage(/\"message\":\"block indexed\".*\"height\"\s*:\s*("?0"?)/))
      .up();

    // Get the number of blocks in the db
    const queryResultBeforeRestart = await sendQueryToPostgres(
      composeEnvironment.getContainer('postgres').getFirstMappedPort(),
      'SELECT COUNT(*) FROM blocks',
    );
    const numberOfBlocksBeforeRestart = parseInt(queryResultBeforeRestart.rows[0].count as string);
    // Restart node
    await Commons.restartComponent(composeEnvironment, 'node', 6000);

    // Verify that Indexer lost connection
    await waitForLogAndVerifyIfLogPresent(
      composeEnvironment,
      'chain-indexer',
      /node disconnected, reconnecting/,
      10,
      5000,
    );
    // Wait for node to be healthy
    await waitForLogAndVerifyIfLogPresent(composeEnvironment, 'node', /Imported #3/, 10, 5000);
    // Check that indexing still working
    await waitForLogAndVerifyIfLogPresent(
      composeEnvironment,
      'chain-indexer',
      /\"message\":\"block indexed\".*\"height\"\s*:\s*("?1"?)/,
      10,
      5000,
    );

    // Get the number of blocks after restart
    const queryResultAfterRestart = await sendQueryToPostgres(
      composeEnvironment.getContainer('postgres').getFirstMappedPort(),
      'SELECT COUNT(*) FROM blocks',
    );
    const numberOfBlocksAfterRestart = parseInt(queryResultAfterRestart.rows[0].count as string);
    // Verify that more blocks have been stored
    expect(numberOfBlocksAfterRestart).toBeGreaterThan(numberOfBlocksBeforeRestart);

  });

});
