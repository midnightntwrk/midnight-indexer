import { jest } from '@jest/globals';
import { type Readable } from 'stream';
import { DockerComposeEnvironment, Wait, type StartedDockerComposeEnvironment } from 'testcontainers';
import { waitForLogAndVerifyIfLogPresent } from '../../helpers/LoggingHelpers';

jest.setTimeout(10 * 3600 * 1000); // 10 hours

describe('Testnet Indexer Benchmark Test', () => {
  let composeEnvironment: StartedDockerComposeEnvironment;

  beforeAll(async () => {});

  afterAll(async () => {});

  beforeEach(() => {});

  afterEach(async () => {
    if (composeEnvironment) await composeEnvironment.down();
  });

  test('Verify that testnet indexing benchmark can be run and it is not degrading', async () => {
    allure.tms('PM-8805', 'PM-8805');
    allure.severity('normal');
    allure.tag('node_communication');

    composeEnvironment = await new DockerComposeEnvironment(
      './docker_composes/',
      'docker-compose-cloud-integrated-env.yaml',
    )
      .withEnvironment({ SUBSTRATE_NODE_WS_URL: 'wss://rpc.testnet.midnight.network', LEDGER_NETWORK_ID: 'TestNet' })
      .withWaitStrategy('nats', Wait.forLogMessage('Listening for client connections on 0.0.0.0:4222'))
      .up();

    await waitForLogAndVerifyIfLogPresent(
      composeEnvironment,
      'chain-indexer',
      /\"message\":\"block indexed\".*\"height\"\s*:\s*("?5000"?)/,
      3100,
      10000,
    );

    const firstBlockIndexedTime = await getTimestampOfLog(
      await composeEnvironment.getContainer('chain-indexer').logs(),
      /\"message\":\"block indexed\".*\"height\"\s*:\s*("?0"?)/,
    );
    const lastBlockIndexedTime = await getTimestampOfLog(
      await composeEnvironment.getContainer('chain-indexer').logs(),
      /\"message\":\"block indexed\".*\"height\"\s*:\s*("?5000"?)/,
    );

    const indexingTimeInSeconds = calculateTimeDifferenceInSeconds(lastBlockIndexedTime, firstBlockIndexedTime);

    allure.description(
      `Starts up indexer locally connected to testnet node and checks the indexing time.
      Current indexing time: ${indexingTimeInSeconds}.`,
    );

    console.log(`Current indexing time: ${indexingTimeInSeconds}.`);

    expect(indexingTimeInSeconds).toBeLessThan(26);
  });

  function getTimestampOfLog(stream: Readable, regexToMatch: RegExp): Promise<string> {
    return new Promise((resolve) => {
      const timestampRegex = /\"timestamp\":\"(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{6}Z)\"/;
      const timeoutId = setTimeout(() => {
        stream.destroy();
      }, 10000);
      stream
        .on('data', (line) => {
          if (line.match(regexToMatch)) {
            clearTimeout(timeoutId);
            stream.destroy();
            resolve(line.match(timestampRegex)[1] as string);
          }
        })
        .on('error', (error) => {
          console.error(error);
          clearTimeout(timeoutId);
          stream.destroy();
        })
        .on('end', () => {
          console.log('Stream closed');
        });
    });
  }

  function calculateTimeDifferenceInSeconds(timestamp1: string, timestamp2: string): number {
    const differenceInMilliseconds = new Date(timestamp1).getTime() - new Date(timestamp2).getTime();
    const differenceInSeconds = differenceInMilliseconds / 1000;
    return Math.abs(differenceInSeconds);
  }
});
