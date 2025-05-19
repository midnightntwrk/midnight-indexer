import { jest } from '@jest/globals';
import { DockerComposeEnvironment, Wait, type StartedDockerComposeEnvironment } from 'testcontainers';
import WebSocket from 'ws';
import { environments, type Environment } from '../../environment/envConfig';
import { verifyThatLogIsPresent, waitForLogAndVerifyIfLogPresent } from '../../helpers/LoggingHelpers';
import { getSessionId } from '../../helpers/SessionHelpers';
import { initConnection, verifyWalletSubscription, walletSubscription } from '../../helpers/SubscriptionHelpers';
import { Commons } from '../../utils/Commons';

describe('Wallet Indexing Tests', () => {
  jest.setTimeout(240000);

  let composeEnvironment: StartedDockerComposeEnvironment;
  let environment: Environment;

  beforeAll(() => {
    environment = environments.compose;
  });

  afterAll(async () => { });

  beforeEach(() => { });

  afterEach(async () => {
    if (composeEnvironment) await composeEnvironment.down();
  });

  test('Verify that indexer sends Viewing Updates and indexes wallet when running in local operation mode', async () => {

    allure.tms('PM-10208', 'PM-10208');
    allure.description(
      `Starts up latest Indexer in standalone mode and verifies that it sends viewing updates and indexes wallet when subscribing to a wallet.`,
    );
    allure.severity('normal');
    allure.tag('walletIndexing');

    composeEnvironment = await new DockerComposeEnvironment(
      './docker_composes/',
      'docker-compose-latest-standalone-indexer.yaml',
    )
      .withWaitStrategy('indexer', Wait.forLogMessage(/\"message\":\"block indexed\".*\"height\"\s*:\s*("?0"?)/))
      .up();
    const indexerPort = composeEnvironment.getContainer('indexer-standalone').getFirstMappedPort();
    await verifyWalletIndexing(indexerPort, 'indexer-standalone');
  });

  test('Verify that indexer sends Viewing Updates and indexes wallet when running in multi-tenant mode', async () => {

    allure.tms('PM-10209', 'PM-10209');
    allure.description(
      `Starts up latest Indexer in multi-tenant mode and verifies that it sends viewing updates and indexes wallet when subscribing to a wallet.`,
    );
    allure.severity('normal');
    allure.tag('walletIndexing');
    composeEnvironment = await new DockerComposeEnvironment(
      './docker_composes/',
      'docker-compose-latest-cloud-indexer.yaml',
    )
      .withWaitStrategy('nats', Wait.forLogMessage('Listening for client connections on 0.0.0.0:4222'))
      .withWaitStrategy('chain-indexer', Wait.forLogMessage(/\"message\":\"block indexed\".*\"height\"\s*:\s*("?0"?)/))
      .up();
    await verifyWalletIndexing(composeEnvironment.getContainer('indexer-api').getFirstMappedPort(), 'wallet-indexer');

  });

  test('Verify that connecting wallets simultaneously before and after restart works as expected using local operation mode', async () => {

    allure.tms('PM-10223', 'PM-10223');
    allure.description(
      `Restart the latest Indexer in standalone operation mode, connects 10 wallets and verifies that correct session ids are received.`,
    );
    allure.severity('normal');
    allure.tag('walletIndexing');
    composeEnvironment = await new DockerComposeEnvironment(
      './docker_composes/',
      'docker-compose-latest-standalone-indexer.yaml',
    )
      .withWaitStrategy('indexer', Wait.forLogMessage('listening to TCP connections'))
      .up();
    // Verify Before restart
    await verifyWalletSubscription(composeEnvironment.getContainer('indexer-standalone').getFirstMappedPort());
    // Restart
    await Commons.restartComponent(composeEnvironment, 'indexer-standalone', 1000);
    await waitForLogAndVerifyIfLogPresent(composeEnvironment, 'indexer-standalone', /block indexed/, 30, 1000);
    // Verify after restart
    await verifyWalletSubscription(composeEnvironment.getContainer('indexer-standalone').getFirstMappedPort());
  });

  test('Verify that connecting wallets simultaneously before and after restart works as expected using multi-tenant mode', async () => {

    allure.tms('PM-10224', 'PM-10224');
    allure.description(
      `Restart the latest Indexer in multi-tenant mode, connects 10 wallets and verifies that correct session ids are received.`,
    );
    allure.severity('normal');
    allure.tag('walletIndexing');

    composeEnvironment = await new DockerComposeEnvironment(
      './docker_composes/',
      'docker-compose-latest-cloud-indexer.yaml',
    )
      .withWaitStrategy('nats', Wait.forLogMessage('Listening for client connections on 0.0.0.0:4222'))
      .withWaitStrategy('indexer-api', Wait.forLogMessage('listening to TCP connections'))
      .up();
    // Verify Before restart
    await verifyWalletSubscription(composeEnvironment.getContainer('indexer-api').getFirstMappedPort());
    // Restart
    const restartPromises = [
      Commons.restartComponent(composeEnvironment, 'chain-indexer', 1000),
      Commons.restartComponent(composeEnvironment, 'wallet-indexer', 1000),
      Commons.restartComponent(composeEnvironment, 'indexer-api', 1000),
    ];
    await Promise.all(restartPromises);
    await waitForLogAndVerifyIfLogPresent(composeEnvironment, 'indexer-api', /listening to TCP connections/, 30, 1000);
    // Verify after restart
    await verifyWalletSubscription(composeEnvironment.getContainer('indexer-api').getFirstMappedPort());

  });

  test('Verify that sqlite database is not locked when connecting wallets', async () => {

    allure.tms('PM-10418', 'PM-10418');
    allure.description(`Connect wallets and verify that 'Wallet indexer worker error' is not apparent.`);
    allure.severity('normal');
    allure.tag('walletIndexing');

    // Start environment
    composeEnvironment = await new DockerComposeEnvironment(
      './docker_composes/',
      'docker-compose-latest-standalone-indexer.yaml',
    )
      .withWaitStrategy('indexer', Wait.forLogMessage(/\"message\":\"block indexed\".*\"height\"\s*:\s*("?0"?)/))
      .up();
    // Connect wallets
    const indexerPort = composeEnvironment.getContainer('indexer-standalone').getFirstMappedPort();
    await verifyWalletSubscription(indexerPort);
    // Check if log is present
    const textToWaitFor = 'The database file is locked';
    const stream = await composeEnvironment.getContainer('indexer-standalone').logs();
    const timeoutId = setTimeout(() => {
      stream.destroy();
    }, 5000);
    stream
      .on('data', (line) => {
        if ((line as string).includes(textToWaitFor)) {
          clearTimeout(timeoutId);
          throw Error(`Log entry found: '${textToWaitFor}'`);
        }
      })
      .on('error', (error) => {
        clearTimeout(timeoutId);
        console.error(error);
      })
      .on('end', () => {
        clearTimeout(timeoutId);
      });

  });

  test('Verify that when a wallet-indexer is restarted a different wallet-indexer can continue indexing', async () => {

    allure.tms('PM-9685', 'PM-9685');
    allure.description(`
      1. Starts up the latest indexer with 2 wallet-indexers
      2. Stops wallet-indexer-2
      3. Then verifies that a wallet subscription is picked up by wallet-indexer-1
      4. Starts wallet-indexer-2 and stops wallet-indexer-1
      5. Then verifies that a wallet subscription is picked up by wallet-indexer-2
      `);
    allure.severity('normal');
    allure.tag('walletIndexing');
    composeEnvironment = await new DockerComposeEnvironment(
      './docker_composes/',
      'docker-compose-latest-cloud-2-wallet-indexers.yaml',
    )
      .withWaitStrategy('nats', Wait.forLogMessage('Listening for client connections on 0.0.0.0:4222'))
      .withWaitStrategy('indexer-api', Wait.forLogMessage('listening to TCP connections'))
      .up();
    const indexerApiPort = composeEnvironment.getContainer('indexer-api').getFirstMappedPort();

    // Stop wallet-indexer-2
    await composeEnvironment.getContainer('wallet-indexer-2').stop({ remove: false });
    // 1st subscription
    await verifyWalletSubscription(indexerApiPort, 0);
    // Verify that subscription is picked up by wallet-indexer-1
    await verifyThatLogIsPresent(
      await composeEnvironment.getContainer('wallet-indexer-1').logs(),
      /wallet indexed/,
      5000,
    );

    // Restart wallet-indexer-2
    await composeEnvironment.getContainer('wallet-indexer-2').restart();
    // Stop wallet-indexer-1
    await composeEnvironment.getContainer('wallet-indexer-1').stop({ remove: false });
    // 2nd subscription
    await verifyWalletSubscription(indexerApiPort, 1);
    // Verify that subscription is picked up by wallet-indexer-2
    await verifyThatLogIsPresent(
      await composeEnvironment.getContainer('wallet-indexer-2').logs(),
      /wallet indexed/,
      5000,
    );

  });

  async function verifyWalletIndexing(port: number, walletIndexerContainerName: string) {

    const client = new WebSocket(`http://localhost:${port}/api/v1/graphql/ws`, ['graphql-ws']);
    await new Promise((resolve) => client.on('open', resolve));
    const preFundedViewingKey = environment.wallets[0].viewingKey;
    await initConnection(client);
    const response = await getSessionId(client, preFundedViewingKey);
    const sessionId = response.payload.data.connect as string;
    const subscriptionResponse = await walletSubscription(client, false, sessionId);
    expect(subscriptionResponse.payload.data.wallet.__typename).toEqual('ViewingUpdate');
    const indexerStream = await composeEnvironment.getContainer(walletIndexerContainerName).logs();
    await verifyThatLogIsPresent(
      indexerStream,
      /\"level\":\"INFO\",\"message\":\"wallet indexed\",\"session_id\":\"SessionId\(.*\)",\"from\":1/,
      2000,
    );
  }
});
