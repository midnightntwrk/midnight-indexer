import { jest } from '@jest/globals';
import { type Readable } from 'stream';
import { DockerComposeEnvironment, Wait, type StartedDockerComposeEnvironment } from 'testcontainers';
import { waitForLogAndVerifyIfLogPresent } from '../../helpers/LoggingHelpers';

describe('Rust Indexer Benchmark Test', () => {
  jest.setTimeout(300000);

  let composeEnvironment: StartedDockerComposeEnvironment;

  beforeAll(async () => { });

  afterAll(async () => { });

  beforeEach(() => { });

  afterEach(async () => {
    if (composeEnvironment) await composeEnvironment.down();
  });

  test('Verify that indexing benchmark can be run and it is not degrading - rust', async () => {
    allure.tms('PM-8805', 'PM-8805');
    allure.severity('normal');
    allure.tag('node_communication');

    composeEnvironment = await new DockerComposeEnvironment(
      './docker_composes/',
      'docker-compose-latest-benchmark-rust.yml',
    )
      .withWaitStrategy('nats', Wait.forLogMessage('Listening for client connections on 0.0.0.0:4222'))
      .withWaitStrategy('chain-indexer', Wait.forLogMessage(/\"message\":\"block fetched\".*\"height\"\s*:\s*("?0"?)/))
      .up();

    await waitForLogAndVerifyIfLogPresent(
      composeEnvironment,
      'chain-indexer',
      /\"message\":\"block indexed\".*\"height\"\s*:\s*("?0"?)/,
      30,
      5000,
    );
    console.log("Found genesis block in chain");

    await waitForLogAndVerifyIfLogPresent(
      composeEnvironment,
      'chain-indexer',
      /\"message\":\"block indexed\".*\"height\"\s*:\s*("?100"?)/,
      30,
      5000,
    );
    console.log("Found 100th block");

    await waitForLogAndVerifyIfLogPresent(
      composeEnvironment,
      'chain-indexer',
      /\"message\":\"block indexed\".*\"height\"\s*:\s*("?500"?)/,
      30,
      5000,
    );
    console.log("Found 500th block");

    await waitForLogAndVerifyIfLogPresent(
      composeEnvironment,
      'chain-indexer',
      /\"message\":\"block indexed\".*\"height\"\s*:\s*("?15000"?)/,
      30,
      5000,
    );
    console.log("Found 15000th block");

    const firstBlockIndexedTime = await getTimestampOfLog(
      await composeEnvironment.getContainer('chain-indexer').logs(),
      /\"message\":\"block indexed\".*\"height\"\s*:\s*("?0"?)/,
    );
    const lastBlockIndexedTime = await getTimestampOfLog(
      await composeEnvironment.getContainer('chain-indexer').logs(),
      /\"message\":\"block indexed\".*\"height\"\s*:\s*("?15000"?)/,
    );

    const indexingTimeInSeconds = calculateTimeDifferenceInSeconds(lastBlockIndexedTime, firstBlockIndexedTime);

    allure.description(
      `Starts up indexer locally connected to a node with 15k blocks.
      Current indexing time: ${indexingTimeInSeconds}.`,
    );

    console.log(`Current indexing time: ${indexingTimeInSeconds}.`);

    expect(indexingTimeInSeconds).toBeLessThan(26);
  });

  test('Verify that contract indexing benchmark can be run and it is not degrading - rust', async () => {
    allure.tms('PM-9839', 'PM-9839');
    allure.severity('normal');
    allure.tag('node_communication');

    composeEnvironment = await new DockerComposeEnvironment(
      './docker_composes/',
      'docker-compose-latest-benchmark-rust-contract.yml',
    )
      .withWaitStrategy('nats', Wait.forLogMessage('Listening for client connections on 0.0.0.0:4222'))
      .withWaitStrategy('chain-indexer', Wait.forLogMessage(/\"message\":\"block indexed\".*\"height\"\s*:\s*("?0"?)/))
      .up();

    await waitForLogAndVerifyIfLogPresent(
      composeEnvironment,
      'chain-indexer',
      /\"message\":\"block indexed\".*\"height\"\s*:\s*("?1141"?)/,
      30,
      5000,
    );

    const firstBlockIndexedTime = await getTimestampOfLog(
      await composeEnvironment.getContainer('chain-indexer').logs(),
      /\"message\":\"block indexed\".*\"height\"\s*:\s*("?0"?)/,
    );
    const lastBlockIndexedTime = await getTimestampOfLog(
      await composeEnvironment.getContainer('chain-indexer').logs(),
      /\"message\":\"block indexed\".*\"height\"\s*:\s*("?1141"?)/,
    );

    const indexingTimeInSeconds = calculateTimeDifferenceInSeconds(lastBlockIndexedTime, firstBlockIndexedTime);

    allure.description(
      `Starts up indexer locally connected to a node with 200 contracts.
      Current indexing time: ${indexingTimeInSeconds}.`,
    );

    console.log(`Current indexing time: ${indexingTimeInSeconds}.`);

    expect(indexingTimeInSeconds).toBeLessThan(14);
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
