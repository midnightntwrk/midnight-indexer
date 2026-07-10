// This file is part of midnight-indexer.
// Copyright (C) Midnight Foundation
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
use nix::{
    sys::signal::{self, Signal},
    unistd::Pid,
};
use std::{
    env, fs,
    net::TcpListener,
    path::Path,
    process::{Child, Command},
    sync::LazyLock,
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
use tokio::time::sleep;
use walkdir::WalkDir;

const API_READY_TIMEOUT: Duration = Duration::from_secs(30);

static LATEST_NODE_VERSION: LazyLock<String> = LazyLock::new(|| {
    let node_versions_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../NODE_VERSIONS");
    let node_versions = fs::read_to_string(&node_versions_path).unwrap_or_else(|error| {
        panic!(
            "cannot read node versions file at {}: {error}",
            node_versions_path.display()
        );
    });
    node_versions
        .lines()
        .last()
        .expect("node versions must not be empty")
        .to_string()
});

static WS_DIR: LazyLock<String> = LazyLock::new(|| format!("{}/..", env!("CARGO_MANIFEST_DIR")));
static TARGET_DIR: LazyLock<String> = LazyLock::new(|| {
    env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| format!("{}/target", &*WS_DIR))
});

/// Setup for e2e testing using workspace executables built by cargo. Sets up the Indexer with the
/// "cloud" architecture, i.e. as three separate processes and PostgreSQL as a Docker container.
/// Pub-sub uses Postgres `LISTEN`/`NOTIFY`, so no separate message broker is needed. This is
/// intended to be executed locally (`just test`) as well as on CI. This setup is also intended to
/// be used for test coverage measurements using `cargo llvm-cov`.
#[cfg(feature = "cloud")]
#[tokio::test]
async fn main() -> anyhow::Result<()> {
    // Start PostgreSQL.
    let (_postgres_container, postgres_port) = start_postgres().await?;
    println!("PostgreSQL started");
    // Give PostgreSQL some headstart.
    sleep(Duration::from_millis(3_000)).await;

    // Start node.
    let node_handle = start_node().await?;
    println!("Node started");

    // Start Indexer components.
    let mut chain_indexer = start_chain_indexer(postgres_port, &node_handle.node_url)?;
    println!("Chain Indexer started");
    let mut wallet_indexer = start_wallet_indexer(postgres_port).await?;
    println!("Wallet Indexer started");
    let (mut indexer_api, api_port) = start_indexer_api(postgres_port).await?;
    println!("Indexer API started");

    // Terminate Chain Indexer, then start it again.
    sleep(Duration::from_millis(1_000)).await;
    signal::kill(Pid::from_raw(chain_indexer.id() as i32), Signal::SIGTERM)
        .context("terminate Chain Indexer")?;
    chain_indexer
        .wait()
        .context("wait for Chain Indexer termination")?;
    chain_indexer = start_chain_indexer(postgres_port, &node_handle.node_url)?;
    println!("Indexer API started again");

    // Wait for Indexer API to become ready.
    wait_for_api_ready(api_port, API_READY_TIMEOUT).await?;
    println!("Indexer API ready");

    // Run the tests.
    let result = indexer_tests::e2e::run(
        "undeployed".try_into().unwrap(),
        "localhost",
        api_port,
        false,
    )
    .await;

    // Terminate Indexer components using SIGTERM and wait which is imporant for coverage data to be
    // written and to avoid zombie processes.
    let _ = signal::kill(Pid::from_raw(indexer_api.id() as i32), Signal::SIGTERM);
    let _ = signal::kill(Pid::from_raw(wallet_indexer.id() as i32), Signal::SIGTERM);
    let _ = signal::kill(Pid::from_raw(chain_indexer.id() as i32), Signal::SIGTERM);
    let _ = indexer_api.wait();
    let _ = wallet_indexer.wait();
    let _ = chain_indexer.wait();

    result
}

