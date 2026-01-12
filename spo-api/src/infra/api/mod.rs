// This file is part of midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

pub mod v1;

use crate::domain::Api;
use async_graphql::Context;
use axum::{
    Router,
    extract::{FromRef, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use indexer_common::{domain::NetworkId, infra::pool::postgres::PostgresPool};
use log::info;
use serde::Deserialize;
use std::{
    io,
    net::IpAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use thiserror::Error;
use tokio::signal::unix::{SignalKind, signal};
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, limit::RequestBodyLimitLayer};

#[derive(Clone)]
pub struct Db(pub PostgresPool);

#[derive(Clone)]
pub struct AppState {
    pub caught_up: Arc<AtomicBool>,
    pub db: Option<Db>,
}

impl FromRef<AppState> for Arc<AtomicBool> {
    fn from_ref(s: &AppState) -> Arc<AtomicBool> {
        s.caught_up.clone()
    }
}
impl FromRef<AppState> for Option<Db> {
    fn from_ref(s: &AppState) -> Option<Db> {
        s.db.clone()
    }
}

pub struct AxumApi {
    config: Config,
    db: Option<Db>,
}

impl AxumApi {
    pub fn new(config: Config) -> Self {
        Self { config, db: None }
    }
    pub fn with_db(mut self, db: Db) -> Self {
        self.db = Some(db);
        self
    }
}

impl Api for AxumApi {
    type Error = AxumApiError;

    async fn serve(
        self,
        network_id: NetworkId,
        caught_up: Arc<AtomicBool>,
    ) -> Result<(), Self::Error> {
        let Config {
            address,
            port,
            request_body_limit,
            max_complexity,
            max_depth,
        } = self.config;

        // In the current shape AxumApi doesn't own the pool; we keep readiness simple (caught_up
        // only).
        let app = make_app(
            caught_up,
            self.db,
            network_id,
            max_complexity,
            max_depth,
            request_body_limit as usize,
        );

        let listener = tokio::net::TcpListener::bind((address, port))
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
}

#[derive(Debug, Error)]
pub enum AxumApiError {
    #[error("cannot bind tcp listener")]
    Bind(#[source] io::Error),
    #[error("cannot serve API")]
    Serve(#[source] io::Error),
}

pub struct Metrics;
impl Default for Metrics {
    fn default() -> Self {
        Self
    }
}

#[allow(clippy::too_many_arguments)]
fn make_app(
    caught_up: Arc<AtomicBool>,
    db: Option<Db>,
    network_id: NetworkId,
    max_complexity: usize,
    max_depth: usize,
    request_body_limit: usize,
) -> Router {
    let app_state = AppState { caught_up, db };
    let v1_app = v1::make_app(network_id, max_complexity, max_depth, app_state.db.clone())
        .with_state(app_state.clone());

    Router::new()
        .route("/ready", get(ready))
        .nest("/api/v1", v1_app)
        .with_state(app_state)
        .layer(
            ServiceBuilder::new()
                .layer(RequestBodyLimitLayer::new(request_body_limit))
                .layer(CorsLayer::permissive()),
        )
}

async fn ready(
    State(caught_up): State<Arc<AtomicBool>>,
    State(db): State<Option<Db>>,
) -> impl IntoResponse {
    if !caught_up.load(Ordering::Acquire) {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "indexer has not yet caught up with the node",
        )
            .into_response()
    } else {
        // If a DB is provided, try a lightweight ping.
        if let Some(Db(pool)) = db {
            if let Err(_error) = sqlx::query_scalar::<_, i32>("SELECT 1")
                .fetch_one(&*pool)
                .await
            {
                return (StatusCode::SERVICE_UNAVAILABLE, "database not ready").into_response();
            }
        }
        StatusCode::OK.into_response()
    }
}

// Removed custom 400->413 transform; default behavior is acceptable for MVP.

async fn shutdown_signal() {
    signal(SignalKind::terminate())
        .expect("install SIGTERM handler")
        .recv()
        .await;
}

pub trait ContextExt {
    fn get_network_id(&self) -> NetworkId;
    fn get_metrics(&self) -> &Metrics;
}
impl ContextExt for Context<'_> {
    fn get_network_id(&self) -> NetworkId {
        self.data::<NetworkId>()
            .cloned()
            .expect("NetworkId is stored in Context")
    }
    fn get_metrics(&self) -> &Metrics {
        self.data::<Metrics>()
            .expect("Metrics is stored in Context")
    }
}
