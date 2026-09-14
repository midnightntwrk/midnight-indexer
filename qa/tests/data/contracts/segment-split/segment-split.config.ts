import { CompiledContract, ContractExecutable } from '@midnight-ntwrk/compact-js/effect';
import { Contract as C_ } from './managed/contract/index.js';

type PrivateState = {};
type SegmentSplitContract = C_<PrivateState>;
const SegmentSplitContract = C_;

const createInitialPrivateState: () => PrivateState = () => ({});

export default {
  contractExecutable: CompiledContract.make<SegmentSplitContract>(
    'SegmentSplitContract',
    SegmentSplitContract,
  ).pipe(
    CompiledContract.withVacantWitnesses,
    CompiledContract.withCompiledFileAssets('./managed'),
    ContractExecutable.make,
  ),
  createInitialPrivateState,
  config: {
    keys: {
      // Seed 0000...0001
      coinPublic: '1bd4f827be97ff013c4a702e4b08f30ec378728a54670cf7cc92cb9b1a14eff6',
    },
    network: 'undeployed',
  },
};
