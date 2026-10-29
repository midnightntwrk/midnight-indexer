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

//! Live Aura -> BABE flip on a node 3.0 dev chain, followed by a skipped BABE epoch.
//!
//! The dev chain's single committee member gets distinct Aura (`//Alice`) and BABE (`//Bob`)
//! keys, so every indexed block author names the authority set it was resolved against: Alice
//! before the first BABE block, Bob from it on. Epochs are shortened to 10 slots, governance
//! schedules the flip, and pausing the node past an epoch skips one.
//!
//! Requires Docker (Docker Desktop on macOS, e.g. `DOCKER_CONTEXT=desktop-linux`) and the release
//! build of `indexer-standalone`:
//!
//! ```text
//! cargo build --release -p indexer-standalone --features standalone
//! cargo nextest run -p indexer-tests --features standalone --run-ignored all babe_flip
//! ```

#![cfg(feature = "standalone")]

use anyhow::{Context, bail};
use serde_json::{Value, json};
use std::{
    env, fs,
    future::Future,
    net::TcpListener,
    path::Path,
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
use tokio::time::sleep;

/// `//Alice` sr25519 public key: the dev committee member's Aura key.
const ALICE: &str = "d43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d";
/// An Aura pre-runtime digest log: `DigestItem::PreRuntime` (6) for engine `aura`.
const AURA_PRE_RUNTIME_PREFIX: &str = "0x0661757261";
/// `//Bob` sr25519 public key: the dev committee member's BABE key.
const BOB: &str = "8eaf04151687736326c9fea17e25fc5287613693c912909cb226aa4794f26a48";
/// `//Bob` SS58 address.
const BOB_SS58: &str = "5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty";
/// The indexer secret used when `APP__INFRA__SECRET` is unset.
const FALLBACK_SECRET: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
/// `ConsensusEngine::schedule_flip`, SCALE-encoded: pallet index 10, call index 0.
const SCHEDULE_FLIP_CALL: &str = "0x0a00";
/// The public key the dev node credits block rewards to.
const SIDECHAIN_BLOCK_BENEFICIARY: &str =
    "04bcf7ad3be7a5c790460be82a713af570f22e0f801f6659ab8e84a52be6969e";
/// The node's pause, longer than two epochs, so the next block skips an epoch wherever in its epoch
/// the pause starts.
const SKIP_PAUSE: Duration = Duration::from_secs(130);
/// Slots per epoch, 1 minute at 6-second slots.
const SLOTS_PER_EPOCH: u64 = 10;
const WS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

/// Docker resources and the indexer process, removed when the test ends.
struct Cleanup {
    network: String,
    node_container: String,
    indexer: Option<Child>,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Some(mut indexer) = self.indexer.take() {
            let _ = indexer.kill();
            let _ = indexer.wait();
        }
        let _ = docker(&["rm", "-f", &self.node_container]);
        let _ = docker(&["network", "rm", &self.network]);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "boots docker containers and drives a live consensus flip; run explicitly"]
async fn babe_flip_with_skipped_epoch() -> anyhow::Result<()> {
    let node_image = env::var("NODE_IMAGE").or_else(|_| {
        latest_node_version()
            .map(|version| format!("ghcr.io/midnightntwrk/midnight-node:{version}"))
    })?;
    // No toolkit image is published for the node release, so the default follows main.
    let toolkit_image = env::var("TOOLKIT_IMAGE")
        .unwrap_or_else(|_| "midnightntwrk/midnight-node-toolkit:latest-main".to_string());
    let temp_dir = tempfile::tempdir().context("create tempdir")?;

    // 1. A dev chain-spec with short epochs and a distinct BABE key.
    let chainspec = docker(&[
        "run",
        "--rm",
        "-e",
        "CFG_PRESET=dev",
        &node_image,
        "build-spec",
    ])?;
    let mut chainspec = serde_json::from_str::<Value>(&chainspec).context("parse chain-spec")?;
    let config = &mut chainspec["genesis"]["runtimeGenesis"]["config"];
    config["sidechain"]["slotsPerEpoch"] = json!(SLOTS_PER_EPOCH);
    // The sole committee member's BABE session key, set at genesis as no call can register it.
    config["sessionCommitteeManagement"]["initialAuthorities"][0]["Permissioned"]["keys"]["babe"] =
        json!(BOB_SS58);
    let chainspec_path = temp_dir.path().join("chainspec.json");
    fs::write(&chainspec_path, chainspec.to_string()).context("write chain-spec")?;
    println!(
        "[1] dev chain-spec: {SLOTS_PER_EPOCH}-slot epochs, validator keys Aura {}, BABE {}",
        key_name(Some(ALICE)),
        key_name(Some(BOB))
    );

    // 2. The node, with Bob's BABE key in its keystore.
    let suffix = std::process::id();
    let network = format!("babe-flip-e2e-{suffix}");
    let node_container = format!("babe-flip-e2e-node-{suffix}");
    docker(&["network", "create", &network])?;
    let mut cleanup = Cleanup {
        network: network.clone(),
        node_container: node_container.clone(),
        indexer: None,
    };
    let node_rpc_port = free_port()?;
    docker(&[
        "run",
        "-d",
        "--name",
        &node_container,
        "--network",
        &network,
        "--network-alias",
        "node",
        "-p",
        &format!("{node_rpc_port}:9944"),
        "-e",
        "CFG_PRESET=dev",
        "-e",
        "CHAIN=/chainspec/chainspec.json",
        "-e",
        "APPEND_ARGS=--rpc-methods unsafe",
        "-e",
        &format!("SIDECHAIN_BLOCK_BENEFICIARY={SIDECHAIN_BLOCK_BENEFICIARY}"),
        "-v",
        &format!("{}:/chainspec/chainspec.json", chainspec_path.display()),
        &node_image,
    ])?;
    let node_rpc = format!("http://localhost:{node_rpc_port}");
    // The dev chain rejects the council's governance transactions with `Inability to pay some
    // fees` when they are submitted at block 1 or 2. They succeed once the chain is past block 5.
    wait_for("node blocks", Duration::from_secs(90), || async {
        let height = node::best_height(&node_rpc).await.unwrap_or_default();
        Ok((height >= 5).then_some(()))
    })
    .await?;
    node::rpc(
        &node_rpc,
        "author_insertKey",
        json!(["babe", "//Bob", format!("0x{BOB}")]),
    )
    .await?;
    println!("[2] node producing blocks; BABE key inserted");

    // 3. The indexer.
    let api_port = free_port()?;
    cleanup.indexer = Some(indexer::start(temp_dir.path(), node_rpc_port, api_port)?);
    let api_url = format!("http://localhost:{api_port}/api/v4/graphql");
    println!("[3] indexer started");

    // 4. The flip, scheduled through governance.
    let call_path = temp_dir.path().join("schedule_flip.hex");
    fs::write(&call_path, SCHEDULE_FLIP_CALL).context("write call")?;
    docker(&[
        "run",
        "--rm",
        "--network",
        &network,
        "-v",
        &format!("{}:/call/schedule_flip.hex", call_path.display()),
        &toolkit_image,
        "root-call",
        "--rpc-url",
        "ws://node:9944",
        "--encoded-call-file",
        "/call/schedule_flip.hex",
    ])?;
    let scheduled_height = node::best_height(&node_rpc).await?;
    println!(
        "[4] flip scheduled at block {scheduled_height} (session {}, Midnight epoch {})",
        node::session_index(&node_rpc, scheduled_height).await?,
        node::midnight_epoch(&node_rpc, scheduled_height).await?
    );
    // The flip lands at the end of the epoch after the next session rotation, within 3 epochs.
    let scanned = AtomicU64::new(1);
    let first_babe_height = wait_for("first BABE block", Duration::from_secs(240), || async {
        node::first_babe_height(&node_rpc, &scanned).await
    })
    .await?;
    println!("[4] first BABE block: {first_babe_height}");

    // 5. A skipped epoch: the next block after the pause starts a later epoch.
    let paused_height = node::best_height(&node_rpc).await?;
    let paused_epoch = node::babe_epoch_index(&node_rpc, paused_height).await?;
    println!(
        "[5] pausing the node at block {paused_height} (BABE epoch {paused_epoch}) for {}s, over \
         two epochs, to skip one",
        SKIP_PAUSE.as_secs()
    );
    docker(&["pause", &node_container])?;
    sleep(SKIP_PAUSE).await;
    docker(&["unpause", &node_container])?;
    // The node may seal a block between reading `paused_height` and pausing, so the skip lies
    // between two consecutive blocks within the next few.
    wait_for(
        "blocks after the pause",
        Duration::from_secs(60),
        || async {
            let height = node::best_height(&node_rpc).await.unwrap_or_default();
            Ok((height >= paused_height + 3).then_some(()))
        },
    )
    .await?;
    let mut post_skip = None;
    for height in paused_height + 1..=paused_height + 3 {
        let parent_epoch = node::babe_epoch_index(&node_rpc, height - 1).await?;
        let epoch = node::babe_epoch_index(&node_rpc, height).await?;
        if epoch >= parent_epoch + 2 {
            post_skip = Some((height, parent_epoch, epoch));
            break;
        }
    }
    let (post_skip_height, before_skip_epoch, post_skip_epoch) =
        post_skip.context("no block after the pause skips a BABE epoch")?;
    println!(
        "[5] block {post_skip_height} resumes in BABE epoch {post_skip_epoch}, after epoch \
         {before_skip_epoch}"
    );

    // 6. Every author names the set its engine resolves against.
    let last_height = post_skip_height + SLOTS_PER_EPOCH;
    wait_for("indexer", Duration::from_secs(180), || async {
        let height = indexer::height(&api_url).await.unwrap_or_default();
        Ok((height >= last_height).then_some(()))
    })
    .await
    .with_context(|| {
        let log = fs::read_to_string(temp_dir.path().join("indexer.log")).unwrap_or_default();
        let tail = log.lines().rev().take(30).collect::<Vec<_>>();
        format!(
            "indexer behind block {last_height}; its log ends:\n{}",
            tail.into_iter().rev().collect::<Vec<_>>().join("\n")
        )
    })?;

    println!("[6] sessions up to block {last_height}:");
    let mut previous_session = None;
    for height in 1..=last_height {
        let session = node::session_index(&node_rpc, height).await?;
        if previous_session == Some(session) {
            continue;
        }
        previous_session = Some(session);
        let engine = if node::aura_authored(&node_rpc, height).await? {
            "Aura".to_string()
        } else {
            format!(
                "BABE epoch {}",
                node::babe_epoch_index(&node_rpc, height).await?
            )
        };
        let note = if height == first_babe_height {
            "first BABE block"
        } else if height == post_skip_height {
            "after the skipped epoch"
        } else {
            ""
        };
        println!(
            "    block {height:<4} session {session:<3} {engine:<13} {note:<23} Midnight epoch {}",
            node::midnight_epoch(&node_rpc, height).await?
        );
    }
    println!("[6] indexer reached block {last_height}; authors by phase:");
    let phases = [
        ("Aura", 1, first_babe_height - 1, ALICE),
        (
            "BABE up to the pause",
            first_babe_height,
            post_skip_height - 1,
            BOB,
        ),
        (
            "after the skipped epoch",
            post_skip_height,
            last_height,
            BOB,
        ),
    ];
    let mut mismatches = Vec::new();
    for (phase, first, last, expected) in phases {
        let mut wrong = Vec::new();
        for height in first..=last {
            let author = indexer::block_author(&api_url, height).await?;
            if author.as_deref() != Some(expected) {
                wrong.push(format!("{height}: {}", key_name(author.as_deref())));
            }
        }
        let blocks = if first == last {
            format!("block  {first}")
        } else {
            format!("blocks {first}-{last}")
        };
        let indexed = if wrong.is_empty() {
            format!("indexed {} \u{2713}", key_name(Some(expected)))
        } else {
            format!("indexed otherwise at {}", wrong.join(", "))
        };
        println!(
            "    {blocks:<14} {phase:<24} expected {:<8} {indexed}",
            key_name(Some(expected))
        );
        mismatches.extend(wrong);
    }
    if !mismatches.is_empty() {
        bail!("wrong block authors: {}", mismatches.join(", "));
    }

    Ok(())
}

/// The dev key name of an author, or its first bytes if it is neither key.
fn key_name(author: Option<&str>) -> String {
    match author {
        Some(ALICE) => "//Alice".to_string(),
        Some(BOB) => "//Bob".to_string(),
        Some(author) => format!("{}\u{2026}", &author[..author.len().min(8)]),
        None => "none".to_string(),
    }
}

async fn wait_for<
    Check: FnMut() -> CheckFuture,
    CheckFuture: Future<Output = anyhow::Result<Option<Found>>>,
    Found,
>(
    what: &str,
    timeout: Duration,
    mut check: Check,
) -> anyhow::Result<Found> {
    let start = Instant::now();
    loop {
        if let Some(found) = check().await? {
            return Ok(found);
        }
        if start.elapsed() >= timeout {
            bail!("timed out after {timeout:?} waiting for {what}");
        }
        sleep(Duration::from_secs(1)).await;
    }
}

/// The last `NODE_VERSIONS` line.
fn latest_node_version() -> anyhow::Result<String> {
    let versions = fs::read_to_string(Path::new(WS_DIR).join("NODE_VERSIONS"))
        .context("read NODE_VERSIONS")?;
    versions
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(str::to_string)
        .context("NODE_VERSIONS is empty")
}

fn free_port() -> anyhow::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").context("bind ephemeral port")?;
    Ok(listener.local_addr().context("local addr")?.port())
}

