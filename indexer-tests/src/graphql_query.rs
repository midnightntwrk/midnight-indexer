// This file is part of midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use graphql_client::GraphQLQuery;
use indexer_api::{domain::HexEncoded, infra::api::v1::UnshieldedAddress};

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "../indexer-api/graphql/schema-v1.graphql",
    query_path = "./queries.graphql",
    response_derives = "Debug,Clone,PartialEq"
)]
pub struct TransactionByHash;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "../indexer-api/graphql/schema-v1.graphql",
    query_path = "./queries.graphql",
    response_derives = "Debug,Clone,PartialEq"
)]
pub struct TransactionsByAddress;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "../indexer-api/graphql/schema-v1.graphql",
    query_path = "./queries.graphql",
    response_derives = "Debug,Clone,PartialEq"
)]
pub struct UnshieldedUtxos;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "../indexer-api/graphql/schema-v1.graphql",
    query_path = "./queries.graphql",
    response_derives = "Debug,Clone,PartialEq"
)]
pub struct UnshieldedUtxosSubscription;
