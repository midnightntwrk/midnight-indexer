import { environments } from './envConfig';
import { IntegratedEnvServer } from './IntegratedEnvServer';
import { type TestEnv } from './ITestEnv';
import { TestContainersComposeEnvironment } from './TestContainersComposeEnvironment';

export function getTestServerEnv(): TestEnv {

  const envType: string | undefined = process.env.ENV_TYPE;

  if (envType === undefined) {
    throw new Error("Please set ENV_TYPE that specifies the target environment")
  }

  // Environment map
  const testEnvMap: Record<string, () => TestEnv> = {
    'compose': () => new TestContainersComposeEnvironment,
    'nodedev01': () => new IntegratedEnvServer(environments.nodedev01),
    'qanet': () => new IntegratedEnvServer(environments.qanet),
    'testnet': () => new IntegratedEnvServer(environments.testnet),
    'testnet02': () => new IntegratedEnvServer(environments.testnet02),
  };

  // Check the target environment is in the expected/supported environments
  if (!(envType in testEnvMap)) {
    throw new Error(`ENV_TYPE=${testEnvMap} is not yet supported`)
  }

  if (envType !== 'compose') {
    process.env.DEPLOYMENT = "N/A";
  }

  return testEnvMap[envType]();
}
