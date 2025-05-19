import { type Environment } from './envConfig';

export interface TestEnv {
  serverReady: () => Promise<unknown>;
  getServer: () => unknown;
  getUrl: () => string;
  getWsUrl: () => string;
  getMappedPort: () => number;
  getEnvConfig: () => Environment;
  tearDownServer: () => Promise<void>;
}
