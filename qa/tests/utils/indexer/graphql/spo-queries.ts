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

export const REGISTERED_TOTALS_BODY_FRAGMENT = `
  epochNo
  totalRegistered
  newlyRegistered
`;

export const GET_REGISTERED_TOTALS_SERIES = `
query GetRegisteredTotalsSeries($FROM_EPOCH: Int!, $TO_EPOCH: Int!) {
  registeredTotalsSeries(fromEpoch: $FROM_EPOCH, toEpoch: $TO_EPOCH) {
    ${REGISTERED_TOTALS_BODY_FRAGMENT}
  }
}`;

export const REGISTERED_STAT_BODY_FRAGMENT = `
  epochNo
  federatedValidCount
  federatedInvalidCount
  registeredValidCount
  registeredInvalidCount
  dparam
`;

export const GET_REGISTERED_SPO_SERIES = `
query GetRegisteredSpoSeries($FROM_EPOCH: Int!, $TO_EPOCH: Int!) {
  registeredSpoSeries(fromEpoch: $FROM_EPOCH, toEpoch: $TO_EPOCH) {
    ${REGISTERED_STAT_BODY_FRAGMENT}
  }
}`;

export const PRESENCE_EVENT_BODY_FRAGMENT = `
  epochNo
  idKey
  source
  status
`;

export const GET_REGISTERED_PRESENCE = `
query GetRegisteredPresence($FROM_EPOCH: Int!, $TO_EPOCH: Int!) {
  registeredPresence(fromEpoch: $FROM_EPOCH, toEpoch: $TO_EPOCH) {
    ${PRESENCE_EVENT_BODY_FRAGMENT}
  }
}`;