fn docker(args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("docker")
        .args(args)
        .output()
        .with_context(|| format!("run docker {}", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "docker {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

mod node {
    use super::*;

    pub(super) async fn rpc(node_rpc: &str, method: &str, params: Value) -> anyhow::Result<Value> {
        let response = reqwest::Client::new()
            .post(node_rpc)
            .json(&json!({"id": 1, "jsonrpc": "2.0", "method": method, "params": params}))
            .send()
            .await
            .with_context(|| format!("send {method}"))?
            .json::<Value>()
            .await
            .with_context(|| format!("decode {method}"))?;
        if let Some(error) = response.get("error") {
            bail!("{method} failed: {error}");
        }
        Ok(response["result"].clone())
    }

    /// The index of the BABE epoch the block at `height` belongs to. It restarts at 0 when the
    /// flip initialises BABE.
    pub(super) async fn babe_epoch_index(node_rpc: &str, height: u64) -> anyhow::Result<u64> {
        // `sp_consensus_babe::Epoch` begins with its `epoch_index: u64`.
        leading_integer(node_rpc, height, "BabeApi_current_epoch", 8).await
    }

    /// The index of the session the block at `height` belongs to.
    pub(super) async fn session_index(node_rpc: &str, height: u64) -> anyhow::Result<u64> {
        leading_integer(node_rpc, height, "SessionInfoApi_current_session_index", 4).await
    }

    /// The Midnight (sidechain) epoch the block at `height` belongs to.
    pub(super) async fn midnight_epoch(node_rpc: &str, height: u64) -> anyhow::Result<u64> {
        // `sp_sidechain::SidechainStatus` begins with its `epoch: u64`.
        leading_integer(
            node_rpc,
            height,
            "GetSidechainStatus_get_sidechain_status",
            8,
        )
        .await
    }

    /// The little-endian integer of `bytes` bytes that the runtime API `method` returns first at
    /// the block at `height`.
    async fn leading_integer(
        node_rpc: &str,
        height: u64,
        method: &str,
        bytes: usize,
    ) -> anyhow::Result<u64> {
        let hash = rpc(node_rpc, "chain_getBlockHash", json!([height])).await?;
        let result = rpc(node_rpc, "state_call", json!([method, "0x", hash])).await?;
        let integer = result
            .as_str()
            .and_then(|result| result.get(2..2 + 2 * bytes))
            .with_context(|| format!("{method} result too short"))?;
        let integer =
            u64::from_str_radix(integer, 16).with_context(|| format!("parse {method}"))?;
        Ok(integer.swap_bytes() >> (64 - 8 * bytes))
    }

    /// Whether the block at `height` carries an Aura pre-runtime digest, which every block before
    /// the first BABE block does.
    pub(super) async fn aura_authored(node_rpc: &str, height: u64) -> anyhow::Result<bool> {
        let hash = rpc(node_rpc, "chain_getBlockHash", json!([height])).await?;
        let header = rpc(node_rpc, "chain_getHeader", json!([hash])).await?;
        let logs = header["digest"]["logs"]
            .as_array()
            .context("header without digest logs")?;
        Ok(logs.iter().any(|log| {
            log.as_str()
                .is_some_and(|log| log.starts_with(AURA_PRE_RUNTIME_PREFIX))
        }))
    }

    pub(super) async fn best_height(node_rpc: &str) -> anyhow::Result<u64> {
        let header = rpc(node_rpc, "chain_getHeader", json!([])).await?;
        let number = header["number"].as_str().context("header without number")?;
        u64::from_str_radix(number.trim_start_matches("0x"), 16).context("parse block number")
    }

    /// The height of the first block without an Aura pre-runtime digest, once the chain has one,
    /// scanning from `scanned` and advancing it past the blocks checked.
    pub(super) async fn first_babe_height(
        node_rpc: &str,
        scanned: &AtomicU64,
    ) -> anyhow::Result<Option<u64>> {
        let best = best_height(node_rpc).await?;
        for height in scanned.load(Ordering::Relaxed)..=best {
            if !aura_authored(node_rpc, height).await? {
                return Ok(Some(height));
            }
        }
        scanned.store(best + 1, Ordering::Relaxed);
        Ok(None)
    }
}

mod indexer {
    use super::*;

    pub(super) fn start(dir: &Path, node_rpc_port: u16, api_port: u16) -> anyhow::Result<Child> {
        let target_dir =
            env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| format!("{WS_DIR}/target"));
        let binary = format!("{target_dir}/release/indexer-standalone");
        let log = fs::File::create(dir.join("indexer.log")).context("create indexer log")?;
        let errors = log.try_clone().context("clone indexer log handle")?;
        let node_url = format!("ws://localhost:{node_rpc_port}");

        Command::new(&binary)
            .env("RUST_LOG", "chain_indexer=info,error")
            .env(
                "CONFIG_FILE",
                format!("{WS_DIR}/indexer-standalone/config.yaml"),
            )
            .env("APP__INFRA__API__PORT", api_port.to_string())
            .env("APP__INFRA__NODE__URL", &node_url)
            // Longer than the node's pause: re-subscribing after a stall fails with `Invalid block
            // hash`, which takes chain-indexer down.
            .env("APP__INFRA__NODE__SUBSCRIPTION_RECOVERY_TIMEOUT", "300s")
            // spo-indexer dials its own node URL; left at the config default it exhausts its
            // reconnect attempts and takes the whole process down.
            .env("APP__INFRA__SPO_NODE__URL", &node_url)
            .env("APP__INFRA__SPO_NODE__BLOCKFROST_ID", "babe-flip-e2e-dummy")
            .env(
                "APP__INFRA__SECRET",
                env::var("APP__INFRA__SECRET").unwrap_or_else(|_| FALLBACK_SECRET.to_string()),
            )
            .env(
                "APP__INFRA__STORAGE__CNN_URL",
                dir.join("indexer.sqlite").display().to_string(),
            )
            .env(
                "APP__INFRA__LEDGER_DB__CNN_URL",
                dir.join("ledger-db.sqlite").display().to_string(),
            )
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(errors))
            .spawn()
            .with_context(|| {
                format!(
                    "spawn {binary}; build it with `cargo build --release -p indexer-standalone \
                     --features standalone`"
                )
            })
    }

    async fn graphql(api_url: &str, query: &str, variables: Value) -> anyhow::Result<Value> {
        let response = reqwest::Client::new()
            .post(api_url)
            .json(&json!({ "query": query, "variables": variables }))
            .send()
            .await
            .context("send GraphQL request")?
            .json::<Value>()
            .await
            .context("decode GraphQL response")?;
        if let Some(errors) = response.get("errors") {
            bail!("GraphQL errors: {errors}");
        }
        response
            .get("data")
            .cloned()
            .context("GraphQL response has no data")
    }

    pub(super) async fn height(api_url: &str) -> anyhow::Result<u64> {
        let data = graphql(api_url, "{ block { height } }", json!({})).await?;
        data["block"]["height"]
            .as_u64()
            .context("no indexed block height")
    }

    /// The block author the API reports, hex-encoded without a prefix.
    pub(super) async fn block_author(api_url: &str, height: u64) -> anyhow::Result<Option<String>> {
        let data = graphql(
            api_url,
            "query($height: Int!) { block(offset: { height: $height }) { author } }",
            json!({ "height": height }),
        )
        .await?;
        Ok(data["block"]["author"]
            .as_str()
            .map(|author| author.trim_start_matches("0x").to_lowercase()))
    }
}
