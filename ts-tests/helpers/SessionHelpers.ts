import { sendQueryAndGetResponse } from '../utils/WebsocketUtils';

import type WebSocket from 'ws';

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export async function getSessionId(client: WebSocket, viewingKey: string): Promise<any> {
  return await sendQueryAndGetResponse(client, '2', {
    id: '2',
    type: 'start',
    payload: {
      query: `mutation { connect ( viewingKey: "${viewingKey}" ) }`,
    },
  });
}

export async function disconnectSession(client: WebSocket, sessionId: string) {
  return await sendQueryAndGetResponse(client, '3', {
    id: '3',
    type: 'start',
    payload: {
      query: `mutation { disconnect ( sessionId: "${sessionId}" ) }`,
    },
  });
}
