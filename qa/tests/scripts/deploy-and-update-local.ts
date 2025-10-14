// This file is part of midnightntwrk/midnight-indexer
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

/**
 * Deploys a contract and updates local.json with deployment details
 *
 * Usage: TARGET_ENV=undeployed npx tsx scripts/deploy-and-update-local.ts
 */

import { ToolkitWrapper } from '../utils/toolkit/toolkit-wrapper.js';
import { LocalDataUtils } from '../utils/local-data-utils.js';

async function main() {
  console.log('='.repeat(80));
  console.log('CONTRACT DEPLOYMENT AND LOCAL DATA UPDATE');
  console.log('='.repeat(80));

  const toolkit = new ToolkitWrapper({});
  const localDataUtils = new LocalDataUtils();

  try {
    console.log('\n1. Starting toolkit...');
    await toolkit.start();

    console.log('2. Deploying contract...');
    const deployResult = await toolkit.deployContract();

    console.log('\n✅ Contract deployed successfully!');
    console.log(`   Address (Untagged):    ${deployResult.addressUntagged}`);
    console.log(`   Address (Tagged):      ${deployResult.addressTagged}`);
    console.log(`   Contract Address:      ${deployResult.contractAddress}`);
    console.log(`   Coin Public:           ${deployResult.coinPublic}`);

    console.log('\n3. Updating local.json with deployment data from indexer...');
    await localDataUtils.writeDeploymentData(deployResult);

    console.log('\nThe test will use this deployed contract to make contract calls.');
    console.log('Run the e2e tests with:');
    console.log('  TARGET_ENV=undeployed yarn test mn-toolkit-contract-call');
    console.log('='.repeat(80));
  } catch (error) {
    console.error('\n❌ Error during deployment:', error);
    throw error;
  } finally {
    await toolkit.stop();
  }
}

if (require.main === module) {
  main()
    .then(() => {
      console.log('\n✅ Script completed successfully!');
      process.exit(0);
    })
    .catch((error) => {
      console.error('\n❌ Script failed:', error);
      process.exit(1);
    });
}

export { main };
