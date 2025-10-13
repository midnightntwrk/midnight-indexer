#!/usr/bin/env ts-node
// This file is part of midnightntwrk/midnight-indexer
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

/**
 * Deploys a contract and updates local.json with deployment details
 *
 * Usage: TARGET_ENV=undeployed npx tsx scripts/deploy-and-update-local.ts
 */

import { ToolkitWrapper } from '../utils/toolkit/toolkit-wrapper.js';
import { IndexerHttpClient } from '../utils/indexer/http-client.js';
import { readFileSync, writeFileSync } from 'fs';
import { join } from 'path';

async function retry<T>(
  fn: () => Promise<T>,
  condition: (result: T) => boolean,
  maxAttempts = 30,
  delayMs = 2000,
): Promise<T> {
  for (let attempt = 1; attempt <= maxAttempts; attempt++) {
    const result = await fn();
    if (condition(result)) {
      return result;
    }
    if (attempt < maxAttempts) {
      console.log(`   Attempt ${attempt}/${maxAttempts} - Waiting for indexer to catch up...`);
      await new Promise((resolve) => setTimeout(resolve, delayMs));
    }
  }
  throw new Error(`Condition not met after ${maxAttempts} attempts`);
}

async function main() {
  console.log('='.repeat(80));
  console.log('CONTRACT DEPLOYMENT AND LOCAL DATA UPDATE');
  console.log('='.repeat(80));

  const toolkit = new ToolkitWrapper({});
  const indexerClient = new IndexerHttpClient();

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

    console.log('\n3. Querying indexer for deployment details...');
    let deployTxHash = '';
    let deployBlockHash = '';

    try {
      const contractActionResponse = await retry(
        () => indexerClient.getContractAction(deployResult.contractAddress),
        (response) => {
          return (
            response?.data?.contractAction?.__typename === 'ContractDeploy' &&
            response?.data?.contractAction?.transaction?.hash !== undefined
          );
        },
        30,
        2000,
      );

      const contractAction = contractActionResponse.data?.contractAction;
      if (contractAction?.__typename === 'ContractDeploy') {
        deployTxHash = contractAction.transaction?.hash || '';
        deployBlockHash = contractAction.transaction?.block?.hash || '';
        console.log(`   ✅ Found deployment in indexer!`);
        console.log(`   Deploy Tx Hash:        ${deployTxHash}`);
        console.log(`   Deploy Block Hash:     ${deployBlockHash}`);
      }
    } catch (error) {
      console.warn(`   ⚠️  Could not fetch deployment details from indexer: ${error}`);
      console.warn(`   The deploy-tx-hash and deploy-block-hash will be empty.`);
    }

    const localData = {
      'contract-address-untagged': deployResult.addressUntagged,
      'contract-address-tagged': deployResult.addressTagged,
      'contract-address': deployResult.contractAddress,
      'coin-public': deployResult.coinPublic,
      'deploy-tx-hash': deployTxHash,
      'deploy-block-hash': deployBlockHash,
    };

    console.log('\n4. Updating local.json...');
    const localJsonPath = join(__dirname, '../data/static/undeployed/local.json');
    
    writeFileSync(localJsonPath, JSON.stringify(localData, null, 2) + '\n', 'utf-8');

    console.log('\n' + '='.repeat(80));
    console.log('✅ SUCCESS - local.json updated with deployment data:');
    console.log('='.repeat(80));
    console.log(JSON.stringify(localData, null, 2));
    console.log('='.repeat(80));
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
