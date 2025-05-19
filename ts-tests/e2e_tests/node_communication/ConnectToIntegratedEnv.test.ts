import { jest } from '@jest/globals';
import { DockerComposeEnvironment, Wait, type StartedDockerComposeEnvironment } from 'testcontainers';
import { environments, type Environments } from '../../environment/envConfig';
import { waitForLogAndVerifyIfLogPresent } from '../../helpers/LoggingHelpers';

describe('Connect to remote node', () => {
  jest.setTimeout(300000);

  let composeEnvironment: StartedDockerComposeEnvironment;
  let substrateNodeWsUrl: string;
  let ledgerNetworkId: string;
  let currentEnv: string | Environments;

  beforeAll(() => {
    currentEnv = process.env.ENV_TYPE as string;
    if (currentEnv !== 'compose') {
      substrateNodeWsUrl = environments[currentEnv as keyof Environments].substrate_node_ws_url;
      ledgerNetworkId = environments[currentEnv as keyof Environments].ledger_network_id;
    }
  });

  afterAll(async () => { });

  beforeEach(() => { });

  afterEach(async () => {
    if (composeEnvironment) await composeEnvironment.down();
  });

  jest.retryTimes(1);
  test('Verify that local indexer can connect to node on integrated environment using postgres', async () => {

    allure.description(`Starts up Indexer with postgres database and waits until 100 blocks are received.`);
    allure.tms('PM-9549', 'PM-9549');
    allure.severity('normal');
    allure.tag('node_communication');

    if (!substrateNodeWsUrl) {
      console.log(`Skipping test. ENV_TYPE is set to '${currentEnv as string}'`);
      return;
    }
    composeEnvironment = await new DockerComposeEnvironment(
      './docker_composes/',
      'docker-compose-cloud-integrated-env.yaml',
    )
      .withEnvironment({ SUBSTRATE_NODE_WS_URL: substrateNodeWsUrl, LEDGER_NETWORK_ID: ledgerNetworkId })
      .withWaitStrategy('nats', Wait.forLogMessage('Listening for client connections on 0.0.0.0:4222'))
      .up();

    const regexesToVerify = [
      /loading zswap_state/,
      /subscribing to finalized blocks/,
      /\"message\":\"block received\",\"hash\":\".*\",\"height\"\s*:\s*("?[0-9]+"?)/,
      /traversing back via parent hashes, this may take some time/,
      /\"message\":\"traversing back via parent hashes\",\"highest_stored_height\":.*,\"current_height\":[0-9]+,\"first_finalized_height\":[0-9]+/,
    ];

    for (const regex of regexesToVerify) {
      await waitForLogAndVerifyIfLogPresent(composeEnvironment, 'chain-indexer', regex, 10, 5000);
    }
  });

  // Skipping this test for now, but check with the developer if the local indexer can connect to a remote
  // node via sqlite, because as far as I understand, the cloud topology of the indexer, only supports Postgres
  test.skip('Verify that local indexer can connect to node on integrated environment using sqlite', async () => {
    allure.tms('PM-9551', 'PM-9551');
    allure.description(
      `Running on environment '${currentEnv as string}'.
      Starts up Indexer with sqlite database and waits until 100 blocks are received.`,
    );
    allure.severity('normal');
    allure.tag('node_communication');
    if (!substrateNodeWsUrl) {
      console.log(`Skipping test. ENV_TYPE is set to '${currentEnv as string}'`);
      return;
    }
    composeEnvironment = await new DockerComposeEnvironment(
      './docker_composes/',
      'docker-compose-local-integrated-env.yaml',
    )
      .withEnvironment({ SUBSTRATE_NODE_WS_URL: substrateNodeWsUrl, LEDGER_NETWORK_ID: ledgerNetworkId })
      .withWaitStrategy('indexer', Wait.forLogMessage(/\"message\":\"starting\"/))
      .up();

    const regexesToVerify = [
      /starting indexing/,
      /subscribing to finalized blocks/,
      /\"message\":\"block received\",\"hash\":\".*\",\"height\"\s*:\s*("?[0-9]+"?)/,
      /traversing back via parent hashes, this may take some time/,
      /\"message\":\"traversing back via parent hashes\",\"highest_stored_height\":.*,\"current_height\":[0-9]+,\"first_finalized_height\":[0-9]+/,
    ];

    for (const regex of regexesToVerify) {
      await waitForLogAndVerifyIfLogPresent(composeEnvironment, 'indexer', regex, 10, 5000);
    }
  });
});
