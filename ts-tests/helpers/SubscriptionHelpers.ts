import WebSocket from 'ws';
import { environments, type Environment, type Wallet } from '../environment/envConfig';
import { sendQueryAndGetResponse } from '../utils/WebsocketUtils';
import { getSessionId } from './SessionHelpers';

export async function initConnection(client: WebSocket): Promise<void> {
  await new Promise<void>((resolve) => {
    client.send(JSON.stringify({ id: '1', type: 'connection_init' }));
    client.on('message', (data) => {
      const message = JSON.parse(data as unknown as string);
      if (message.type === 'connection_ack') {
        resolve();
      }
    });
  });
}

export async function contractSubscription(client: WebSocket, id: string, address: string, offset: string) {
  return await sendQueryAndGetResponse(client, id, {
    id,
    type: 'start',
    payload: {
      query: `subscription { contract ( address: "${address}", offset: ${offset} ) { address state } }`,
    },
  });
}

export async function contractSubscriptionWithTransactionOffset(
  client: WebSocket,
  id: string,
  address: string,
  transactionOffset: string,
) {
  return await sendQueryAndGetResponse(client, id, {
    id,
    type: 'start',
    payload: {
      query: `subscription { contract ( address: "${address}", transactionOffset: ${transactionOffset} ) { address state } }`,
    },
  });
}

export async function emptyContractSubscription(client: WebSocket, id: string, address: string, offset: string) {
  return await sendQueryAndGetResponse(
    client,
    id,
    {
      id,
      type: 'start',
      payload: {
        query: `subscription { contract ( address: "${address}", offset: ${offset} ) { address state } }`,
      },
    },
    undefined,
    true,
  );
}

export async function contractSubscriptionNoOffsets(client: WebSocket, id: string, address: string) {
  return await sendQueryAndGetResponse(client, id, {
    id,
    type: 'start',
    payload: {
      query: `subscription { contract ( address: "${address}" ) { address state } }`,
    },
  });
}

export async function transactionSubscription(client: WebSocket, offset?: string) {
  let filter = '';
  if (offset) {
    filter = offset;
  }

  return await sendQueryAndGetResponse(client, '7', {
    id: '7',
    type: 'start',
    payload: {
      query: `subscription { transactions ${filter} { __typename ... on TransactionAdded { transaction { hash identifiers block { height hash } } } } }`,
    },
  });
}

export async function blocksSubscription(client: WebSocket, blockOffsetInput?: string) {
  let offset = '';
  if (blockOffsetInput) {
    offset = blockOffsetInput;
  }

  return await sendQueryAndGetResponse(client, '8', {
    id: '8',
    type: 'start',
    payload: {
      query: `subscription{ blocks ${offset} { hash height } }`,
    },
  });
}

export async function walletSubscription(
  client: WebSocket,
  sendProgressUpdates: boolean,
  sessionId?: string,
  index?: number,
) {
  let filter = '';
  if (sessionId) {
    filter += `sessionId: \"${sessionId}\"`;
  }

  if (index !== undefined) {
    if (filter.length > 0) {
      filter += ', ';
    }
    filter += `index: ${index}`;
  }

  if (filter.length > 0) {
    filter += ', ';
  }
  filter += `sendProgressUpdates: ${sendProgressUpdates}`;

  return await sendQueryAndGetResponse(client, '9', {
    id: '9',
    type: 'start',
    payload: {
      query: `subscription {
              wallet (${filter}) {
                __typename
                ... on ViewingUpdate {
                  index update {
                    __typename
                    ... on MerkleTreeCollapsedUpdate { update protocolVersion }
                    ... on RelevantTransaction { transaction { hash merkleTreeRoot protocolVersion } }
                  }
                }
              }
            }`,
    },
  });
}

export async function verifyWalletSubscription(indexerPort: number, walletNumber?: number) {
  const timeout = (ms: number) =>
    new Promise((_resolve, reject) =>
      setTimeout(() => {
        reject(new Error('Timeout error during wallet subscription verification'));
      }, ms),
    );
  const handleWalletConnection = async (wallet: Wallet) => {
    const client = new WebSocket(`http://localhost:${indexerPort}/api/v1/graphql/ws`, ['graphql-ws']);
    await new Promise((resolve) => client.on('open', resolve));
    await initConnection(client);
    const response = await getSessionId(client, wallet.viewingKey);
    const sessionId = response.payload.data.connect as string;
    const subscriptionResponse = await walletSubscription(client, false, sessionId);
    expect(subscriptionResponse.payload.data.wallet.__typename).toEqual('ViewingUpdate');
  };
  const environment: Environment = environments.compose;
  const wallets = walletNumber === undefined ? environment.wallets : [environment.wallets[walletNumber]];
  const walletPromises = wallets.map((wallet) => handleWalletConnection(wallet));
  await Promise.race([Promise.all(walletPromises), timeout(2000)]);
}
