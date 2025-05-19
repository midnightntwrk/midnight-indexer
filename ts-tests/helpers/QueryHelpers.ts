import { sendQueryAndGetResponse } from '../utils/WebsocketUtils';

import type WebSocket from 'ws';

export async function getTxInfo(client: WebSocket, txOffset?: string) {
  let offset = '';
  if (txOffset) {
    offset = txOffset;
  }

  return await sendQueryAndGetResponse(client, '2', {
    id: '2',
    type: 'start',
    payload: {
      query: `query { transactions ${offset} { hash block { height hash } applyStage identifiers contractCalls { __typename address state } } }`,
    },
  });
}

export async function getBlockInfo(client: WebSocket, blockOffsetInput?: string) {
  let offset = '';
  if (blockOffsetInput) {
    offset = blockOffsetInput;
  }

  return await sendQueryAndGetResponse(client, '3', {
    id: '3',
    type: 'start',
    payload: {
      query: `query { block ${offset} { hash height timestamp transactions { hash identifiers } } }`,
    },
  });
}
