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

use anyhow::Context;
use fs_extra::dir::{CopyOptions, copy};
use indexer_common::domain::NetworkId;
use indexer_tests::e2e;
use reqwest::StatusCode;
use std::{
    collections::HashMap,
    net::TcpListener,
    path::Path,
    time::{Duration, Instant},
};
use tempfile::TempDir;
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{Mount, WaitFor},
    runners::AsyncRunner,
};
#[cfg(feature = "cloud")]
use testcontainers_modules::postgres::Postgres;
use tokio::{
    process::{Child, Command},
    time::sleep,
};

const API_READY_TIMEOUT: Duration = Duration::from_secs(30);
const NODE_VERSION: &str = "0.13.0-alpha.1";

/// Setup for e2e testing using workspace executables built by cargo. Sets up the Indexer with the
/// "cloud" architecture, i.e. as three separate processes and also PostgreSQL and NATS as Docker
/// containers. This is intended to be executed locally (`just test`) as well as on CI.
#[tokio::test]
#[cfg(feature = "cloud")]
async fn main() -> anyhow::Result<()> {
    // Start PostgreSQL and NATS.
    let (_postgres_container, postgres_port) = start_postgres().await.context("start postgres")?;
    let (_nats_container, nats_url) = start_nats().await.context("start nats")?;
    // Give PostgreSQL and NATS some headstart.
    sleep(Duration::from_millis(3_000)).await;

    // Start node.
    let node_handle = start_node().await.context("start node")?;

    // Start Indexer components.
    let mut chain_indexer = start_chain_indexer(postgres_port, &nats_url, &node_handle.node_url)
        .context("start chain-indexer")?;
    let (mut indexer_api, api_port) =
        start_indexer_api(postgres_port, &nats_url, &mut chain_indexer)
            .await
            .context("start indexer-api")?;
    let mut wallet_indexer = start_wallet_indexer(
        postgres_port,
        &nats_url,
        &mut chain_indexer,
        &mut indexer_api,
    )
    .await
    .context("start wallet-indexer")?;

    // Wait until indexer-api ready.
    wait_for_api_ready(api_port, API_READY_TIMEOUT)
        .await
        .context("wait for indexer-api to become ready")?;

    // Run the tests.
    let result = e2e::run(NetworkId::Undeployed, "localhost", api_port, false).await;

    // It is best practice to kill the processes even when spawned with `kill_on_drop`.
    let _ = chain_indexer.kill().await;
    let _ = indexer_api.kill().await;
    let _ = wallet_indexer.kill().await;

    result
}

/// Setup for e2e testing using workspace executables built by cargo. Sets up the Indexer with the
/// "standalone" architecture, i.e. as a single process. This is intended to be executed locally
/// (`just test`) as well as on CI.
#[tokio::test]
#[cfg(feature = "standalone")]
async fn main() -> anyhow::Result<()> {
    // Start node.
    let node_handle = start_node().await.context("start node")?;

    // Start Indexer components.
    let (mut indexer_standalone, api_port, _temp_dir) =
        start_indexer_standalone(&node_handle.node_url).context("start indexer_standalone")?;

    // Wait until indexer-api ready.
    wait_for_api_ready(api_port, API_READY_TIMEOUT)
        .await
        .context("wait for indexer-api to become ready")?;

    // Run the tests.
    let result = e2e::run(NetworkId::Undeployed, "localhost", api_port, false).await;

    // It is best practice to kill the processes even when spawned with `kill_on_drop`.
    let _ = indexer_standalone.kill().await;

    result
}

struct NodeHandle {
    node_url: String,

    // Needed to extend the lifetime over the execution of `start_node`.
    _temp_dir: TempDir,

    // Needed to extend the lifetime over the execution of `start_node`.
    _node_container: ContainerAsync<GenericImage>,
}

