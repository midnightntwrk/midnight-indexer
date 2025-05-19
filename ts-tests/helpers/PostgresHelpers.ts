import pg from 'pg';

export async function sendQueryToPostgres(portPostgres: number, queryToSend: string): Promise<pg.QueryResult> {
  const dbClient = new pg.Client({
    user: 'indexer',
    host: 'localhost',
    database: 'indexer',
    password: 'indexer',
    port: portPostgres,
  });
  await dbClient.connect();
  const result = await dbClient.query(queryToSend);
  await dbClient.end();
  return result;
}
