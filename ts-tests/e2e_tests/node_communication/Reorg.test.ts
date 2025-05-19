import { jest } from '@jest/globals';
import { DockerComposeEnvironment, Wait, type StartedDockerComposeEnvironment } from 'testcontainers';
import { waitForLogAndVerifyIfLogPresent } from '../../helpers/LoggingHelpers';
import { Commons } from '../../utils/Commons';

describe('Reorg Test', () => {
  jest.setTimeout(3600000);

  let composeEnvironment: StartedDockerComposeEnvironment;

  beforeAll(async () => { });

  afterAll(async () => { });

  beforeEach(async () => {
    composeEnvironment = await new DockerComposeEnvironment('./docker_composes/', 'docker-compose-reorg-test.yml')
      .withWaitStrategy('nats', Wait.forLogMessage('Listening for client connections on 0.0.0.0:4222'))
      .up();
  });

  afterEach(async () => {
    if (composeEnvironment) await composeEnvironment.down();
  });

  test.each(new Array(10).fill(null).map((_, i) => [i + 1]))(

    'Verify that when blocks are reorganized the chain indexer only indexes finalized blocks - Iteration %s',
    async () => {
      allure.tms('PM-13651', 'PM-13651');
      allure.description(`Starts up latest node with 2 networks and behind a load balancer.
        Starts up chain indexer locally connected to the load balancer.
        Restarts the boot-node and verifies that when reorg happens chain indexer receives the correct block.`);
      allure.severity('normal');
      allure.tag('node_communication');
      await Commons.sleep(10000);
      await Commons.restartComponent(composeEnvironment, 'boot-node', 30000);
      await waitForLogAndVerifyIfLogPresent(
        composeEnvironment,
        'chain-indexer',
        /\"message\":\"block indexed\".*\"height\"\s*:\s*("?7"?)/,
        20,
        5000,
      );
    },
  );
});