async fn start_node() -> anyhow::Result<NodeHandle> {
    let node_dir = Path::new(&format!("{}/../.node", env!("CARGO_MANIFEST_DIR")))
        .join(NODE_VERSION)
        .canonicalize()
        .context("create path to node directory")?;
    let temp_dir = tempfile::tempdir().context("cannot create tempdir")?;
    copy(&node_dir, &temp_dir, &CopyOptions::default())
        .context("copy .node directory into tempdir")?;
    let node_path = temp_dir.path().join(NODE_VERSION).display().to_string();

    let node_container = GenericImage::new("ghcr.io/midnight-ntwrk/midnight-node", NODE_VERSION)
        .with_wait_for(WaitFor::message_on_stderr("9944"))
        .with_mount(Mount::bind_mount(node_path, "/node"))
        .with_env_var("SHOW_CONFIG", "false")
        .with_env_var("CFG_PRESET", "dev")
        .start()
        .await
        .context("start node container")?;

    let node_port = node_container
        .get_host_port_ipv4(9944)
        .await
        .context("failed to get node port")?;
    let node_url = format!("ws://localhost:{node_port}");

    Ok(NodeHandle {
        node_url,
        _temp_dir: temp_dir,
        _node_container: node_container,
    })
}

#[cfg(feature = "cloud")]
async fn start_postgres() -> anyhow::Result<(ContainerAsync<Postgres>, u16)> {
    use testcontainers::{ImageExt, runners::AsyncRunner};

    let postgres_container = Postgres::default()
        .with_db_name("indexer")
        .with_user("indexer")
        .with_password(env!("APP__INFRA__STORAGE__PASSWORD"))
        .with_tag("17.1-alpine")
        .start()
        .await
        .context("start Postgres container")?;

    let postgres_port = postgres_container
        .get_host_port_ipv4(5432)
        .await
        .context("get Postgres port")?;

    Ok((postgres_container, postgres_port))
}

#[cfg(feature = "cloud")]
async fn start_nats() -> anyhow::Result<(ContainerAsync<GenericImage>, String)> {
    use testcontainers::{ImageExt, core::WaitFor, runners::AsyncRunner};

    let nats_container = GenericImage::new("nats", "2.11.1")
        .with_wait_for(WaitFor::message_on_stderr("Server is ready"))
        .with_cmd([
            "--user",
            "indexer",
            "--pass",
            env!("APP__INFRA__PUB_SUB__PASSWORD"),
            "-js",
        ])
        .start()
        .await
        .context("start NATS container")?;

    // In spite of the above "WaitFor" NATS stubbornly rejects connections.
    let start = Instant::now();
    while reqwest::get("localhost:8222/healthz")
        .await
        .and_then(|r| r.error_for_status())
        .is_err()
        && Instant::now() - start < Duration::from_millis(1_500)
    {
        sleep(Duration::from_millis(100)).await;
    }

    let nats_port = nats_container
        .get_host_port_ipv4(4222)
        .await
        .context("get NATS port")?;
    let nats_url = format!("localhost:{nats_port}");

    Ok((nats_container, nats_url))
}

#[cfg(feature = "cloud")]
fn start_chain_indexer(
    postgres_port: u16,
    nats_url: &str,
    node_url: &str,
) -> anyhow::Result<Child> {
    let env_vars = [
        ("RUST_LOG", "chain_indexer=error".to_string()),
        (
            "CONFIG_FILE",
            format!(
                "{}/../chain-indexer/config.yaml",
                env!("CARGO_MANIFEST_DIR")
            ),
        ),
        ("APP__INFRA__NODE__URL", node_url.to_owned()),
        ("APP__INFRA__PUB_SUB__URL", nats_url.to_owned()),
        ("APP__INFRA__STORAGE__PORT", postgres_port.to_string()),
        ("APP__INFRA__LEDGER_STATE_STORAGE__URL", nats_url.to_owned()),
    ];

    spawn_child("chain-indexer", env_vars.into())
}

