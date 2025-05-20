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

mod v1;

use crate::domain::Storage;
use axum::{
    Router,
    extract::Request,
    http::{StatusCode, header::ACCEPT},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use tonic::{include_file_descriptor_set, service::Routes};

const FILE_DESCRIPTOR_SET: &[u8] = include_file_descriptor_set!("midnight_indexer");

pub fn routes<S>(storage: S) -> Router
where
    S: Storage,
{
    Routes::new(v1::transaction_service(storage))
        .add_service(v1::reflection_service())
        .into_axum_router()
        .layer(middleware::from_fn(require_application_grpc))
}

async fn require_application_grpc(request: Request, next: Next) -> Response {
    match request.headers().get(ACCEPT) {
        Some(accept) if accept == "application/grpc" => next.run(request).await,
        _ => StatusCode::NOT_ACCEPTABLE.into_response(),
    }
}
