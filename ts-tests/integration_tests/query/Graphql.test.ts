import { jest } from '@jest/globals';
import fs from 'fs';
import path from 'path';
import { type TestEnv } from '../../environment/ITestEnv';
import { getTestServerEnv } from '../../environment/ServerEnvFactory';
import { getResponseForQuery, getResponse } from '../../utils/HttpRequestUtils';
import { Commons } from '../../utils/Commons';

describe(`Indexer Query Cost and Error Tests - ${process.env.ENV_TYPE} ${process.env.DEPLOYMENT}`, () => {

  const currentDir = path.resolve(new URL(import.meta.url).pathname, '..');
  let serverEnv: TestEnv;

  beforeAll(async () => {
    serverEnv = getTestServerEnv();
    await serverEnv.serverReady();
  });

  afterAll(async () => {
    await serverEnv.tearDownServer();
  });

  beforeEach(() => { });

  afterEach(() => { });

  test('Verify the error message for a query that exceeds the maximum complexity', async () => {

    allure.description("A complex block query that exceeds maximum cost returns an error");
    allure.tms('PM-6614', 'PM-6614');
    allure.severity('blocker');
    allure.tag('query');
    allure.tag('smoke');

    const complexQueryBody = Commons.importGraphQL(currentDir, 'complex_query.graphql');
    const response = await getResponseForQuery(`${serverEnv.getUrl()}/api/v1/graphql`, complexQueryBody);

    expect(response.status).toBe(200);
    const responseBody = await response.body;
    expect(responseBody.data).toBeNull();
    expect(responseBody.errors[0].message).toBe('Query is too complex.');

  });

  test('Verify the error message for a query that exceeds the maximum depth', async () => {

    allure.description("Queries a complex block query that exceeds maximum depth, then verifies that the correct error is returned.");
    allure.tms('PM-6615', 'PM-6615');
    allure.severity('blocker');
    allure.tag('query');
    allure.tag('smoke');

    const queryBody = Commons.importGraphQL(currentDir, 'deep_query.graphql');
    const response = await getResponseForQuery(`${serverEnv.getUrl()}/api/v1/graphql`, queryBody);

    expect(response.status).toBe(200);
    const responseBody = await response.body;
    expect(responseBody.data).toBeNull();
    expect(responseBody.errors[0].message).toBe('Query is nested too deep.');

  });

});
