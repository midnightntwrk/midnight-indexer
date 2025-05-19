import { jest } from '@jest/globals';
import * as fs from 'fs';
import * as path from 'path';
import { DockerComposeEnvironment, Wait, type StartedDockerComposeEnvironment } from 'testcontainers';
import { environments, type Environment } from '../../environment/envConfig';
import { sendQueryToPostgres } from '../../helpers/PostgresHelpers';
import { Commons } from '../../utils/Commons';
import { deployCounterContract } from '../../utils/counter/contract-deployer';

describe('Postgres Test', () => {
  jest.setTimeout(120000);

  let composeEnvironment: StartedDockerComposeEnvironment;
  let environment: Environment;
  const currentDir = path.resolve(new URL(import.meta.url).pathname, '..');
  const filePath = path.join(currentDir, '..', '..', 'utils', 'counter', 'deployedContractTxHash.txt');

  beforeAll(async () => {
    environment = environments.compose;
    composeEnvironment = await new DockerComposeEnvironment(
      './docker_composes/',
      'docker-compose-latest-cloud-indexer.yaml',
    )
      .withWaitStrategy('chain-indexer', Wait.forLogMessage(/block indexed.*\"height\"\s*:\s*("?0"?)/))
      .withWaitStrategy('indexer-api', Wait.forLogMessage('listening to TCP connections'))
      .withWaitStrategy('nats', Wait.forLogMessage('Listening for client connections on 0.0.0.0:4222'))
      .up();
  });

  afterAll(async () => {
    if (composeEnvironment) await composeEnvironment.down();
    if (!Commons.isFileEmpty(filePath)) fs.writeFileSync(filePath, '');
  });

  beforeEach(() => {});

  afterEach(async () => {});

  test('Verify that wallets, relevant_transactions and other tables are present in the database', async () => {
    allure.tms('PM-8446', 'PM-8446');
    allure.description(
      `Starts up latest Indexer locally.
      Verifies that the correct tables are present in the database.`,
    );
    allure.severity('normal');
    allure.tag('database');
    const portPostgres = composeEnvironment.getContainer('postgres').getFirstMappedPort();

    const result = await sendQueryToPostgres(
      portPostgres,
      `SELECT table_name FROM information_schema.tables WHERE table_schema = 'public';`,
    );

    const expectedTables = [
      { table_name: '_sqlx_migrations' },
      { table_name: 'blocks' },
      { table_name: 'contract_actions' },
      { table_name: 'relevant_transactions' },
      { table_name: 'transactions' },
      { table_name: 'wallets' },
    ];

    expect(result.rows.length).toBe(expectedTables.length);
    expectedTables.forEach((table) => {
      expect(result.rows).toEqual(expect.arrayContaining([expect.objectContaining(table)]));
    });
  });

  test.skip('not able to deploy contract yet - Verify that zswap chain state is stored for each contract', async () => {
    allure.tms('PM-10435', 'PM-10435');
    allure.description(
      `Starts up latest Indexer locally, deploys a contract and verifies that zswap chain state is recorded for it.`,
    );
    allure.severity('normal');
    allure.tag('database');
    // Deploy contract if necessary
    if (Commons.isFileEmpty(filePath)) await deployCounterContract(environment.wallets[0].seed as string);
    const portPostgres = composeEnvironment.getContainer('postgres').getFirstMappedPort();
    const result1 = await sendQueryToPostgres(portPostgres, 'SELECT * FROM contracts WHERE zswap_chain_state IS NULL;');
    expect(result1.rows.length).toEqual(0);
    const result2 = await sendQueryToPostgres(portPostgres, 'SELECT zswap_chain_state FROM contracts LIMIT 1;');
    expect(result2.rows[0].zswap_chain_state).toMatch(/^[0-9a-f]+$/);
  });
});
