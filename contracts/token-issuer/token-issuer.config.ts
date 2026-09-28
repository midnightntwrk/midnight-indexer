import { CompiledContract, ContractExecutable, type Contract } from '@midnight-ntwrk/compact-js/effect';
import { Contract as C_ } from './out/contract/index.js';

/**
 * token-issuer has no witnesses and no private state.
 */
type PrivateState = {};

type TokenIssuerContract = C_<PrivateState>;
const TokenIssuerContract = C_;

const createInitialPrivateState: () => PrivateState = () => ({});

export default {
  contractExecutable: CompiledContract.make<TokenIssuerContract>('TokenIssuerContract', TokenIssuerContract).pipe(
    CompiledContract.withVacantWitnesses,
    CompiledContract.withCompiledFileAssets('./out'),
    ContractExecutable.make
  ),
  createInitialPrivateState,
  config: {
    keys: {
      // Seed: 0000000000000000000000000000000000000000000000000000000000000001
      coinPublic: 'aa0d72bb77ea46f986a800c66d75c4e428a95bd7e1244f1ed059374e6266eb98',
    },
    network: 'undeployed'
  }
}
