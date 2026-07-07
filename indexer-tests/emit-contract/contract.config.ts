import { CompiledContract, ContractExecutable, type Contract } from '@midnight-ntwrk/compact-js/effect';
import { Contract as C_ } from './managed/emitcounter/contract/index.js';

type PrivateState = {
  count: number;
};

type EmitCounterContract = C_<PrivateState>;
const EmitCounterContract = C_;

const witnesses: Contract.Contract.Witnesses<EmitCounterContract> = {
  private_increment: ({ privateState }, amount) => [{ count: privateState.count + Number(amount) }, []]
};

const createInitialPrivateState: () => PrivateState = () => ({ count: 0 });

export default {
  contractExecutable: CompiledContract.make<EmitCounterContract>('EmitCounterContract', EmitCounterContract).pipe(
    CompiledContract.withWitnesses(witnesses),
    CompiledContract.withCompiledFileAssets('./managed/emitcounter'),
    ContractExecutable.make
  ),
  createInitialPrivateState,
  config: {
    keys: {
      coinPublic: '1bd4f827be97ff013c4a702e4b08f30ec378728a54670cf7cc92cb9b1a14eff6',
    },
    network: 'undeployed'
  }
}
