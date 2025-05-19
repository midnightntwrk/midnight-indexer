import path from 'node:path';
import WebSocket from 'ws';
import { createLogger } from './Logger';

const logger = createLogger(path.resolve('logs', `Websocket_Messages.log`));

/* eslint-disable @typescript-eslint/no-explicit-any */
export function sendQueryAndGetResponse(
  client: WebSocket,
  id: string,
  query: object,
  onData?: (parsedMessage: any) => boolean,
  handleEmptyData: boolean = false,
): Promise<any> {
  logger.info(`QUERY: ${JSON.stringify(query)}`);

  let emptyDataTimeout: NodeJS.Timeout | undefined;

  return new Promise<any>((resolve, reject) => {
    if (handleEmptyData) {
      emptyDataTimeout = setTimeout(() => {
        resolve('no_data');
      }, 5000);
    }

    const handleClose = (code: number, reason: string) => {
      if (handleEmptyData && emptyDataTimeout) {
        clearTimeout(emptyDataTimeout);
      }
      reject(new Error(`Connection closed with code: ${code}, reason: ${reason}`));
    };
    client.on('close', handleClose);

    const messageHandler = (data: WebSocket.Data) => {
      if (handleEmptyData && emptyDataTimeout) {
        clearTimeout(emptyDataTimeout);
      }

      try {
        // eslint-disable-next-line @typescript-eslint/no-base-to-string
        const message = typeof data === 'string' ? data : data.toString();
        const parsedMessage = JSON.parse(message);
        logger.info(`RESPONSE MESSAGE: ${JSON.stringify(parsedMessage)}`);

        if (parsedMessage.id !== id) return;
        if (parsedMessage.type === 'complete') return;

        if (onData && onData(parsedMessage)) {
          client.removeListener('message', messageHandler);
          client.removeListener('close', handleClose);
          resolve(parsedMessage);
          return;
        }

        resolve(parsedMessage);
      } catch (error) {
        const errorMessage: string = `An error occurred while parsing the message: ${error.message}`;
        reject(new Error(errorMessage));
      }
    };

    client.on('message', messageHandler);

    try {
      client.send(JSON.stringify(query));
    } catch (error) {
      const errorMessage: string = `An error occurred while parsing the message: ${error.message}`;
      reject(new Error(errorMessage));
    }
  });
}

export async function initClientWithHeader(wsUrl: string, token: string) {
  return await new Promise<WebSocket>((resolve) => {
    const headers = {
      Authorization: `${token}`,
    };

    const client = new WebSocket(`${wsUrl}/api/v1/graphql/ws`, ['graphql-ws'], { headers });

    client.on('open', () => {
      resolve(client);
    });
  });
}
