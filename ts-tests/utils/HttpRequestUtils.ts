import path from 'node:path';
import { URL } from 'url';
import fetch from 'node-fetch';
import { Agent as HttpAgent } from 'http';
import { Agent as HttpsAgent } from 'https';
import { createLogger } from './Logger';

const logger = createLogger(path.resolve('logs', `Http_Requests.log`));

/* eslint-disable @typescript-eslint/no-explicit-any */
async function getResponse(url: string, request?: any) {

  // The following approach uses Http and Https agents
  // (based on the url of the request) so that we can
  // add the keepAlive = false param. This is to allow
  // fetch to leave open handles that are detected by
  // jest, slowing down the completion of the test
  // execution
  const parsedUrl = new URL(url);
  const isHttps = parsedUrl.protocol === 'https:';
  const agent = isHttps
    ? new HttpsAgent({ keepAlive: false })
    : new HttpAgent({ keepAlive: false });

  // The other nasty thing with fetch is to always make sure
  // to consume the response content, otherwise, being a stream
  // it can be left open in case of errors
  const response = await fetch(url, { ...request, agent });
  const bodyText = await response.text();

  // Are these needed, they could be changed in debug logs
  logger.info(`URL: ${url}`);
  logger.info(`REQUEST: ${JSON.stringify(request)}`);
  logger.info(`RESPONSE STATUS CODE: ${response.status}`);

  // We try to parse the body in json format, if we fail
  // we will use plain text, but it's up to the caller
  // to know the format to consume based on what's its
  // expectation
  let parsedBody;
  try {
    parsedBody = JSON.parse(bodyText);
    logger.info(`RESPONSE BODY: ${JSON.stringify(parsedBody)}`);
  } catch {
    parsedBody = bodyText;
    logger.info(`RESPONSE BODY: ${parsedBody}`);
  }

  return {
    status: response.status,
    body: parsedBody,
    headers: response.headers
  };

}

async function getResponseForQuery(url: string, queryBody: any) {
  const request = {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ query: queryBody }),
  };
  return await getResponse(url, request);
}

export { getResponse };
export { getResponseForQuery };
