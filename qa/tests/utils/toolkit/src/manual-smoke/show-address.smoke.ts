// Minimal manual smoke: prints shielded & unshielded addresses for a known seed.

import { showAddress } from '../show-address.ts';

(async () => {
  const seed = '0000000000000000000000000000000000000000000000000000000000000001';

  const shielded = await showAddress({
    chain: 'undeployed',
    addressType: 'shielded',
    seed,
  });
  console.log('Shielded address:', shielded);

  const unshielded = await showAddress({
    chain: 'undeployed',
    addressType: 'unshielded',
    seed,
  });
  console.log('Unshielded address:', unshielded);
})();
