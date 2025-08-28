// This file is part of midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// PM-18678: Long-term monitoring script for THE ISSUE™ investigation
// This script creates multiple wallet subscriptions and monitors for the hanging issue

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::Parser;
use futures::{SinkExt, StreamExt};
use log::{error, info, warn};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::{interval, sleep};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// GraphQL API endpoints (comma-separated for multiple replicas)
    #[arg(long, default_value = "http://localhost:8080,http://localhost:8081,http://localhost:8082")]
    api_endpoints: String,

    /// PostgreSQL connection URL
    #[arg(long, default_value = "postgres://indexer:postgres@localhost:5432/indexer")]
    database_url: String,

    /// Number of wallets to monitor
    #[arg(long, default_value_t = 30)]
    wallet_count: usize,

    /// Check interval in seconds
    #[arg(long, default_value_t = 60)]
    check_interval: u64,

    /// Database monitoring interval in seconds
    #[arg(long, default_value_t = 120)]
    db_check_interval: u64,

    /// Network ID (undeployed, dev, test, mainnet)
    #[arg(long, default_value = "undeployed")]
    network_id: String,
}

#[derive(Debug, Clone)]
struct Wallet {
    viewing_key: String,
    session_id: String,
    replica_endpoint: String,
    created_at: DateTime<Utc>,
    last_viewing_update: Option<DateTime<Utc>>,
    last_progress_update: Option<DateTime<Utc>>,
    viewing_update_count: u64,
    progress_update_count: u64,
    consecutive_empty_queries: u64,
    highest_index: u64,
    highest_relevant_index: u64,
}

#[derive(Debug)]
struct MonitoringState {
    wallets: Arc<RwLock<HashMap<String, Wallet>>>,
    issue_detected: Arc<RwLock<HashMap<String, DateTime<Utc>>>>, // session_id -> first detection time
    start_time: Instant,
    db_pool: PgPool,
}

