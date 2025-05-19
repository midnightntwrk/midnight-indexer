import { DockerComposeEnvironment, Wait, type StartedDockerComposeEnvironment } from 'testcontainers';
import { type TestEnv } from './ITestEnv';
import { environments, type Environment } from './envConfig';

export class TestContainersComposeEnvironment implements TestEnv {
  private startedEnvironment: StartedDockerComposeEnvironment;
  private readonly deployment = process.env.DEPLOYMENT ?? 'cloud';

  async serverReady(): Promise<StartedDockerComposeEnvironment> {
    let composeFile: string;
    let chainIndexerContainer: string;
    if (this.deployment === 'cloud') {
      composeFile = 'docker-compose-latest-cloud-indexer.yaml';
      chainIndexerContainer = 'chain-indexer';
    } else if (this.deployment === 'standalone'){
      composeFile = 'docker-compose-latest-standalone-indexer.yaml';
      chainIndexerContainer = 'indexer-standalone';
    } else {
      throw new Error(`Unexpected local test environment: ${this.deployment} please use "cloud" or "standalone" by setting DEPLOYMENT env variable`);
    }
    this.startedEnvironment = await new DockerComposeEnvironment('./docker_composes/', composeFile)
      .withWaitStrategy('nats', Wait.forLogMessage('Listening for client connections on 0.0.0.0:4222'))
      .withWaitStrategy(chainIndexerContainer, Wait.forLogMessage(/\"message\":\"block indexed\".*\"height\"\s*:\s*("?0"?)/))
      .up();
    return this.startedEnvironment;
  }

  getServer(): StartedDockerComposeEnvironment {
    return this.startedEnvironment;
  }

  getUrl(): string {
    return `http://localhost:${this.getMappedPort()}`;
  }

  getWsUrl(): string {
    return `ws://localhost:${this.getMappedPort()}`;
  }

  getMappedPort(): number {
    const indexerApiContainer = this.deployment === 'cloud' ? 'indexer-api' : 'indexer-standalone';
    return this.startedEnvironment.getContainer(indexerApiContainer).getFirstMappedPort();
  }

  getEnvConfig(): Environment {
    return environments.compose;
  }

  async tearDownServer(): Promise<void> {
    if (this.startedEnvironment) {
      await this.startedEnvironment.down();
    }
  }
}
