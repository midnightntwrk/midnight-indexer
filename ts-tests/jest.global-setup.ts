import { type AllureJestApi } from 'allure-jest/dist/AllureJestApi';
import { execSync } from 'child_process';
import path from 'path';
import { DockerComposeEnvironment, GenericContainer, Wait } from 'testcontainers';
import { createLogger } from './utils/Logger';

const logger = createLogger(path.resolve('logs', `GlobalSetup_${new Date().toISOString().replaceAll(':', '')}.log`));

// for allure annotations
declare global {
  const allure: AllureJestApi;
}

export default async (): Promise<void> => {
  const currentEnv = process.env.ENV_TYPE as string;

  // Setup docker network
  try {
    execSync('docker network inspect midnight-net', { stdio: 'ignore' });
    logger.info('"midnight-net" already exists. Skipping creation.');
  } catch (error) {
    logger.error('"midnight-net" does not exist. Creating it...');
    execSync('docker network create midnight-net');
    logger.info('"midnight-net" has been created. \nPulling midnight-node image');
  }

};
