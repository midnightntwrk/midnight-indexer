export interface Block {
  hash: string;
  height: number;
  timestamp: string;
  parent: Block;
  transactions: Transaction[];
}

export interface UnshieldedUtxo {
  owner: string;
  intentHash: string;
  value: string;
  tokenType: string;
  outputIndex: number;
  createdAtTransaction: Transaction;
  spentAtTransaction: Transaction;
}

export type TransactionResult = {
  status: TransactionResultStatus;
  segments: Segment;
};

export enum TransactionResultStatus {
  SUCCESS = 'SUCCESS',
  PARTIAL_SUCCESS = 'PARTIAL_SUCCESS',
  FAILURE = 'FAILURE',
}

export interface Segment {
  id: number;
  success: boolean;
}

export interface TransactionFees {
  paidFees: string;
  estimatedFees: string;
}

// Base Transaction interface (common to both RegularTransaction and SystemTransaction)
export interface Transaction {
  __typename: 'RegularTransaction' | 'SystemTransaction';
  id?: number;
  hash?: string;
  protocolVersion?: number;
  raw?: string;
  block?: Block;
  transactionResult?: TransactionResult;
  fees?: TransactionFees;
  merkleTreeRoot?: string;
  contractActions?: ContractAction[];
  unshieldedCreatedOutputs?: UnshieldedUtxo[];
  unshieldedSpentOutputs?: UnshieldedUtxo[];
  zswapLedgerEvents?: ZswapLedgerEvent[];
  dustLedgerEvents?: DustLedgerEvent[];
}

// RegularTransaction interface (includes additional fields)
export interface RegularTransaction extends Transaction {
  merkleTreeRoot?: string;
  identifiers?: string[];
  startIndex?: number;
  endIndex?: number;
  fees?: TransactionFees;
  transactionResult?: TransactionResult;
}

// SystemTransaction interface (only base fields)
export interface SystemTransaction extends Transaction {
  // No additional fields beyond the base Transaction interface
}

export type ContractAction = ContractDeploy | ContractCall | ContractUpdate;

export interface ContractDeploy {
  __typename: 'ContractDeploy';
  address: string;
  state: string;
  chainState: string;
  transaction: Transaction;
  unshieldedBalances: ContractBalance[];
}

export interface ContractCall {
  __typename: 'ContractCall';
  address: string;
  state: string;
  chainState: string;
  transaction: Transaction;
  entryPoint: string;
  deploy: ContractDeploy;
  unshieldedBalances: ContractBalance[];
}

export interface ContractUpdate {
  __typename: 'ContractUpdate';
  address: string;
  state: string;
  chainState: string;
  transaction: Transaction;
  unshieldedBalances: ContractBalance[];
}

export interface ContractBalance {
  tokenType: string;
  amount: string;
}

export interface ZswapLedgerEvent {
  id: number;
  raw: string;
  maxId: number;
}

export interface DustLedgerEvent {
  id: number;
  raw: string;
  maxId: number;
}
