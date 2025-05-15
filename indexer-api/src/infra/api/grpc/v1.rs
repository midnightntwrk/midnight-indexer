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

tonic::include_proto!("midnight_indexer.v1");

use crate::{
    domain::{self, Storage},
    infra::api::grpc::{
        FILE_DESCRIPTOR_SET,
        v1::transaction_service_server::{TransactionService, TransactionServiceServer},
    },
};
use futures::StreamExt;
use indexer_common::error::StdErrorExt;
use log::error;
use std::{num::NonZeroU32, pin::pin};
use tokio::{sync::mpsc, task};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tonic_reflection::server::v1::{ServerReflection, ServerReflectionServer};

// TODO: Make configurable!
const BATCH_SIZE: NonZeroU32 = NonZeroU32::new(100).unwrap();

pub fn transaction_service<S>(storage: S) -> TransactionServiceServer<TransactionServiceImpl<S>>
where
    S: Storage,
{
    TransactionServiceServer::new(TransactionServiceImpl { storage })
}

pub fn reflection_service() -> ServerReflectionServer<impl ServerReflection> {
    tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
        .build_v1()
        .expect("v1 reflection can be built")
}

pub struct TransactionServiceImpl<S> {
    storage: S,
}

#[tonic::async_trait]
impl<S> TransactionService for TransactionServiceImpl<S>
where
    S: Storage,
{
    // TODO: Once the Rust async story is better and tonic no longer uses async_trait with a
    // requirement for a 'static stream, revisit this indicretion!
    type TransactionsStream = ReceiverStream<Result<TransactionsResponse, Status>>;

    async fn transactions(
        &self,
        request: Request<TransactionsRequest>,
    ) -> Result<Response<Self::TransactionsStream>, Status> {
        let id = request.into_inner().id;

        let (sender, receiver) = mpsc::channel(42);

        task::spawn({
            let storage = self.storage.to_owned();

            async move {
                let mut transactions = pin!(storage.get_transactions(id, BATCH_SIZE));

                while let Some(transaction) = transactions.next().await {
                    let transaction = transaction
                        .map_err(|error| {
                            error!(error = error.as_chain(); "cannot get next transaction");
                            Status::internal("internal error")
                        })
                        .map(|transaction| {
                            let domain::Transaction {
                                id,
                                hash,
                                protocol_version,
                                apply_stage,
                                raw,
                                start_index,
                                end_index,
                                ..
                            } = transaction;

                            TransactionsResponse {
                                id,
                                hash: hash.into(),
                                protocol_version: protocol_version.into(),
                                apply_stage: apply_stage.into(),
                                raw: raw.0,
                                start_index,
                                end_index,
                            }
                        });

                    if sender.send(transaction).await.is_err() {
                        // Receiver has dropped.
                        break;
                    }
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(receiver)))
    }
}