/// Setup for e2e testing using workspace executables built by cargo. Sets up the Indexer with the
/// "standalone" architecture, i.e. as a single process. This is intended to be executed locally
/// (`just test`) as well as on CI. This setup is also intended to be used for test coverage
/// measurements using `cargo llvm-cov`.
#[cfg(feature = "standalone")]
#[tokio::test]
async fn main() -> anyhow::Result<()> {
    // Start node.
    let node_handle = start_node().await?;
    println!("Node started");

    // Start Indexer.
    let (mut indexer_standalone, api_port, _temp_dir) =
        start_indexer_standalone(&node_handle.node_url).context("start indexer_standalone")?;
    println!("Indexer started");

    // Wait for indexer-api to become ready.
    wait_for_api_ready(api_port, API_READY_TIMEOUT).await?;
    println!("Indexer API ready");

    // Run the tests.
    let result = indexer_tests::e2e::run(
        "undeployed".try_into().unwrap(),
        "localhost",
        api_port,
        false,
    )
    .await;

    // Terminate Indexer using SIGTERM and wait which is imporant for coverage data to be written
    // and to avoid zombie processes.
    let _ = signal::kill(
        Pid::from_raw(indexer_standalone.id() as i32),
        Signal::SIGTERM,
    );
    let _ = indexer_standalone.wait();

    result
}

#[cfg(any(feature = "cloud", feature = "standalone"))]
struct NodeHandle {
    node_url: String,

    // Needed to extend the lifetime over the execution of `start_node`.
    _temp_dir: TempDir,

    // Needed to extend the lifetime over the execution of `start_node`.
    _node_container: ContainerAsync<GenericImage>,
}

#[cfg(any(feature = "cloud", feature = "standalone"))]
async fn start_node() -> anyhow::Result<NodeHandle> {
    use fs_extra::dir::{CopyOptions, copy};

    let node_dir = Path::new(&format!("{}/../.node", env!("CARGO_MANIFEST_DIR")))
        .join(LATEST_NODE_VERSION.trim())
        .canonicalize()
        .context("create path to node directory")?;
    let temp_dir = tempfile::tempdir().context("cannot create tempdir")?;
    copy(&node_dir, &temp_dir, &CopyOptions::default())
        .context("copy .node directory into tempdir")?;

    // Make chain directory writable by container user (appuser).
    // The new node container runs as non-root user (appuser) for security,
    // so the bind-mounted directory needs to be writable by all users.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let chain_dir = temp_dir
            .path()
            .join(LATEST_NODE_VERSION.trim())
            .join("chain");
        if chain_dir.exists() {
            fs::set_permissions(&chain_dir, fs::Permissions::from_mode(0o777))
                .context("set permissions on chain directory")?;

            // Recursively set permissions on all subdirectories and files.
            for entry in WalkDir::new(&chain_dir) {
                let entry = entry.context("walk chain directory")?;
                let path = entry.path();
                let mode = if path.is_dir() { 0o777 } else { 0o666 };
                fs::set_permissions(path, fs::Permissions::from_mode(mode))
                    .with_context(|| format!("set permissions on {}", path.display()))?;
            }
        }
    }

    let node_path = temp_dir
        .path()
        .join(LATEST_NODE_VERSION.trim())
        .display()
        .to_string();

    let node_container =
        GenericImage::new("midnightntwrk/midnight-node", LATEST_NODE_VERSION.trim())
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
    use Postgres;
    use testcontainers::{ImageExt, runners::AsyncRunner};

    // The indexer binaries require TLS (PgSslMode::Require), so the test Postgres must serve it.
    // Generate a throwaway self-signed cert at startup and enable ssl. `Require` does not validate
    // the certificate, so self-signed is sufficient and no client cert is needed. The outer
    // entrypoint passes our `sh -c` through verbatim (arg[0] != "postgres"); we then re-exec the
    // entrypoint with "postgres" so initdb and the POSTGRES_* env are still honoured.
    const SSL_ENTRYPOINT: &str = "apk add --no-cache openssl >/dev/null 2>&1 && \
        openssl req -new -x509 -days 365 -nodes -subj /CN=localhost \
            -keyout /var/lib/postgresql/server.key -out /var/lib/postgresql/server.crt && \
        chown postgres:postgres /var/lib/postgresql/server.key /var/lib/postgresql/server.crt && \
        chmod 600 /var/lib/postgresql/server.key && \
        exec docker-entrypoint.sh postgres \
            -c ssl=on \
            -c ssl_cert_file=/var/lib/postgresql/server.crt \
            -c ssl_key_file=/var/lib/postgresql/server.key";

    let postgres_container = Postgres::default()
        .with_db_name("indexer")
        .with_user("indexer")
        .with_password(env!("APP__INFRA__STORAGE__PASSWORD"))
        .with_tag("17.1-alpine")
        .with_cmd(["sh", "-c", SSL_ENTRYPOINT])
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
fn start_chain_indexer(postgres_port: u16, node_url: &str) -> anyhow::Result<Child> {
    Command::new(format!("{}/debug/chain-indexer", &*TARGET_DIR))
        .env(
            "RUST_LOG",
            "chain_indexer=info,fastrace_opentelemetry=off,error",
        )
        .env(
            "CONFIG_FILE",
            format!("{}/chain-indexer/config.yaml", &*WS_DIR),
        )
        .env("APP__INFRA__NODE__URL", node_url)
        .env("APP__INFRA__STORAGE__PORT", postgres_port.to_string())
        .env("APP__TELEMETRY__TRACING__ENABLED", "true")
        .spawn()
        .context("spawn chain-indexer process")
}

