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

pub mod v1;
pub mod v2;

use crate::domain::{Api, Storage, ZswapStateCache};
use async_graphql::{scalar, Context};
use axum::{
    body::Body,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use fastrace_axum::FastraceLayer;
use indexer_common::domain::{NetworkId, Subscriber, ZswapStateStorage};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::{
    convert::Infallible,
    io,
    net::IpAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use thiserror::Error;
use tokio::{
    net::TcpListener,
    signal::unix::{signal, SignalKind},
};
use tower::ServiceBuilder;
use tower_http::limit::RequestBodyLimitLayer;

/// Attention: This could change if the used libraries change!
/// See https://docs.rs/http-body-util/0.1.2/src/http_body_util/limited.rs.html#93.
const LENGTH_LIMIT_EXCEEDED_BODY: &[u8] =
    b"Io(Custom { kind: Other, error: \"length limit exceeded\" })";

pub struct AxumApi<S, Z, B> {
    config: Config,
    storage: S,
    zswap_state_storage: Z,
    subscriber: B,
}

impl<S, Z, B> AxumApi<S, Z, B> {
    pub fn new(config: Config, storage: S, zswap_state_storage: Z, subscriber: B) -> Self {
        Self {
            config,
            storage,
            zswap_state_storage,
            subscriber,
        }
    }
}

impl<S, Z, B> Api for AxumApi<S, Z, B>
where
    S: Storage,
    B: Subscriber,
    Z: ZswapStateStorage,
{
    type Error = AxumApiError;

    /// Serve the API.
    async fn serve(self, caught_up: Arc<AtomicBool>) -> Result<(), Self::Error> {
        let Config {
            address,
            port,
            request_body_limit,
            max_complexity,
            max_depth,
            network_id,
        } = self.config;

        let app = make_app(
            caught_up,
            network_id,
            self.storage,
            self.zswap_state_storage,
            self.subscriber,
            max_complexity,
            max_depth,
            request_body_limit as usize,
        );

        let listener = TcpListener::bind((address, port))
            .await
            .map_err(AxumApiError::Bind)?;
        info!(address:?, port; "listening to TCP connections");

        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .map_err(AxumApiError::Serve)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub address: IpAddr,
    pub port: u16,
    #[serde(with = "byte_unit_serde")]
    pub request_body_limit: u64,
    pub max_complexity: usize,
    pub max_depth: usize,
    pub network_id: NetworkId,
}

#[derive(Debug, Error)]
pub enum AxumApiError {
    #[error("cannot bind tcp listener")]
    Bind(#[source] io::Error),

    #[error("cannot serve API")]
    Serve(#[source] io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unit;

scalar!(Unit);

#[allow(clippy::too_many_arguments)]
fn make_app<S, Z, B>(
    caught_up: Arc<AtomicBool>,
    network_id: NetworkId,
    storage: S,
    zswap_state_storage: Z,
    subscriber: B,
    max_complexity: usize,
    max_depth: usize,
    request_body_limit: usize,
) -> Router
where
    S: Storage,
    B: Subscriber,
    Z: ZswapStateStorage,
{
    let zswap_state_cache = ZswapStateCache::default();

    let v1_app = v1::make_app(
        network_id,
        zswap_state_cache,
        storage,
        zswap_state_storage,
        subscriber,
        max_complexity,
        max_depth,
    );

    Router::new()
        .route("/ready", get(ready))
        .route("/health", get(health))
        .nest("/api/v1", v1_app)
        .with_state(caught_up)
        .layer(
            ServiceBuilder::new()
                .layer(ServiceBuilder::new().layer(FastraceLayer))
                .layer(RequestBodyLimitLayer::new(request_body_limit))
                .and_then(transform_lentgh_limit_exceeded),
        )
}

async fn ready(State(caught_up): State<Arc<AtomicBool>>) -> impl IntoResponse {
    if !caught_up.load(Ordering::Acquire) {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "indexer has not yet caught up with the node",
        )
            .into_response()
    } else {
        StatusCode::OK.into_response()
    }
}

// TODO: Remove once clients no longer use it!
async fn health(State(caught_up): State<Arc<AtomicBool>>) -> impl IntoResponse {
    if !caught_up.load(Ordering::Acquire) {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "indexer has not yet caught up with the node; deprecated: use ../ready instead",
        )
    } else {
        (StatusCode::OK, "OK, deprecated: use ../ready instead")
    }
}

/// This is a workaround for async-graphql swallowing `LengthLimitError`s returned by the
/// `RequestBodyLimitLayer` for requests that are too large but do not expose that via the
/// `Content-Length` header which results in responses with status code 400 instead of 413.
async fn transform_lentgh_limit_exceeded(response: Response<Body>) -> Result<Response, Infallible> {
    if response.status() == StatusCode::BAD_REQUEST {
        let (mut head, body) = response.into_parts();

        let Ok(bytes) = axum::body::to_bytes(body, LENGTH_LIMIT_EXCEEDED_BODY.len()).await else {
            warn!("cannot consume response body");
            return Ok(Response::from_parts(head, Body::empty()));
        };

        if &*bytes == LENGTH_LIMIT_EXCEEDED_BODY {
            head.status = StatusCode::PAYLOAD_TOO_LARGE;
            Ok(Response::from_parts(
                head,
                Body::from("length limit exceeded"),
            ))
        } else {
            Ok(Response::from_parts(head, Body::from(bytes)))
        }
    } else {
        Ok::<_, Infallible>(response)
    }
}

async fn shutdown_signal() {
    signal(SignalKind::terminate())
        .expect("install SIGTERM handler")
        .recv()
        .await;
}

trait ContextExt {
    fn get_network_id(&self) -> async_graphql::Result<NetworkId>;

    fn get_storage<S>(&self) -> async_graphql::Result<&S>
    where
        S: Storage;

    fn get_subscriber<B>(&self) -> async_graphql::Result<&B>
    where
        B: Subscriber;

    fn get_zswap_state_storage<Z>(&self) -> async_graphql::Result<&Z>
    where
        Z: ZswapStateStorage;

    fn get_zswap_state_cache(&self) -> async_graphql::Result<&ZswapStateCache>;
}

impl ContextExt for Context<'_> {
    fn get_network_id(&self) -> async_graphql::Result<NetworkId> {
        self.data::<NetworkId>()
            .copied()
            .map_err(|_| async_graphql::Error::new("cannot get NetworkId from context"))
    }

    fn get_storage<S>(&self) -> async_graphql::Result<&S>
    where
        S: Storage,
    {
        self.data::<S>()
            .map_err(|_| async_graphql::Error::new("cannot get Storage from context"))
    }

    fn get_subscriber<B>(&self) -> async_graphql::Result<&B>
    where
        B: Subscriber,
    {
        self.data::<B>()
            .map_err(|_| async_graphql::Error::new("cannot get Subscriber from context"))
    }

    fn get_zswap_state_storage<Z>(&self) -> async_graphql::Result<&Z>
    where
        Z: ZswapStateStorage,
    {
        self.data::<Z>()
            .map_err(|_| async_graphql::Error::new("cannot get ZswapStateStorage from context"))
    }

    fn get_zswap_state_cache(&self) -> async_graphql::Result<&ZswapStateCache> {
        self.data::<ZswapStateCache>()
            .map_err(|_| async_graphql::Error::new("cannot get ZSwapStateCache from context"))
    }
}
