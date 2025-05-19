import { get } from 'https';
import path from 'node:path';
import { type TestEnv } from './ITestEnv';
import { graphQlWaitStrategy } from './Strategies';
import { type Environment } from './envConfig';
import { Commons } from '../utils/Commons';
import { getResponse } from '../utils/HttpRequestUtils';
import { createLogger } from '../utils/Logger';

const logger = createLogger(path.resolve('logs', `IntegratedEnvServer.log`));

class IntegratedEnvServer implements TestEnv {
  private readonly server: unknown;
  private readonly port: number;

  environment: Environment;

  constructor(environment: Environment) {
    this.port = 443;
    this.environment = environment;
  }

  async serverReady(): Promise<void> {

    let attempts = 0;
    const MAX_ATTEMPTS = 3

    logger.info(`Checking indexer is up and synced`);
    let response = await getResponse(`${this.getUrl()}/ready`);
    attempts++;

    while (response.status !== 200) {

      logger.warn(`Indexer not ready yet, received ${response.status}`);
      if (attempts === MAX_ATTEMPTS) {
        const errorMessage = `Indexer not ready yet, timed out after ${MAX_ATTEMPTS} attempts`;
        logger.error(errorMessage);
        throw new Error(errorMessage);
      }

      logger.warn(`Waiting ${response.status / 1000} secs`);
      await Commons.sleep(2000);

      response = await getResponse(`${this.getUrl()}/ready`);
      attempts++
    }

    logger.info(`Indexer up and synced`);
    await graphQlWaitStrategy(this.getUrl());

  }

  getServer(): unknown {
    return this.server;
  }

  getUrl(): string {
    return this.environment.indexer_http_url;
  }

  getWsUrl(): string {
    return this.environment.indexer_ws_url;
  }

  getMappedPort(): number {
    return this.port;
  }

  getEnvConfig(): Environment {
    return this.environment;
  }

  async tearDownServer(): Promise<void> { }
}


export { IntegratedEnvServer };