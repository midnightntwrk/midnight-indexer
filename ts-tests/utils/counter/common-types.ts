import { type FoundContract } from '@midnight-ntwrk/midnight-js-contracts';
import { type MidnightProviders } from '@midnight-ntwrk/midnight-js-types';
import { type Contract, type Witnesses } from './contract';

export type CounterPrivateState = Record<string, never>;

export const witnesses = {};

export interface PrivateStates {
  counterPrivateState: CounterPrivateState;
}

export type CounterProviders = MidnightProviders<'increment', PrivateStates>;

export type CounterContract = Contract<CounterPrivateState, Witnesses<CounterPrivateState>>;

export type DeployedCounterContract = FoundContract<CounterPrivateState, CounterContract>;
