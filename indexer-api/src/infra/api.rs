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

use crate::domain::{Api, Storage, ZswapStateCache};
use anyhow::Context as _;
use async_graphql::Context;
use axum::{
    Router,
    body::Body,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use fastrace_axum::FastraceLayer;
use indexer_common::domain::{LedgerStateStorage, NetworkId, Subscriber};
use log::{error, info, warn};
use serde::Deserialize;
use std::{
    convert::Infallible,
    error::Error as StdError,
    fmt::Display,
    io,
    net::IpAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use thiserror::Error;
use tokio::{
    net::TcpListener,
    signal::unix::{SignalKind, signal},
};
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, limit::RequestBodyLimitLayer};

/// Attention: This could change if the used libraries change!
/// See https://docs.rs/http-body-util/0.1.2/src/http_body_util/limited.rs.html#93.
const LENGTH_LIMIT_EXCEEDED_BODY: &[u8] =
    b"Io(Custom { kind: Other, error: \"length limit exceeded\" })";

pub struct AxumApi<S, Z, B> {
    config: Config,
    storage: S,
    ledger_state_storage: Z,
    subscriber: B,
}

impl<S, Z, B> AxumApi<S, Z, B> {
    pub fn new(config: Config, storage: S, ledger_state_storage: Z, subscriber: B) -> Self {
        Self {
            config,
            storage,
            ledger_state_storage,
            subscriber,
        }
    }
}

impl<S, Z, B> Api for AxumApi<S, Z, B>
where
    S: Storage,
    B: Subscriber,
    Z: LedgerStateStorage,
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
            self.ledger_state_storage,
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

#[allow(clippy::too_many_arguments)]
fn make_app<S, Z, B>(
    caught_up: Arc<AtomicBool>,
    network_id: NetworkId,
    storage: S,
    ledger_state_storage: Z,
    subscriber: B,
    max_complexity: usize,
    max_depth: usize,
    request_body_limit: usize,
) -> Router
where
    S: Storage,
    B: Subscriber,
    Z: LedgerStateStorage,
{
    let zswap_state_cache = ZswapStateCache::default();

    let v1_app = v1::make_app(
        network_id,
        zswap_state_cache,
        storage,
        ledger_state_storage,
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
            ServiceBuilder::new().layer(
                ServiceBuilder::new()
                    .layer(FastraceLayer)
                    .layer(RequestBodyLimitLayer::new(request_body_limit))
                    .layer(CorsLayer::permissive())
                    .and_then(transform_lentgh_limit_exceeded),
            ),
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
    fn get_network_id(&self) -> NetworkId;

    fn get_storage<S>(&self) -> &S
    where
        S: Storage;

    fn get_subscriber<B>(&self) -> &B
    where
        B: Subscriber;

    fn get_ledger_state_storage<Z>(&self) -> &Z
    where
        Z: LedgerStateStorage;

    fn get_zswap_state_cache(&self) -> &ZswapStateCache;
}

impl ContextExt for Context<'_> {
    fn get_network_id(&self) -> NetworkId {
        self.data::<NetworkId>()
            .copied()
            .expect("NetworkId is stored in Context")
    }

    fn get_storage<S>(&self) -> &S
    where
        S: Storage,
    {
        self.data::<S>().expect("Storage is stored in Context")
    }

    fn get_subscriber<B>(&self) -> &B
    where
        B: Subscriber,
    {
        self.data::<B>().expect("Subscriber is stored in Context")
    }

    fn get_ledger_state_storage<Z>(&self) -> &Z
    where
        Z: LedgerStateStorage,
    {
        self.data::<Z>()
            .expect("ZswapStateStorage is stored in Context")
    }

    fn get_zswap_state_cache(&self) -> &ZswapStateCache {
        self.data::<ZswapStateCache>()
            .expect("ZswapStateCache is stored in Context")
    }
}

trait ResultExt<T, E>
where
    E: StdError + Send + Sync + 'static,
{
    /// In case of an `Err`, log an error with the given context and return just "Internal Error"
    /// without further details.
    fn internal<C>(self, context: C) -> async_graphql::Result<T>
    where
        C: Display + Send + Sync + 'static;
}

impl<T, E> ResultExt<T, E> for Result<T, E>
where
    E: StdError + Send + Sync + 'static,
{
    fn internal<C>(self, context: C) -> async_graphql::Result<T>
    where
        C: Display + Send + Sync + 'static,
    {
        self.context(context)
            .inspect_err(|error| error!(error = format!("{error:#}"); "API error"))
            .map_err(|_| async_graphql::Error::new("Internal Error"))
    }
}

trait OptionExt<T> {
    /// In case of `None`, log an error with the given context and return just "Internal Error"
    /// without further details.
    fn internal<C>(self, context: C) -> async_graphql::Result<T>
    where
        C: Display + Send + Sync + 'static;
}

impl<T> OptionExt<T> for Option<T> {
    fn internal<C>(self, context: C) -> async_graphql::Result<T>
    where
        C: Display + Send + Sync + 'static,
    {
        self.context(context)
            .inspect_err(|error| error!(error = format!("{error:#}"); "API error"))
            .map_err(|_| async_graphql::Error::new("Internal Error"))
    }
}