#[cfg(feature = "cloud")]
async fn start_wallet_indexer(postgres_port: u16) -> anyhow::Result<Child> {
    Command::new(format!("{}/debug/wallet-indexer", &*TARGET_DIR))
        .env(
            "RUST_LOG",
            "wallet_indexer=debug,fastrace_opentelemetry=off,error",
        )
        .env(
            "CONFIG_FILE",
            format!("{}/wallet-indexer/config.yaml", &*WS_DIR),
        )
        .env("APP__INFRA__STORAGE__PORT", postgres_port.to_string())
        .env("APP__TELEMETRY__TRACING__ENABLED", "true")
        .spawn()
        .context("spawn wallet-indexer process")
}

#[cfg(feature = "cloud")]
async fn start_indexer_api(postgres_port: u16) -> anyhow::Result<(Child, u16)> {
    let api_port = find_free_port()?;

    Command::new(format!("{}/debug/indexer-api", &*TARGET_DIR))
        .env(
            "RUST_LOG",
            "indexer_api=info,fastrace_opentelemetry=off,error",
        )
        .env(
            "CONFIG_FILE",
            format!("{}/indexer-api/config.yaml", &*WS_DIR),
        )
        .env("APP__INFRA__API__PORT", api_port.to_string())
        .env("APP__INFRA__API__MAX_COMPLEXITY", "600")
        .env("APP__INFRA__STORAGE__PORT", postgres_port.to_string())
        .env("APP__TELEMETRY__TRACING__ENABLED", "true")
        .spawn()
        .context("spawn indexer-api process")
        .map(|child| (child, api_port))
}

#[cfg(feature = "standalone")]
fn start_indexer_standalone(node_url: &str) -> anyhow::Result<(Child, u16, TempDir)> {
    let api_port = find_free_port()?;
    let temp_dir = tempfile::tempdir().context("cannot create tempdir")?;
    let sqlite_file = temp_dir.path().join("indexer.sqlite").display().to_string();
    let sqlite_ledger_db_file = temp_dir
        .path()
        .join("ledger-db.sqlite")
        .display()
        .to_string();

    Command::new(format!("{}/debug/indexer-standalone", &*TARGET_DIR))
        .env(
            "RUST_LOG",
            "indexer_standalone=info,chain_indexer=info,indexer_api=info,wallet_indexer=debug,fastrace_opentelemetry=off,error",
        )
        .env(
            "CONFIG_FILE",
            format!("{}/indexer-standalone/config.yaml", &*WS_DIR),
        )
        .env("APP__INFRA__API__PORT", api_port.to_string())
        .env("APP__INFRA__API__MAX_COMPLEXITY", "600")
        .env("APP__INFRA__NODE__URL", node_url)
        .env("APP__INFRA__SPO_NODE__BLOCKFROST_ID", "e2e-test-dummy")
        .env("APP__INFRA__STORAGE__CNN_URL", sqlite_file)
        .env("APP__INFRA__LEDGER_DB__CNN_URL", sqlite_ledger_db_file)
        .env("APP__TELEMETRY__TRACING__ENABLED", "true")
        .spawn()
        .context("spawn indexer-standalone process")
        .map(|child| (child, api_port, temp_dir))
}

#[cfg(any(feature = "cloud", feature = "standalone"))]
async fn wait_for_api_ready(api_port: u16, timeout: Duration) -> anyhow::Result<()> {
    use reqwest::StatusCode;

    let client = reqwest::Client::new();
    let ready_url = format!("http://localhost:{api_port}/ready");

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

#[cfg(any(feature = "cloud", feature = "standalone"))]
fn find_free_port() -> anyhow::Result<u16> {
    // Bind to port 0, which tells the OS to assign a free port.
    let listener = TcpListener::bind("127.0.0.1:0").context("bind to 127.0.0.1:0")?;
    let standalone_address = listener.local_addr().context("get standalone address")?;
    Ok(standalone_address.port())
}
