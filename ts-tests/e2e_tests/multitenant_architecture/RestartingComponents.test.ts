import { jest } from '@jest/globals';
import { type Readable } from 'stream';
import { DockerComposeEnvironment, Wait, type StartedDockerComposeEnvironment } from 'testcontainers';
import { Commons } from '../../utils/Commons';

describe('Restarting Components Test', () => {
  jest.setTimeout(2400000);

  let composeEnvironment: StartedDockerComposeEnvironment;

  beforeAll(async () => {
    composeEnvironment = await new DockerComposeEnvironment(
      './docker_composes/',
      'docker-compose-latest-cloud-indexer.yaml',
    )
      .withWaitStrategy('nats', Wait.forLogMessage('Listening for client connections on 0.0.0.0:4222'))
      .withWaitStrategy('chain-indexer', Wait.forLogMessage(/\"message\":\"block indexed\".*\"height\"\s*:\s*("?0"?)/))
      .up();
  });

  afterAll(async () => {
    if (composeEnvironment) await composeEnvironment.down();
  });

  beforeEach(() => { });

  afterEach(async () => { });

  test.skip('nats does not raise warn message - Verify that when components are down WARN messages are raised', async () => {

    allure.description(`Starts up latest Indexer and stack locally, restarts the node/postgres services, finally it verifies that the log messages have WARN levels.`);
    allure.tms('PM-9645', 'PM-9645');
    allure.severity('normal');
    allure.tag('multitenant_architecture');

    // Restart node
    await Commons.restartComponent(composeEnvironment, 'node', 1000);

    // Verify log level for node
    verifyLogHasWarnLevel(await composeEnvironment.getContainer('chain-indexer').logs(), [
      'node disconnected, reconnecting',
    ]);

    // Restart postgres
    await Commons.restartComponent(composeEnvironment, 'postgres', 5000);
    // Verify log level for postgres
    verifyLogHasWarnLevel(await composeEnvironment.getContainer('chain-indexer').logs(), [
      'process exited with ERROR',
      'Failed to open `.pgpass` file: Os',
    ]);

    // Restart nats
    await Commons.restartComponent(composeEnvironment, 'nats', 5000);
    // Verify log level for nats
    verifyLogHasWarnLevel(await composeEnvironment.getContainer('chain-indexer').logs(), [
      'event: client error: nats: IO error',
    ]);
  });

  function verifyLogHasWarnLevel(stream: Readable, textsToWaitFor: string[]) {
    let textAppeared = false;
    const timeoutId = setTimeout(() => {
      stream.destroy();
    }, 10000);
    stream
      .on('data', (line) => {
        if (textsToWaitFor.some((text) => line.includes(text))) {
          textAppeared = true;
          expect(line).toMatch(/\"level\":\"WARN\"/);
          clearTimeout(timeoutId);
        }
      })
      .on('error', (error) => {
        console.error(error);
      })
      .on('end', () => {
        expect(textAppeared).toBe(true);
      });
  }

});
