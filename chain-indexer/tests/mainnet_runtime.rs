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

//! Blocks produced by the node 1.0 runtime — the runtime mainnet upgraded to at block
//! 1_774_492 (protocol version 1_000_000) — must be ingestible. Indexer versions without
//! node 1.0 support fail here, either with `ProtocolVersionError::Unsupported(1_000_000)`
//! or with subxt metadata validation errors.
//!
//! This file does **not** cover the 2026-07-20 upgrade *boundary* (enactment block
//! 1_774_491 with a contract action). That lives in
//! `chain-indexer/src/infra/subxt_node/runtime_upgrade_boundary.rs` and runs from
//! recorded fixtures in `just test` (midnight-indexer#1402).

#![cfg(any(feature = "cloud", feature = "standalone"))]

use anyhow::Context;
use chain_indexer::{
    domain::node::Node,
    infra::subxt_node::{Config, SubxtNode},
};
use fs_extra::dir::{CopyOptions, copy};
use futures::TryStreamExt;
use std::{fs, path::Path, pin::pin, time::Duration};
use testcontainers::{
    GenericImage, ImageExt,
    core::{Mount, WaitFor},
    runners::AsyncRunner,
};
use walkdir::WalkDir;

/// The node version running the same runtime (identical metadata) as mainnet after the
/// upgrade at block 1_774_492.
const NODE_VERSION: &str = "1.0.0";

#[tokio::test(flavor = "multi_thread")]
async fn test_finalized_blocks_node_1_0() -> anyhow::Result<()> {
    let _ledger_db = init_ledger_db().await?;

    let node_dir = Path::new(&format!("{}/../.node", env!("CARGO_MANIFEST_DIR")))
        .join(NODE_VERSION)
        .canonicalize()
        .context("create path to node directory")?;
    let temp_dir = tempfile::tempdir().context("create tempdir")?;
    copy(&node_dir, &temp_dir, &CopyOptions::default())
        .context("copy .node directory into tempdir")?;

    // The node container runs as non-root user (appuser), so the bind-mounted directory
    // must be writable by all users.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let chain_dir = temp_dir.path().join(NODE_VERSION).join("chain");
        if chain_dir.exists() {
            for entry in WalkDir::new(&chain_dir) {
                let entry = entry.context("walk chain directory")?;
                let path = entry.path();
                let mode = if path.is_dir() { 0o777 } else { 0o666 };
                fs::set_permissions(path, fs::Permissions::from_mode(mode))
                    .with_context(|| format!("set permissions on {}", path.display()))?;
            }
        }
    }

    let node_path = temp_dir.path().join(NODE_VERSION).display().to_string();
    let node_container = GenericImage::new("midnightntwrk/midnight-node", NODE_VERSION)
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
        .context("get node port")?;

    let config = Config {
        url: format!("ws://localhost:{node_port}"),
        reconnect_max_delay: Duration::from_secs(1),
        reconnect_max_attempts: 1,
        subscription_recovery_timeout: Duration::from_secs(30),
    };
    let mut node = SubxtNode::new(config).await.context("create SubxtNode")?;

    let blocks = node.finalized_blocks(None);
    let mut blocks = pin!(blocks);
    for _ in 0..3 {
        let block = blocks
            .try_next()
            .await
            .context("get next finalized block")?
            .context("stream of finalized blocks must not end")?;
        assert_eq!(u32::from(block.protocol_version), 1_000_000);
    }

    Ok(())
}

#[cfg(feature = "cloud")]
async fn init_ledger_db()
-> anyhow::Result<testcontainers::ContainerAsync<testcontainers_modules::postgres::Postgres>> {
    use indexer_common::infra::{
        ledger_db, migrations,
        pool::postgres::{Config, PostgresPool},
    };
    use sqlx::postgres::PgSslMode;
    use testcontainers_modules::postgres::Postgres;

    let postgres_container = Postgres::default()
        .with_db_name("indexer")
        .with_user("indexer")
        .with_password("postgres")
        .with_tag("17.1-alpine")
        .start()
        .await
        .context("start Postgres container")?;
    let postgres_port = postgres_container
        .get_host_port_ipv4(5432)
        .await
        .context("get Postgres port")?;

    let config = Config {
        host: "localhost".to_string(),
        port: postgres_port,
        dbname: "indexer".to_string(),
        user: "indexer".to_string(),
        password: "postgres".to_string().into(),
        sslmode: PgSslMode::Prefer,
        max_connections: 10,
        idle_timeout: Duration::from_secs(60),
        max_lifetime: Duration::from_secs(5 * 60),
    };
    let pool = PostgresPool::new(config)
        .await
        .context("create PostgresPool")?;
    migrations::postgres::run(&pool)
        .await
        .context("run Postgres migrations")?;

    ledger_db::init(
        ledger_db::Config {
            cache_max_nodes: 1_024,
        },
        pool,
    );

    Ok(postgres_container)
}

#[cfg(feature = "standalone")]
async fn init_ledger_db() -> anyhow::Result<tempfile::TempDir> {
    use indexer_common::infra::ledger_db;

    let temp_dir = tempfile::tempdir().context("create tempdir")?;
    let cnn_url = temp_dir
        .path()
        .join("ledger-db.sqlite")
        .display()
        .to_string();

    ledger_db::init(ledger_db::Config {
        cache_max_nodes: 1_024,
        cnn_url,
    })
    .await
    .context("init ledger db")?;

    Ok(temp_dir)
}
