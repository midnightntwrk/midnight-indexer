import { type Config } from '@jest/types';

const config: Config.InitialOptions = {
  preset: 'ts-jest',
  verbose: true,
  testTimeout: 180000,
  testPathIgnorePatterns: ['node_modules', 'dist'],
  passWithNoTests: true,
  maxWorkers: 1,
  extensionsToTreatAsEsm: ['.ts'],
  testMatch: ['**/*.test.ts'],
  transform: {
    '^.+\\.ts$': ['ts-jest', { tsconfig: 'tsconfig.json', useESM: true }],
  },
  reporters: [
    'default',
    [
      'jest-junit',
      {
        suiteName: 'jest tests',
        outputDirectory: './reports/jest',
        outputName: 'testResults.xml',
      },
    ],
  ],
  globalSetup: './jest.global-setup.ts',
  testEnvironment: 'allure-jest/node',
  testEnvironmentOptions: {
    resultsDir: './reports/allure-results',
    links: [
      {
        type: 'tms',
        urlTemplate: 'https://shielded.atlassian.net/browse/%s',
      },
    ],
  },
  resolver: '<rootDir>/js-resolver.cjs',
};

export default config;
