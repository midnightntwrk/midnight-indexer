import { Commons } from '../utils/Commons';
import { getResponseForQuery } from '../utils/HttpRequestUtils';

export async function graphQlWaitStrategy(url: string): Promise<void> {

  const maxRetries = 10;
  const queryBody = 'query { block ( offset: { height: 1 } ) { height } }';

  for (let attempt = 1; attempt <= maxRetries; attempt++) {
    try {
      const response = await getResponseForQuery(`${url}/api/v1/graphql`, queryBody);

      if (response.status === 200) {
        const body = await response.body;

        // Once the wait strategy condition is satisfied, we return
        if (body?.data?.block?.height === 1) return;

      }
    } catch (error) {
      console.log('Error querying GraphQL. Retrying...', error.message, error);
    }

    // Delay and retry with exp backoff strategy
    await Commons.sleep(1000 * Math.pow(2, attempt));
  }

  throw new Error('Data not ready after max retries.');

}