#[cfg(feature = "cloud")]
async fn start_indexer_api(
    postgres_port: u16,
    nats_url: &str,
    chain_indexer: &mut Child,
) -> anyhow::Result<(Child, u16)> {
    let api_port = find_free_port()?;

    let env_vars = [
        ("RUST_LOG", "error".to_string()),
        (
            "CONFIG_FILE",
            format!("{}/../indexer-api/config.yaml", env!("CARGO_MANIFEST_DIR")),
        ),
        ("APP__INFRA__API__PORT", api_port.to_string()),
        ("APP__INFRA__API__MAX_COMPLEXITY", "250".into()),
        ("APP__INFRA__NETWORK_ID", "Undeployed".to_string()),
        ("APP__INFRA__PUB_SUB__URL", nats_url.to_owned()),
        ("APP__INFRA__STORAGE__PORT", postgres_port.to_string()),
        ("APP__INFRA__LEDGER_STATE_STORAGE__URL", nats_url.to_owned()),
    ];

    let child = spawn_child("indexer-api", env_vars.into());

    if child.is_err() {
        let _ = chain_indexer.kill().await;
    }

    child.map(|child| (child, api_port))
}

#[cfg(feature = "cloud")]
async fn start_wallet_indexer(
    postgres_port: u16,
    nats_url: &str,
    chain_indexer: &mut Child,
    indexer_api: &mut Child,
) -> anyhow::Result<Child> {
    let env_vars = [
        ("RUST_LOG", "error".into()),
        (
            "CONFIG_FILE",
            format!(
                "{}/../wallet-indexer/config.yaml",
                env!("CARGO_MANIFEST_DIR")
            ),
        ),
        ("APP__INFRA__PUB_SUB__URL", nats_url.to_owned()),
        ("APP__INFRA__STORAGE__PORT", postgres_port.to_string()),
    ];

    let child = spawn_child("wallet-indexer", env_vars.into());

    if child.is_err() {
        let _ = chain_indexer.kill().await;
        let _ = indexer_api.kill().await;
    }

    child
}

#[cfg(feature = "standalone")]
fn start_indexer_standalone(node_url: &str) -> anyhow::Result<(Child, u16, TempDir)> {
    let api_port = find_free_port()?;
    let temp_dir = tempfile::tempdir().context("cannot create tempdir")?;
    let sqlite_file = temp_dir.path().join("indexer.sqlite").display().to_string();

    let env_vars = [
        ("RUST_LOG", "error".to_string()),
        (
            "CONFIG_FILE",
            format!(
                "{}/../indexer-standalone/config.yaml",
                env!("CARGO_MANIFEST_DIR")
            ),
        ),
        ("APP__INFRA__API__PORT", api_port.to_string()),
        ("APP__INFRA__API__MAX_COMPLEXITY", "250".into()),
        ("APP__INFRA__NODE__URL", node_url.to_owned()),
        ("APP__INFRA__STORAGE__CNN_URL", sqlite_file),
    ];

    spawn_child("indexer-standalone", env_vars.into()).map(|child| (child, api_port, temp_dir))
}

fn spawn_child(
    name: &'static str,
    env_vars: HashMap<&'static str, String>,
) -> anyhow::Result<Child> {
    Command::new(format!(
        "{}/../target/debug/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .envs(env_vars)
    .kill_on_drop(true)
    .spawn()
    .context(format!("spawn child {name}"))
}

async fn wait_for_api_ready(api_port: u16, timeout: Duration) -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    let ready_url = format!("http://localhost:{}/ready", api_port);

    let start_time = Instant::now();
    while start_time.elapsed() < timeout {
        match client.get(&ready_url).send().await {
            Ok(response) if response.status() == StatusCode::OK => {
                return Ok(());
            }

            _ => {
                sleep(Duration::from_millis(500)).await;
            }
        }
    }

    anyhow::bail!("indexer-api has not become ready within {timeout:?}")
}

fn find_free_port() -> anyhow::Result<u16> {
    // Bind to port 0, which tells the OS to assign a free port.
    let listener = TcpListener::bind("127.0.0.1:0").context("bind to 127.0.0.1:0")?;
    let standalone_address = listener.local_addr().context("get standalone address")?;
    Ok(standalone_address.port())
}