#[derive(Debug, Serialize, Deserialize)]
struct GraphQLRequest {
    #[serde(rename = "type")]
    msg_type: String,
    id: Option<String>,
    payload: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct GraphQLResponse {
    #[serde(rename = "type")]
    msg_type: String,
    id: Option<String>,
    payload: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ConnectResponse {
    data: Option<ConnectData>,
    errors: Option<Vec<GraphQLError>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ConnectData {
    connect: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct GraphQLError {
    message: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SubscriptionData {
    data: Option<ShieldedTransactionsData>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ShieldedTransactionsData {
    #[serde(rename = "shieldedTransactions")]
    shielded_transactions: ShieldedTransactionsEvent,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "__typename")]
enum ShieldedTransactionsEvent {
    ViewingUpdate {
        index: u64,
        update: serde_json::Value,
    },
    ShieldedTransactionsProgress {
        #[serde(rename = "highestIndex")]
        highest_index: u64,
        #[serde(rename = "highestRelevantIndex")]
        highest_relevant_index: u64,
        #[serde(rename = "highestRelevantWalletIndex")]
        highest_relevant_wallet_index: u64,
    },
}

impl MonitoringState {
    async fn new(database_url: &str) -> Result<Self> {
        let db_pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await
            .context("Failed to connect to database")?;

        Ok(Self {
            wallets: Arc::new(RwLock::new(HashMap::new())),
            issue_detected: Arc::new(RwLock::new(HashMap::new())),
            start_time: Instant::now(),
            db_pool,
        })
    }

    fn get_test_viewing_key(&self, index: usize, network_id: &str) -> String {
        // Use the same test viewing key for all wallets in this network
        // This is a known valid key from the tests
        match network_id {
            "undeployed" => "mn_shield-esk_undeployed1qvqpljf0wrewfdr5k6scfmqtertc4gvu8s2nhkpg8yrmx6n6v4t0evgrqyqw7".to_string(),
            "dev" => "mn_shield-esk_dev1qvqpljf0wrewfdr5k6scfmqtertc4gvu8s2nhkpg8yrmx6n6v4t0evgc05kh2".to_string(),
            "test" => "mn_shield-esk_test1qvqpljf0wrewfdr5k6scfmqtertc4gvu8s2nhkpg8yrmx6n6v4t0evgwk3tj3".to_string(),
            _ => {
                // For mainnet or unknown, generate a unique key per wallet
                // Note: These won't be valid for real use but will create unique sessions
                format!("mn_shield-esk_undeployed1qvqpljf0wrewfdr5k6scfmqtertc4gvu8s2nhkpg8yrmx6n6v4t0evg{:07x}", index)
            }
        }
    }

    async fn create_wallet(&self, endpoint: &str, network_id: &str, index: usize) -> Result<Wallet> {
        let client = Client::new();
        
        // Use a test viewing key
        let viewing_key = self.get_test_viewing_key(index, network_id);
        
        // Connect mutation
        let connect_mutation = r#"
            mutation Connect($viewingKey: ViewingKey!) {
                connect(viewingKey: $viewingKey)
            }
        "#;
        
        let request = json!({
            "query": connect_mutation,
            "variables": {
                "viewingKey": viewing_key.clone()
            }
        });
        
        let response = client
            .post(format!("{}/graphql", endpoint))
            .json(&request)
            .send()
            .await
            .context("Failed to send connect request")?;
        
        let connect_response: ConnectResponse = response
            .json()
            .await
            .context("Failed to parse connect response")?;
        
        if let Some(errors) = connect_response.errors {
            return Err(anyhow::anyhow!("GraphQL errors: {:?}", errors));
        }
        
        let session_id = connect_response
            .data
            .ok_or_else(|| anyhow::anyhow!("No data in response"))?
            .connect;
        
        info!(
            "Created wallet {} with session_id: {} on endpoint: {}",
            index, session_id, endpoint
        );
        
        Ok(Wallet {
            viewing_key: viewing_key.clone(),
            session_id,
            replica_endpoint: endpoint.to_string(),
            created_at: Utc::now(),
            last_viewing_update: None,
            last_progress_update: None,
            viewing_update_count: 0,
            progress_update_count: 0,
            consecutive_empty_queries: 0,
            highest_index: 0,
            highest_relevant_index: 0,
        })
    }

    async fn monitor_wallet_subscription(&self, wallet: Wallet) {
        let session_id = wallet.session_id.clone();
        let endpoint = wallet.replica_endpoint.clone();
        
        // Store wallet in state
        {
            let mut wallets = self.wallets.write().await;
            wallets.insert(session_id.clone(), wallet);
        }
        
        // Convert HTTP endpoint to WebSocket
        let ws_endpoint = endpoint
            .replace("http://", "ws://")
            .replace("https://", "wss://");
        let ws_url = format!("{}/graphql", ws_endpoint);
        
        // Start WebSocket subscription
        loop {
            match self.run_subscription(&session_id, &ws_url).await {
                Ok(_) => {
                    warn!("Subscription ended for wallet {}", session_id);
                }
                Err(e) => {
                    error!("Subscription error for wallet {}: {}", session_id, e);
                }
            }
            
            // Check if we should mark this as THE ISSUE™
            self.check_for_issue(&session_id).await;
            
            // Reconnect after delay
            sleep(Duration::from_secs(30)).await;
        }
    }

    async fn run_subscription(&self, session_id: &str, ws_url: &str) -> Result<()> {
        // Connect to WebSocket
        let (ws_stream, _) = connect_async(ws_url)
            .await
            .context("Failed to connect to WebSocket")?;
        
        let (mut write, mut read) = ws_stream.split();
        
        // Send connection init
        let init_msg = json!({
            "type": "connection_init"
        });
        write.send(Message::Text(init_msg.to_string())).await?;
        
        // Wait for connection_ack
        if let Some(msg) = read.next().await {
            let _ack = msg?;
        }
        
        // Send subscription
        let subscription_query = r#"
            subscription ShieldedTransactions($sessionId: HexEncoded!) {
                shieldedTransactions(sessionId: $sessionId, sendProgressUpdates: true) {
                    __typename
                    ... on ViewingUpdate {
                        index
                        update
                    }
                    ... on ShieldedTransactionsProgress {
                        highestIndex
                        highestRelevantIndex
                        highestRelevantWalletIndex
                    }
                }
            }
        "#;
        
        let subscribe_msg = json!({
            "id": "1",
            "type": "subscribe",
            "payload": {
                "query": subscription_query,
                "variables": {
                    "sessionId": session_id
                }
            }
        });
        
        write.send(Message::Text(subscribe_msg.to_string())).await?;
        
        // Process messages
        while let Some(msg) = read.next().await {
            match msg? {
                Message::Text(text) => {
                    if let Ok(response) = serde_json::from_str::<GraphQLResponse>(&text) {
                        if response.msg_type == "next" {
                            if let Some(payload) = response.payload {
                                self.process_subscription_event(session_id, payload).await;
                            }
                        } else if response.msg_type == "error" {
                            error!("Subscription error for {}: {:?}", session_id, response.payload);
                            break;
                        } else if response.msg_type == "complete" {
                            info!("Subscription completed for {}", session_id);
                            break;
                        }
                    }
                }
                Message::Close(_) => {
                    info!("WebSocket closed for {}", session_id);
                    break;
                }
                _ => {}
            }
        }
        
        Ok(())
    }

    async fn process_subscription_event(&self, session_id: &str, payload: serde_json::Value) {
        if let Ok(sub_data) = serde_json::from_value::<SubscriptionData>(payload) {
            if let Some(data) = sub_data.data {
                let mut wallets = self.wallets.write().await;
                if let Some(wallet) = wallets.get_mut(session_id) {
                    let now = Utc::now();
                    
                    match data.shielded_transactions {
                        ShieldedTransactionsEvent::ViewingUpdate { index, .. } => {
                            wallet.last_viewing_update = Some(now);
                            wallet.viewing_update_count += 1;
                            wallet.consecutive_empty_queries = 0; // Reset counter
                            info!(
                                "PM-18678: ViewingUpdate received - session: {}, index: {}, total: {}",
                                session_id, index, wallet.viewing_update_count
                            );
                        }
                        ShieldedTransactionsEvent::ShieldedTransactionsProgress {
                            highest_index,
                            highest_relevant_index,
                            ..
                        } => {
                            wallet.last_progress_update = Some(now);
                            wallet.progress_update_count += 1;
                            wallet.highest_index = highest_index;
                            wallet.highest_relevant_index = highest_relevant_index;
                            
                            // Check if we're getting progress but no viewing updates
                            if let Some(last_viewing) = wallet.last_viewing_update {
                                let duration = now.signed_duration_since(last_viewing);
                                if duration.num_minutes() > 5 && wallet.progress_update_count > wallet.viewing_update_count + 10 {
                                    warn!(
                                        "PM-18678 POTENTIAL ISSUE: Progress updates without ViewingUpdates - session: {}, last viewing: {} min ago",
                                        session_id,
                                        duration.num_minutes()
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    async fn check_for_issue(&self, session_id: &str) {
        let wallets = self.wallets.read().await;
        if let Some(wallet) = wallets.get(session_id) {
            let now = Utc::now();
            
            // Detect THE ISSUE™: Progress updates continue but ViewingUpdates stop
            let has_issue = if let (Some(last_viewing), Some(last_progress)) = 
                (wallet.last_viewing_update, wallet.last_progress_update) {
                
                let viewing_age = now.signed_duration_since(last_viewing);
                let progress_age = now.signed_duration_since(last_progress);
                
                // Issue detected if:
                // 1. Haven't received ViewingUpdate in 10+ minutes
                // 2. Still receiving ProgressUpdates (within last 2 minutes)
                // 3. Have received significantly more progress than viewing updates
                viewing_age.num_minutes() > 10 
                    && progress_age.num_minutes() < 2
                    && wallet.progress_update_count > wallet.viewing_update_count + 20
            } else {
                false
            };
            
            if has_issue {
                let mut issue_detected = self.issue_detected.write().await;
                if !issue_detected.contains_key(session_id) {
                    let runtime_minutes = now.signed_duration_since(wallet.created_at).num_minutes();
                    error!(
                        "PM-18678 THE ISSUE™ DETECTED! Session: {}, ViewingKey: {}, Runtime: {} minutes, ViewingUpdates: {}, ProgressUpdates: {}",
                        session_id, 
                        &wallet.viewing_key[..20.min(wallet.viewing_key.len())], // Show first 20 chars of viewing key
                        runtime_minutes,
                        wallet.viewing_update_count, 
                        wallet.progress_update_count
                    );
                    issue_detected.insert(session_id.to_string(), now);
                    
                    // Capture diagnostics
                    drop(wallets); // Release read lock before capturing
                    self.capture_diagnostics(session_id).await;
                }
            }
        }
    }

    async fn capture_diagnostics(&self, session_id: &str) {
        error!("PM-18678: Capturing diagnostics for session_id: {}", session_id);
        
        // Query database state
        let wallet_check = sqlx::query(
            r#"
            SELECT w.id, w.session_id, w.last_indexed_transaction_id,
                   COUNT(rt.id) as relevant_count
            FROM wallets w
            LEFT JOIN relevant_transactions rt ON rt.wallet_id = w.id
            WHERE w.session_id = $1
            GROUP BY w.id, w.session_id, w.last_indexed_transaction_id
            "#
        )
        .bind(session_id)
        .fetch_optional(&self.db_pool)
        .await;
        
        match wallet_check {
            Ok(Some(row)) => {
                let wallet_id: uuid::Uuid = row.get("id");
                let last_indexed: i64 = row.get("last_indexed_transaction_id");
                let relevant_count: i64 = row.get("relevant_count");
                error!(
                    "PM-18678: Wallet state - id: {}, last_indexed: {}, relevant_transactions: {}",
                    wallet_id, last_indexed, relevant_count
                );
            }
            Ok(None) => {
                error!("PM-18678: Wallet not found in database!");
            }
            Err(e) => {
                error!("PM-18678: Database query error: {}", e);
            }
        }
        
        // Check connection pool state
        self.check_database_connections().await;
        
        // Check transaction processing state
        let transaction_check = sqlx::query(
            r#"
            SELECT MAX(id) as max_id, COUNT(*) as total
            FROM transactions
            "#
        )
        .fetch_one(&self.db_pool)
        .await;
        
        if let Ok(row) = transaction_check {
            let max_id: Option<i64> = row.get("max_id");
            let total: i64 = row.get("total");
            error!(
                "PM-18678: Transaction state - max_id: {:?}, total: {}",
                max_id, total
            );
        }
    }

    async fn check_database_connections(&self) {
        let connections = sqlx::query(
            r#"
            SELECT pid, application_name, backend_start, state, 
                   backend_xmin::text as xmin,
                   EXTRACT(EPOCH FROM (NOW() - backend_start)) as connection_age_seconds
            FROM pg_stat_activity 
            WHERE datname = 'indexer'
            ORDER BY backend_start
            "#
        )
        .fetch_all(&self.db_pool)
        .await;
        
        match connections {
            Ok(conns) => {
                info!("PM-18678: Database connections: {}", conns.len());
                for row in conns {
                    let pid: i32 = row.get("pid");
                    let state: Option<String> = row.get("state");
                    let age: Option<f64> = row.get("connection_age_seconds");
                    if let Some(age_secs) = age {
                        if age_secs > 300.0 {
                            warn!(
                                "PM-18678: Old connection - pid: {}, age: {}s, state: {:?}",
                                pid, age_secs, state
                            );
                        }
                    }
                }
            }
            Err(e) => {
                error!("PM-18678: Failed to check database connections: {}", e);
            }
        }
    }

    async fn periodic_database_monitor(&self, interval_secs: u64) {
        let mut interval = interval(Duration::from_secs(interval_secs));
        
        loop {
            interval.tick().await;
            
            // Check for stale transactions
            let stale_check = sqlx::query(
                r#"
                SELECT pid, query
                FROM pg_stat_activity 
                WHERE datname = 'indexer' 
                  AND state = 'idle in transaction'
                  AND xact_start < NOW() - interval '5 minutes'
                "#
            )
            .fetch_all(&self.db_pool)
            .await;
            
            match stale_check {
                Ok(stale) if !stale.is_empty() => {
                    warn!("PM-18678: Found {} stale transactions", stale.len());
                    for row in stale {
                        let pid: i32 = row.get("pid");
                        let query: Option<String> = row.get("query");
                        warn!("PM-18678: Stale transaction - PID {}: {:?}", pid, query);
                    }
                }
                _ => {}
            }
            
            // Log overall status
            let uptime = self.start_time.elapsed();
            let wallets = self.wallets.read().await;
            let issues = self.issue_detected.read().await;
            
            info!(
                "PM-18678 Status: uptime: {}h, wallets: {}, issues_detected: {}",
                uptime.as_secs() / 3600,
                wallets.len(),
                issues.len()
            );
            
            // Log wallet states
            for (session_id, wallet) in wallets.iter() {
                let viewing_age = wallet.last_viewing_update
                    .map(|t| Utc::now().signed_duration_since(t).num_seconds())
                    .unwrap_or(-1);
                let progress_age = wallet.last_progress_update
                    .map(|t| Utc::now().signed_duration_since(t).num_seconds())
                    .unwrap_or(-1);
                    
                let runtime_minutes = Utc::now().signed_duration_since(wallet.created_at).num_minutes();
                info!(
                    "PM-18678 Wallet {} (created {} min ago): viewing_updates: {}, progress_updates: {}, last_viewing: {}s ago, last_progress: {}s ago",
                    session_id,
                    runtime_minutes,
                    wallet.viewing_update_count,
                    wallet.progress_update_count,
                    viewing_age,
                    progress_age
                );
            }
        }
    }

    async fn wait_for_services(&self, endpoints: &[String]) -> Result<()> {
        info!("PM-18678: Checking service readiness...");
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;
        
        let mut attempts = 0;
        const MAX_ATTEMPTS: u32 = 60; // 5 minutes total
        
        loop {
            attempts += 1;
            let mut all_ready = true;
            
            for endpoint in endpoints {
                let query = json!({
                    "query": "{ __typename }"
                });
                
                match client
                    .post(format!("{}/graphql", endpoint))
                    .json(&query)
                    .send()
                    .await
                {
                    Ok(response) if response.status().is_success() => {
                        info!("PM-18678: Service {} is ready", endpoint);
                    }
                    _ => {
                        warn!("PM-18678: Service {} not ready yet (attempt {}/{})", 
                              endpoint, attempts, MAX_ATTEMPTS);
                        all_ready = false;
                    }
                }
            }
            
            if all_ready {
                info!("PM-18678: All services are ready!");
                return Ok(());
            }
            
            if attempts >= MAX_ATTEMPTS {
                return Err(anyhow::anyhow!("Services failed to become ready after {} attempts", MAX_ATTEMPTS));
            }
            
            sleep(Duration::from_secs(5)).await;
        }
    }
    
    async fn generate_load(&self, endpoints: Vec<String>, wallet_count: usize, network_id: &str) {
        info!("PM-18678: Starting load generation with {} wallets", wallet_count);
        
        // Wait for all services to be ready before creating wallets
        if let Err(e) = self.wait_for_services(&endpoints).await {
            error!("PM-18678: Service readiness check failed: {}", e);
            // Continue anyway, as services might be partially ready
        }
        
        let mut failed_wallets = Vec::new();
        
        // First pass: try to create all wallets
        for i in 0..wallet_count {
            let endpoint = &endpoints[i % endpoints.len()];
            
            match self.create_wallet(endpoint, network_id, i).await {
                Ok(wallet) => {
                    info!("PM-18678: Successfully created wallet {}", i);
                    let state = self.clone();
                    tokio::spawn(async move {
                        state.monitor_wallet_subscription(wallet).await;
                    });
                }
                Err(e) => {
                    error!("PM-18678: Failed to create wallet {} on {}: {}", i, endpoint, e);
                    failed_wallets.push((i, endpoint.clone()));
                }
            }
            
            // Stagger wallet creation
            sleep(Duration::from_millis(500)).await;
        }
        
        // Retry failed wallets with exponential backoff
        if !failed_wallets.is_empty() {
            warn!("PM-18678: {} wallets failed to create. Starting retry loop...", failed_wallets.len());
            
            let mut retry_delay = Duration::from_secs(10);
            let max_retry_delay = Duration::from_secs(300); // 5 minutes max
            
            while !failed_wallets.is_empty() {
                sleep(retry_delay).await;
                
                let mut still_failed = Vec::new();
                
                for (i, endpoint) in failed_wallets {
                    match self.create_wallet(&endpoint, network_id, i).await {
                        Ok(wallet) => {
                            info!("PM-18678: Successfully created wallet {} on retry", i);
                            let state = self.clone();
                            tokio::spawn(async move {
                                state.monitor_wallet_subscription(wallet).await;
                            });
                        }
                        Err(e) => {
                            warn!("PM-18678: Wallet {} still failing: {}", i, e);
                            still_failed.push((i, endpoint));
                        }
                    }
                    sleep(Duration::from_millis(200)).await;
                }
                
                failed_wallets = still_failed;
                
                if !failed_wallets.is_empty() {
                    error!("PM-18678: {} wallets still failing. Retrying in {:?}...", 
                           failed_wallets.len(), retry_delay);
                    
                    // Exponential backoff
                    retry_delay = std::cmp::min(retry_delay * 2, max_retry_delay);
                }
            }
        }
        
        info!("PM-18678: Load generation complete. All {} wallets created.", wallet_count);
    }
}

impl Clone for MonitoringState {
    fn clone(&self) -> Self {
        Self {
            wallets: Arc::clone(&self.wallets),
            issue_detected: Arc::clone(&self.issue_detected),
            start_time: self.start_time,
            db_pool: self.db_pool.clone(),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("pm_18678_investigation=debug".parse()?)
                .add_directive("info".parse()?),
        )
        .init();
    
    let args = Args::parse();
    
    info!("PM-18678 Investigation Monitor Starting");
    info!("Configuration:");
    info!("  - API Endpoints: {}", args.api_endpoints);
    info!("  - Database: {}", args.database_url);
    info!("  - Network ID: {}", args.network_id);
    info!("  - Wallet Count: {}", args.wallet_count);
    
    // Parse endpoints
    let endpoints: Vec<String> = args.api_endpoints
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();
    
    // Create monitoring state
    let state = MonitoringState::new(&args.database_url).await?;
    
    // Start database monitoring
    let db_state = state.clone();
    let db_interval = args.db_check_interval;
    tokio::spawn(async move {
        db_state.periodic_database_monitor(db_interval).await;
    });
    
    // Start wallet load generation
    state.generate_load(endpoints, args.wallet_count, &args.network_id).await;
    
    // Keep the main thread alive and check for issues
    loop {
        sleep(Duration::from_secs(60)).await;
        
        let issues = state.issue_detected.read().await;
        if !issues.is_empty() {
            error!("PM-18678 THE ISSUE™ detected in {} wallets!", issues.len());
            for (session_id, first_detected) in issues.iter() {
                let duration = Utc::now().signed_duration_since(*first_detected);
                error!(
                    "  - Session {}: first detected {} minutes ago",
                    session_id,
                    duration.num_minutes()
                );
            }
        }
    }
}