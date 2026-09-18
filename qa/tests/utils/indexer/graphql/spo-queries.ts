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

// GraphQL queries for the SPO (stake pool operator) indexer surface (#1003).
// Exercised by two suites split along data reality:
//   - spo-queries.test.ts: governance history (dParameterHistory,
//     termsAndConditionsHistory, written by chain-indexer), epoch and committee
//     data and the committee-derived registration series (written by
//     spo-indexer), all live on permissioned environments.
//   - spo-registration-queries.test.ts: the registration, pool metadata,
//     performance and stake surface, which is empty on every environment until
//     post-mainnet registration tooling exists.

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

export const GET_TERMS_AND_CONDITIONS_HISTORY = `
query TermsAndConditionsHistory {
  termsAndConditionsHistory {
    blockHeight
    blockHash
    timestamp
    hash
    url
  }
}`;

const POOL_METADATA_FIELDS = `
    poolIdHex
    hexId
    name
    ticker
    homepageUrl
    logoUrl`;

export const GET_POOL_METADATA = `
query PoolMetadata($POOL_ID_HEX: String!) {
  poolMetadata(poolIdHex: $POOL_ID_HEX) {${POOL_METADATA_FIELDS}
  }
}`;

export const GET_POOL_METADATA_LIST = `
query PoolMetadataList($LIMIT: Int, $OFFSET: Int, $WITH_NAME_ONLY: Boolean) {
  poolMetadataList(limit: $LIMIT, offset: $OFFSET, withNameOnly: $WITH_NAME_ONLY) {${POOL_METADATA_FIELDS}
  }
}`;

const EPOCH_PERF_FIELDS = `
    epochNo
    spoSkHex
    produced
    expected
    identityLabel
    stakeSnapshot
    poolIdHex
    validatorClass`;

export const GET_SPO_COMPOSITE_BY_POOL_ID = `
query SpoCompositeByPoolId($POOL_ID_HEX: String!) {
  spoCompositeByPoolId(poolIdHex: $POOL_ID_HEX) {
    identity {
      poolIdHex
      mainchainPubkeyHex
      sidechainPubkeyHex
      auraPubkeyHex
      validatorClass
    }
    metadata {${POOL_METADATA_FIELDS}
    }
    performance {${EPOCH_PERF_FIELDS}
    }
  }
}`;

export const GET_SPO_PERFORMANCE_LATEST = `
query SpoPerformanceLatest($LIMIT: Int, $OFFSET: Int) {
  spoPerformanceLatest(limit: $LIMIT, offset: $OFFSET) {${EPOCH_PERF_FIELDS}
  }
}`;

export const GET_SPO_PERFORMANCE_BY_SPO_SK = `
query SpoPerformanceBySpoSk($SPO_SK_HEX: String!, $LIMIT: Int, $OFFSET: Int) {
  spoPerformanceBySpoSk(spoSkHex: $SPO_SK_HEX, limit: $LIMIT, offset: $OFFSET) {${EPOCH_PERF_FIELDS}
  }
}`;

export const GET_EPOCH_PERFORMANCE = `
query EpochPerformance($EPOCH: Int!, $LIMIT: Int, $OFFSET: Int) {
  epochPerformance(epoch: $EPOCH, limit: $LIMIT, offset: $OFFSET) {${EPOCH_PERF_FIELDS}
  }
}`;

export const GET_EPOCH_UTILIZATION = `
query EpochUtilization($EPOCH: Int!) {
  epochUtilization(epoch: $EPOCH)
}`;

export const GET_REGISTERED_SPO_SERIES = `
query RegisteredSpoSeries($FROM_EPOCH: Int!, $TO_EPOCH: Int!) {
  registeredSpoSeries(fromEpoch: $FROM_EPOCH, toEpoch: $TO_EPOCH) {
    epochNo
    federatedValidCount
    federatedInvalidCount
    registeredValidCount
    registeredInvalidCount
    dparam
  }
}`;

export const GET_REGISTERED_PRESENCE = `
query RegisteredPresence($FROM_EPOCH: Int!, $TO_EPOCH: Int!) {
  registeredPresence(fromEpoch: $FROM_EPOCH, toEpoch: $TO_EPOCH) {
    epochNo
    idKey
    source
    status
  }
}`;

export const GET_REGISTERED_FIRST_VALID_EPOCHS = `
query RegisteredFirstValidEpochs($UPTO_EPOCH: Int) {
  registeredFirstValidEpochs(uptoEpoch: $UPTO_EPOCH) {
    idKey
    firstValidEpoch
  }
}`;

export const GET_STAKE_DISTRIBUTION = `
query StakeDistribution($LIMIT: Int, $OFFSET: Int, $SEARCH: String, $ORDER_BY_STAKE_DESC: Boolean) {
  stakeDistribution(limit: $LIMIT, offset: $OFFSET, search: $SEARCH, orderByStakeDesc: $ORDER_BY_STAKE_DESC) {
    poolIdHex
    name
    ticker
    homepageUrl
    logoUrl
    liveStake
    activeStake
    liveDelegators
    liveSaturation
    declaredPledge
    livePledge
    stakeShare
  }
}`;
