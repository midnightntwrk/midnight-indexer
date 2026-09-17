// This file is part of midnightntwrk/midnight-indexer
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// GraphQL queries for the SPO (stake pool operator) indexer surface (#1003):
// governance history (dParameterHistory, written by chain-indexer), epoch and
// committee data (written by spo-indexer), and the SPO registration surface
// (spoCount, spoList, spoIdentities, stakePoolOperators, pool-id lookups),
// which is empty on every environment until post-mainnet registration tooling
// exists.

export const GET_D_PARAMETER_HISTORY = `
query DParameterHistory {
  dParameterHistory {
    blockHeight
    blockHash
    timestamp
    numPermissionedCandidates
    numRegisteredCandidates
  }
}`;

export const GET_CURRENT_EPOCH_INFO = `
query CurrentEpochInfo {
  currentEpochInfo {
    epochNo
    durationSeconds
    elapsedSeconds
  }
}`;

export const GET_COMMITTEE = `
query Committee($EPOCH: Int!) {
  committee(epoch: $EPOCH) {
    epochNo
    position
    sidechainPubkeyHex
    expectedSlots
    auraPubkeyHex
    poolIdHex
    spoSkHex
  }
}`;

export const GET_SPO_COUNT = `
query SpoCount {
  spoCount
}`;

export const GET_SPO_LIST = `
query SpoList($LIMIT: Int, $OFFSET: Int, $SEARCH: String) {
  spoList(limit: $LIMIT, offset: $OFFSET, search: $SEARCH) {
    poolIdHex
    validatorClass
    sidechainPubkeyHex
    auraPubkeyHex
    name
    ticker
    homepageUrl
    logoUrl
  }
}`;

export const GET_SPO_IDENTITIES = `
query SpoIdentities($LIMIT: Int, $OFFSET: Int) {
  spoIdentities(limit: $LIMIT, offset: $OFFSET) {
    poolIdHex
    mainchainPubkeyHex
    sidechainPubkeyHex
    auraPubkeyHex
    validatorClass
  }
}`;

export const GET_STAKE_POOL_OPERATORS = `
query StakePoolOperators($LIMIT: Int) {
  stakePoolOperators(limit: $LIMIT)
}`;

export const GET_SPO_BY_POOL_ID = `
query SpoByPoolId($POOL_ID_HEX: String!) {
  spoByPoolId(poolIdHex: $POOL_ID_HEX) {
    poolIdHex
    validatorClass
    sidechainPubkeyHex
    auraPubkeyHex
    name
    ticker
    homepageUrl
    logoUrl
  }
}`;

export const GET_SPO_IDENTITY_BY_POOL_ID = `
query SpoIdentityByPoolId($POOL_ID_HEX: String!) {
  spoIdentityByPoolId(poolIdHex: $POOL_ID_HEX) {
    poolIdHex
    mainchainPubkeyHex
    sidechainPubkeyHex
    auraPubkeyHex
    validatorClass
  }
}`;

export const GET_REGISTERED_TOTALS_SERIES = `
query RegisteredTotalsSeries($FROM_EPOCH: Int!, $TO_EPOCH: Int!) {
  registeredTotalsSeries(fromEpoch: $FROM_EPOCH, toEpoch: $TO_EPOCH) {
    epochNo
    totalRegistered
    newlyRegistered
  }
}`;
