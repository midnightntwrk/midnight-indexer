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

//! Live ledger 8 -> 9 hard-fork crossing, from genesis, against a real node.
//!
//! Boots the ledger-9 migration node on a *ledger-8* chain-spec (so the chain
//! starts below `spec_version` 2_000_000 but the migration host functions are
//! present), attaches `indexer-standalone`, drives the governance runtime
//! upgrade, and checks that the indexer crosses the boundary in lock-step with
//! the node.
//!
//! # What makes this a real test
//!
//! `chain-indexer` re-derives the whole ledger state itself and compares its
//! arena root against the node's `ledger_state_root` on *every* block
//! (`chain-indexer/src/application.rs`, "ledger state root mismatch"). That root
//! covers `dust`, so it is a total consistency oracle: a wrong dust wipe, a
//! dropped replayed system transaction, or one applied out of order all bail the
//! indexer on the very next block. This test's job is therefore not to invent a
//! comparison but to *drive a chain that exercises one* -- specifically, one
//! whose dust generation set is non-empty before the fork, so the wipe and the
//! node's cNIGHT replay have something to act on.
//!
//! Step 3 asserts exactly that (`pre_fork_generation_end_index > 0`). Without it
//! the whole test would pass vacuously against an indexer that never implemented
//! the wipe at all.
//!
//! # `registeredForDustGeneration` across the boundary
//!
//! The root comparison covers the ledger's dust state, but not how the indexer
//! *projects* it into `unshielded_utxos.registered_for_dust_generation`. That
//! column is a snapshot, computed once when the UTXO is created and never
//! updated. That is sound while the chain runs -- the ledger's `night_indices`
//! is append-only -- but a fork that wipes dust strands every entry, so the API
//! scopes the flag to the chain's current dust epoch. Steps 4b and 8b pin the
//! `true` -> `false` flip across the boundary; 9b reports the post-fork side when
//! there is any.
//!
//! # Requirements
//!
//! Docker, plus these images (see `NODE_VERSIONS` and the `*_TAG` overrides):
//!
//! - `midnight-node:1.0.0` -- ledger-8 chain-spec source.
//! - `midnight-node:2.1.0-beta.1` + matching toolkit -- the migration node.
//!
//! Ignored by default: it pulls/boots containers and takes a few minutes. Run it
//! with
//!
//! ```text
//! cargo nextest run -p indexer-tests --features standalone --run-ignored all hardfork
//! ```

#![cfg(feature = "standalone")]

use anyhow::{Context, bail};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    env, fs,
    net::TcpListener,
    path::Path,
    process::{Child, Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};
use tempfile::TempDir;
use tokio::time::sleep;

/// Ledger-8 node whose `dev` preset provides the fork-from chain-spec.
const FROM_NODE_TAG: &str = "1.0.0";

/// Genesis-funded dev wallet the test transacts from.
const SOURCE_SEED: &str = "0000000000000000000000000000000000000000000000000000000000000001";

/// Any `spec_version` at or above this is a ledger-9 runtime.
const LEDGER_9_SPEC_VERSION: u64 = 2_000_000;

const WS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

fn image_registry() -> String {
    env::var("IMAGE_REGISTRY").unwrap_or_else(|_| "midnightntwrk".to_string())
}

/// The migration node under test: the last `NODE_VERSIONS` line, overridable.
fn to_node_tag() -> anyhow::Result<String> {
    if let Ok(tag) = env::var("TO_NODE_TAG") {
        return Ok(tag);
    }
    let versions = fs::read_to_string(Path::new(WS_DIR).join("NODE_VERSIONS"))
        .context("read NODE_VERSIONS")?;
    versions
        .lines()
        .rfind(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
        .context("NODE_VERSIONS is empty")
}

fn toolkit_tag() -> anyhow::Result<String> {
    match env::var("TO_TOOLKIT_TAG") {
        Ok(tag) => Ok(tag),
        Err(_) => to_node_tag(),
    }
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

/// One unshielded output as the API reports it, reduced to the fields this test
/// compares across the fork boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CreatedOutput {
    output_index: u64,
    owner: String,
    registered_for_dust_generation: bool,
}

impl CreatedOutput {
    /// Parse an `unshieldedCreatedOutputs` list, ordered by output index.
    fn parse_list(value: &Value) -> anyhow::Result<Vec<Self>> {
        let mut outputs = value
            .as_array()
            .context("unshieldedCreatedOutputs is not a list")?
            .iter()
            .map(|output| {
                Ok(Self {
                    output_index: output["outputIndex"].as_u64().context("no outputIndex")?,
                    owner: output["owner"].as_str().context("no owner")?.to_owned(),
                    registered_for_dust_generation: output["registeredForDustGeneration"]
                        .as_bool()
                        .context("no registeredForDustGeneration")?,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        outputs.sort_by_key(|output| output.output_index);
        Ok(outputs)
    }
}

/// The docker network, node container and indexer child process, all torn down
/// on drop so a failing assertion cannot leak them.
struct Harness {
    network: String,
    node_container: String,
    /// Behind a `Mutex` so the health check can poll `try_wait` (which needs
    /// `&mut Child`) from the `&self` closures `wait_for` drives.
    indexer: Mutex<Option<Child>>,
    node_rpc: String,
    api_url: String,
    temp_dir: TempDir,
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.indexer.lock()
            && let Some(mut indexer) = guard.take()
        {
            let _ = indexer.kill();
            let _ = indexer.wait();
        }
        let _ = docker(&["rm", "-f", &self.node_container]);
        let _ = docker(&["network", "rm", &self.network]);
    }
}

impl Harness {
    /// A toolkit run attached to the harness network. The toolkit reaches the
    /// node as `ws://node:9944` from inside it rather than via `--network host`
    /// plus a published port: Docker Desktop for Mac gives containers no real
    /// host networking, so host-networked toolkit runs cannot reach the node.
    fn toolkit(&self, tag: &str, args: &[&str]) -> anyhow::Result<String> {
        let image = format!("{}/midnight-node-toolkit:{tag}", image_registry());
        let mut full = vec!["run", "--rm", "--network", &self.network, &image];
        full.extend_from_slice(args);
        docker(&full)
    }

    async fn graphql(&self, query: &str, variables: Value) -> anyhow::Result<Value> {
        let body = json!({ "query": query, "variables": variables });
        let response = reqwest::Client::new()
            .post(&self.api_url)
            .json(&body)
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

    async fn spec_version(&self) -> anyhow::Result<u64> {
        let response = reqwest::Client::new()
            .post(&self.node_rpc)
            .json(&json!({"id": 1, "jsonrpc": "2.0", "method": "state_getRuntimeVersion"}))
            .send()
            .await
            .context("state_getRuntimeVersion")?
            .json::<Value>()
            .await
            .context("decode runtime version")?;
        response["result"]["specVersion"]
            .as_u64()
            .context("no specVersion in response")
    }

    /// `dustGenerationEndIndex` at `height`, i.e. the ledger's dust generation
    /// tree `first_free` as the indexer reconstructed it.
    async fn generation_end_index(&self, height: u64) -> anyhow::Result<u64> {
        let data = self
            .graphql(
                "query($h: Int!) { block(offset: { height: $h }) { dustGenerationEndIndex } }",
                json!({ "h": height }),
            )
            .await?;
        data["block"]["dustGenerationEndIndex"]
            .as_u64()
            .with_context(|| format!("no dustGenerationEndIndex at height {height}"))
    }

    /// NIGHT balance per reward address, summed over its registrations, as
    /// `dustGenerations` reports it.
    ///
    /// This is the query that sums `dust_generation_info WHERE dtime IS NULL`,
    /// so it is the one a fork-wiped-but-unretired row inflates. Reading it on
    /// both sides of the boundary is what makes step 8's invariant meaningful.
    async fn night_balances(
        &self,
        reward_addresses: &[String],
    ) -> anyhow::Result<BTreeMap<String, u128>> {
        // The query caps at ten reward addresses, so page through them.
        const BATCH: usize = 10;
        let mut balances = BTreeMap::new();

        for chunk in reward_addresses.chunks(BATCH) {
            let data = self
                .graphql(
                    "query($a: [CardanoRewardAddress!]!) { \
                       dustGenerations(cardanoRewardAddresses: $a) { \
                         cardanoRewardAddress registrations { nightBalance } } }",
                    json!({ "a": chunk }),
                )
                .await?;

            for entry in data["dustGenerations"]
                .as_array()
                .context("dustGenerations is not a list")?
            {
                let address = entry["cardanoRewardAddress"]
                    .as_str()
                    .context("no cardanoRewardAddress")?
                    .to_owned();
                let balance = entry["registrations"]
                    .as_array()
                    .context("no registrations")?
                    .iter()
                    .map(|r| {
                        r["nightBalance"]
                            .as_str()
                            .context("no nightBalance")?
                            .parse::<u128>()
                            .context("parse nightBalance")
                    })
                    .sum::<anyhow::Result<u128>>()?;
                balances.insert(address, balance);
            }
        }

        Ok(balances)
    }

    /// Every transaction in blocks `from..=to` that created unshielded outputs,
    /// as `(transaction hash, outputs)`.
    ///
    /// Reads a fixed block range rather than looking transactions up by hash:
    /// `transactions(offset: { hash })` is plural by design
    /// (`get_transactions_by_hash`), because a hash is not unique across blocks,
    /// so a range is the unambiguous way to re-read the same rows later.
    async fn created_outputs_in_range(
        &self,
        from: u64,
        to: u64,
    ) -> anyhow::Result<Vec<(String, Vec<CreatedOutput>)>> {
        let mut found = Vec::new();

        for height in from..=to {
            let data = self
                .graphql(
                    "query($h: Int!) { block(offset: { height: $h }) { transactions { hash \
                       unshieldedCreatedOutputs { \
                         owner outputIndex registeredForDustGeneration } } } }",
                    json!({ "h": height }),
                )
                .await?;

            let Some(transactions) = data["block"]["transactions"].as_array() else {
                continue;
            };
            for transaction in transactions {
                let outputs = CreatedOutput::parse_list(&transaction["unshieldedCreatedOutputs"])?;
                if !outputs.is_empty() {
                    let hash = transaction["hash"].as_str().context("no hash")?.to_owned();
                    found.push((hash, outputs));
                }
            }
        }

        Ok(found)
    }

    async fn indexed_height(&self) -> anyhow::Result<u64> {
        let data = self.graphql("{ block { height } }", json!({})).await?;
        data["block"]["height"]
            .as_u64()
            .context("no indexed block height")
    }

    fn node_logs(&self) -> anyhow::Result<String> {
        let output = Command::new("docker")
            .args(["logs", &self.node_container])
            .output()
            .context("docker logs")?;
        Ok(format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }

    fn indexer_log(&self) -> anyhow::Result<String> {
        fs::read_to_string(self.temp_dir.path().join("indexer.log")).context("read indexer log")
    }

    /// The indexer bails on a boundary failure rather than indexing on, so a
    /// dead process *is* the failure signal. Surface the reason, not just the
    /// exit.
    fn assert_indexer_healthy(&self, stage: &str) -> anyhow::Result<()> {
        let log = self.indexer_log().unwrap_or_default();
        for marker in [
            "ledger state root mismatch",
            "zswap state root mismatch",
            "translate ledger state",
        ] {
            if log.contains(marker) {
                let context = log
                    .lines()
                    .filter(|line| line.contains(marker))
                    .take(3)
                    .collect::<Vec<_>>()
                    .join("\n");
                bail!("indexer reported '{marker}' at {stage}:\n{context}");
            }
        }
        let mut guard = self.indexer.lock().expect("indexer mutex poisoned");
        if let Some(indexer) = guard.as_mut()
            && let Some(status) = indexer.try_wait().context("poll indexer")?
        {
            let tail = log.lines().rev().take(10).collect::<Vec<_>>().join("\n");
            bail!("indexer exited ({status}) at {stage}; last lines:\n{tail}");
        }
        Ok(())
    }
}

/// Poll `f` until it returns `Some`, or fail after `timeout`.
async fn wait_for<T, F, Fut>(what: &str, timeout: Duration, mut f: F) -> anyhow::Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = anyhow::Result<Option<T>>>,
{
    let start = Instant::now();
    loop {
        if let Some(value) = f().await? {
            return Ok(value);
        }
        if start.elapsed() >= timeout {
            bail!("timed out after {timeout:?} waiting for {what}");
        }
        sleep(Duration::from_millis(1_000)).await;
    }
}

/// Bech32-encode the cNIGHT reward addresses the fork-from chain-spec ships, so
/// they can be handed to `dustGenerationStatus`. Genesis stores them raw; the
/// API takes them Bech32 with the `stake_test` HRP.
fn cardano_reward_addresses(chainspec: &Value, limit: usize) -> anyhow::Result<Vec<String>> {
    use bech32::{Bech32, Hrp};

    let mappings = chainspec["genesis"]["runtimeGenesis"]["config"]["cNightObservation"]["config"]
        ["mappings"]
        .as_object()
        .context("chain-spec has no cNightObservation mappings")?;
    let hrp = Hrp::parse("stake_test").expect("valid hrp");

    mappings
        .keys()
        .take(limit)
        .map(|raw| {
            let bytes = const_hex_decode(raw)?;
            bech32::encode::<Bech32>(hrp, &bytes).context("bech32-encode reward address")
        })
        .collect()
}

fn const_hex_decode(s: &str) -> anyhow::Result<Vec<u8>> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).with_context(|| format!("hex-decode {s}")))
        .collect()
}

#[tokio::test]
#[ignore = "boots docker containers and drives a live runtime upgrade; run explicitly"]
async fn hardfork_8_to_9_crossing() -> anyhow::Result<()> {
    let registry = image_registry();
    let to_node = to_node_tag()?;
    let toolkit = toolkit_tag()?;
    let temp_dir = tempfile::tempdir().context("create tempdir")?;

    // --- 1. Ledger-8 chain-spec from the previous release -------------------
    //
    // This is what makes the fork real: the chain starts on a ledger-8 runtime
    // while running the ledger-9 binary, so `migrate_state_v8_to_v9` is present
    // from genesis but does not fire until the upgrade. The `dev` preset also
    // ships cNIGHT `utxo_owners` with matching ledger-8 dust generation entries,
    // which is what gives the wipe and the replay something to do.
    println!("[1] building ledger-8 chain-spec from node {FROM_NODE_TAG}");
    let chainspec_json = docker(&[
        "run",
        "--rm",
        "-e",
        "CFG_PRESET=dev",
        &format!("{registry}/midnight-node:{FROM_NODE_TAG}"),
        "build-spec",
    ])?;
    let chainspec = serde_json::from_str::<Value>(&chainspec_json).context("parse chain-spec")?;
    let reward_addresses = cardano_reward_addresses(&chainspec, usize::MAX)?;
    assert!(
        !reward_addresses.is_empty(),
        "the fork-from chain-spec must ship cNIGHT mappings, else the replay has nothing to do"
    );
    let chainspec_path = temp_dir.path().join("chainspec.json");
    fs::write(&chainspec_path, &chainspec_json).context("write chain-spec")?;

    // --- 2. Runtime WASM to upgrade to --------------------------------------
    //
    // Taken from the node image itself. A node image built by swapping only the
    // binary into a ledger-8 base still embeds the ledger-8 WASM, which would
    // make the upgrade a silent no-op; step 5 catches that by asserting the
    // spec_version actually crosses into the ledger-9 range.
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "amd64"
    };
    let wasm_path = temp_dir.path().join("runtime.wasm");
    let wasm = Command::new("docker")
        .args([
            "run",
            "--rm",
            "--entrypoint",
            "cat",
            &format!("{registry}/midnight-node:{to_node}"),
            &format!("/artifacts-{arch}/midnight_node_runtime.compact.compressed.wasm"),
        ])
        .output()
        .context("extract runtime WASM")?;
    if !wasm.status.success() {
        bail!(
            "extracting runtime WASM failed: {}",
            String::from_utf8_lossy(&wasm.stderr)
        );
    }
    fs::write(&wasm_path, &wasm.stdout).context("write runtime WASM")?;
    println!("[2] runtime WASM: {} bytes", wasm.stdout.len());

    // --- 3. Boot the migration node on the ledger-8 chain-spec --------------
    let suffix = std::process::id();
    let network = format!("hardfork-e2e-{suffix}");
    let node_container = format!("hardfork-e2e-node-{suffix}");
    docker(&["network", "create", &network])?;
    let node_rpc_port = free_port()?;

    let mut harness = Harness {
        network: network.clone(),
        node_container: node_container.clone(),
        indexer: Mutex::new(None),
        node_rpc: format!("http://localhost:{node_rpc_port}"),
        api_url: String::new(),
        temp_dir,
    };

    docker(&[
        "run",
        "-d",
        "--name",
        &node_container,
        "--network",
        &network,
        // The toolkit addresses the node by this alias from inside the network.
        "--network-alias",
        "node",
        "-p",
        &format!("{node_rpc_port}:9944"),
        "-e",
        "SHOW_CONFIG=false",
        "-e",
        "CFG_PRESET=dev",
        "-e",
        "CHAIN=/chainspec/chainspec.json",
        "-e",
        "SIDECHAIN_BLOCK_BENEFICIARY=04bcf7ad3be7a5c790460be82a713af570f22e0f801f6659ab8e84a52be6969e",
        "-v",
        &format!("{}:/chainspec/chainspec.json", chainspec_path.display()),
        &format!("{registry}/midnight-node:{to_node}"),
    ])?;

    let pre_fork_spec = wait_for("node RPC", Duration::from_secs(90), || async {
        Ok(harness.spec_version().await.ok())
    })
    .await?;
    assert!(
        pre_fork_spec < LEDGER_9_SPEC_VERSION,
        "chain must start on a ledger-8 runtime, got spec_version {pre_fork_spec}"
    );
    println!("[3] node up on ledger-8 spec_version {pre_fork_spec}");

    // --- 4. Attach the indexer and let it index the pre-fork chain ----------
    let api_port = free_port()?;
    harness.api_url = format!("http://localhost:{api_port}/api/v4/graphql");
    *harness.indexer.lock().expect("indexer mutex poisoned") = Some(start_indexer(
        harness.temp_dir.path(),
        node_rpc_port,
        api_port,
    )?);

    wait_for("indexer API readiness", Duration::from_secs(90), || async {
        let ready = reqwest::get(format!("http://localhost:{api_port}/ready"))
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        Ok(ready.then_some(()))
    })
    .await?;

    // Index a few pre-fork blocks, then read the dust generation tree size the
    // indexer reconstructed. This is the assertion the whole test rests on: if
    // the fork-from chain-spec ever stops shipping dust generation entries, the
    // wipe becomes a no-op and every later assertion passes for the wrong reason.
    let pre_fork_height = wait_for("pre-fork blocks", Duration::from_secs(120), || async {
        Ok(harness.indexed_height().await.ok().filter(|h| *h >= 5))
    })
    .await?;
    harness.assert_indexer_healthy("pre-fork")?;

    let pre_fork_generations = harness.generation_end_index(pre_fork_height).await?;
    assert!(
        pre_fork_generations > 0,
        "the pre-fork dust generation tree is empty ({pre_fork_generations}), so the hardfork \
         wipe would be a no-op and this test would prove nothing"
    );
    let pre_fork_balances = harness.night_balances(&reward_addresses).await?;
    let pre_fork_total = pre_fork_balances.values().sum::<u128>();
    assert!(
        pre_fork_total > 0,
        "no reward address holds any NIGHT before the fork, so the balance invariant in step 8 \
         would be vacuous"
    );
    println!(
        "[4] indexer at height {pre_fork_height}, pre-fork dust generations: \
         {pre_fork_generations}, NIGHT across {} reward addresses: {pre_fork_total}",
        pre_fork_balances.len()
    );

    // --- 4b. Pre-fork `registeredForDustGeneration` baseline ----------------
    //
    // The flag is a snapshot, not a live view: `registered_for_dust_generation_v8`
    // / `_v9` (`indexer-common/src/domain/ledger/ledger_state.rs`) ask the ledger
    // whether the new UTXO's initial nonce is in `dust.generation.night_indices`
    // at the moment the UTXO is created, and `chain-indexer/src/infra/storage.rs`
    // writes the answer once, on INSERT. Nothing updates that column afterwards,
    // and the API exposes the field only through the transaction that created the
    // UTXO -- there is no "as of now" read of it anywhere in the schema.
    //
    // No traffic is needed to observe this. The `dev` preset distributes its
    // genesis NIGHT as `PayFromTreasuryUnshielded` system transactions in the
    // first few blocks (measured: 20 single-output transactions across 4
    // addresses in blocks 0..=5, every one of them `true`), which is exactly the
    // cohort the wipe strands.
    //
    // Deliberately *not* driven by the toolkit: a pre-fork send would add a
    // second flaky external dependency for rows the chain already provides, and
    // post-fork the toolkit cannot reliably produce any (see step 9).
    let pre_fork_created = harness.created_outputs_in_range(0, pre_fork_height).await?;
    let registered_before = pre_fork_created
        .iter()
        .flat_map(|(_, outputs)| outputs)
        .filter(|output| output.registered_for_dust_generation)
        .count();
    assert!(
        registered_before > 0,
        "no unshielded output indexed in blocks 0..={pre_fork_height} is registered for dust \
         generation, so there is no row for the wipe to strand and step 8b would prove nothing: \
         {pre_fork_created:?}"
    );
    println!(
        "[4b] pre-fork baseline: {registered_before} of {} indexed unshielded outputs in blocks \
         0..={pre_fork_height} report registeredForDustGeneration = true",
        pre_fork_created
            .iter()
            .map(|(_, outputs)| outputs.len())
            .sum::<usize>()
    );

    // --- 5. Governance runtime upgrade --------------------------------------
    println!("[5] driving the governance runtime upgrade");
    // Not via `Harness::toolkit`: this one call needs the WASM bind-mounted.
    let upgrade = docker(&[
        "run",
        "--rm",
        "--network",
        &network,
        "-v",
        &format!("{}:/wasm/runtime.wasm", wasm_path.display()),
        &format!("{registry}/midnight-node-toolkit:{toolkit}"),
        "runtime-upgrade",
        "--wasm-file",
        "/wasm/runtime.wasm",
        "--rpc-url",
        "ws://node:9944",
        "-c",
        "//Eve",
        "-c",
        "//Ferdie",
        "-c",
        "//Dave",
        "-t",
        "//Alice",
        "-t",
        "//Bob",
        "-t",
        "//Charlie",
        "--signer-key",
        "//Alice",
    ])?;
    println!(
        "[5] {}",
        upgrade.lines().next_back().unwrap_or("upgrade done")
    );

    let post_fork_spec = wait_for("ledger-9 runtime", Duration::from_secs(120), || async {
        Ok(harness
            .spec_version()
            .await
            .ok()
            .filter(|v| *v >= LEDGER_9_SPEC_VERSION))
    })
    .await?;
    println!("[5] spec_version {pre_fork_spec} -> {post_fork_spec}");

    // --- 6. The indexer must cross the boundary -----------------------------
    //
    // The per-block root comparison in `application.rs` is the oracle: it covers
    // the whole ledger state, dust included, so it fires on the first block where
    // the indexer's translation, the dust wipe, or the replayed system
    // transactions disagree with the node.
    let post_fork_height = wait_for("post-fork indexing", Duration::from_secs(180), || async {
        harness.assert_indexer_healthy("crossing")?;
        let node_height = harness.indexed_height().await.ok();
        // Require the indexer to get meaningfully past the code-applied block,
        // so the replay window is indexed too, not just the boundary itself.
        Ok(node_height.filter(|h| *h >= pre_fork_height + 12))
    })
    .await?;
    harness.assert_indexer_healthy("post-fork")?;
    println!("[6] indexer crossed the boundary, now at height {post_fork_height}");

    // --- 7. The node's cNIGHT dust replay actually restored something -------
    //
    // Winding up is not the same as succeeding: a replay that self-cancels ends
    // in the same storage state as one that worked. `DustReapplyCompleted`
    // carries the count, so assert on that.
    let node_logs = harness.node_logs()?;
    assert!(
        node_logs.contains("DustReapplyStarted"),
        "the node never armed the cNIGHT dust replay; is this really a migration runtime?"
    );
    let completed = node_logs
        .lines()
        .find(|line| line.contains("DustReapplyCompleted"))
        .context("the node's dust generation replay never completed")?;
    let applied = completed
        .split("complete, ")
        .nth(1)
        .and_then(|rest| rest.split(' ').next())
        .and_then(|n| n.parse::<u64>().ok())
        .with_context(|| format!("cannot read an applied count from: {completed}"))?;
    assert!(
        applied > 0,
        "the dust replay restored nothing ({completed}); the chain-spec's cNIGHT utxo_owners \
         should have given it work to do"
    );
    println!("[7] node replayed {applied} cNIGHT dust generation entries");

    // --- 8. The indexer's dust state came back the same way -----------------
    let post_fork_generations = harness.generation_end_index(post_fork_height).await?;
    assert!(
        post_fork_generations > 0,
        "dust generation is empty after the fork: the wipe landed but the replayed \
         CNightGeneratesDustUpdate system transactions never reached the indexer"
    );
    assert_eq!(
        post_fork_generations, applied,
        "the indexer's rebuilt dust generation tree ({post_fork_generations}) disagrees with the \
         count the node says it replayed ({applied})"
    );

    // The dust generation tree legitimately shrinks across the boundary: the
    // wipe resets `first_free` and the replay refills only cNIGHT's slice. That
    // is the node's behaviour, mirrored faithfully, and the equality above is
    // what pins it.
    println!(
        "[8] dust generation end index {pre_fork_generations} -> {post_fork_generations} \
         (wipe + replay, matching the node)"
    );

    // FULL CONSISTENCY: every reward address must report the NIGHT the *live*
    // ledger backs, not the live entries plus the wiped ones.
    //
    // The fork leaves the pre-fork `dust_generation_info` rows behind with
    // `dtime IS NULL` -- nothing retires them, because the wipe happens inside
    // the state translation rather than via a transaction that could emit a
    // `DustGenerationDtimeUpdate`. Before `dust_epoch` scoped these reads, this
    // query summed both cohorts and reported roughly double for every holder the
    // replay restored (measured: 124 live rows over 72 real UTXOs).
    //
    // So each address must land on exactly one of two values: its pre-fork
    // balance if the replay restored it, or zero if it did not. Anything in
    // between - and in particular the sum of the two - is the regression.
    let post_fork_balances = harness.night_balances(&reward_addresses).await?;
    let mut restored = 0usize;
    for (address, pre) in &pre_fork_balances {
        let post = post_fork_balances.get(address).copied().unwrap_or(0);
        if post == *pre {
            restored += 1;
        }
        assert!(
            post == *pre || post == 0,
            "reward address {address} reports {post} NIGHT after the fork but held {pre} before. \
             A value near {} means wiped pre-fork generation rows are being summed alongside the \
             replayed ones (dust_epoch scoping regressed).",
            pre * 2
        );
    }
    assert!(
        restored > 0,
        "not one reward address kept its NIGHT balance across the fork; the replay restored \
         {applied} entries, so at least some should have"
    );
    let post_fork_total = post_fork_balances.values().sum::<u128>();
    assert!(
        post_fork_total <= pre_fork_total,
        "total NIGHT backing dust generation grew across a fork that only ever wipes and \
         partially restores: {pre_fork_total} -> {post_fork_total}"
    );
    println!(
        "[8] NIGHT balances consistent: {restored}/{} reward addresses restored intact, \
         total {pre_fork_total} -> {post_fork_total}, none double-counted",
        pre_fork_balances.len()
    );

    // The registrations survive as the indexer's own projection, and generation
    // is live again for at least one of them.
    // Capped at ten addresses by the resolver; a sample is enough here since the
    // balance invariant above already covered every address.
    let sample = &reward_addresses[..reward_addresses.len().min(10)];
    let status = harness
        .graphql(
            "query($a: [CardanoRewardAddress!]!) { \
               dustGenerationStatus(cardanoRewardAddresses: $a) { registered generationRate } }",
            json!({ "a": sample }),
        )
        .await?;
    let entries = status["dustGenerationStatus"]
        .as_array()
        .context("dustGenerationStatus is not a list")?;
    assert!(
        entries.iter().any(|e| e["registered"] == json!(true)),
        "no cNIGHT registration survived the fork: {entries:?}"
    );
    assert!(
        entries
            .iter()
            .any(|e| e["generationRate"].as_str().is_some_and(|r| r != "0")),
        "every reward address generates 0 DUST after the fork, so the replay did not restore \
         generation the indexer can see: {entries:?}"
    );
    println!("[8] cNIGHT registrations and generation rates survived the crossing");

    // --- 8b. The wipe clears `registeredForDustGeneration` ------------------
    //
    // The ledger's `night_indices` is append-only for the whole normal life of a
    // chain -- "a nonce, once inserted, is never removed"
    // (`DustGenerationState::night_indices`) -- and a later registration attaches
    // generations only to NIGHT outputs of the registering intent itself
    // (`apply_registration`), never to a UTXO that already exists. So the value
    // the chain-indexer computes at creation is stable, and storing it is sound
    // right up to a fork that wipes dust.
    //
    // This fork wipes it. Every stranded nonce leaves `night_indices` at once and
    // those UTXOs stop generating DUST -- which is exactly why step 9 below has to
    // re-register before it can pay a fee. Nothing retires the stored `true`: the
    // wipe happens inside the state translation, not via a transaction that could
    // emit an event.
    //
    // The API therefore scopes the flag to the chain's current dust epoch
    // (`scope_to_dust_epoch` in `indexer-api`'s unshielded storage), the same
    // mechanism `dust_epoch` already gives `dust_generation_info`. Assert the
    // result end-to-end: the very rows that read `true` in step 4b must read
    // `false` now, with everything else about them unchanged.
    //
    // The premise is that the wipe really stranded them, so check the generation
    // set actually shrank: step 8 pinned `post_fork_generations == applied`, i.e.
    // every surviving entry is one the node replayed for cNIGHT (measured:
    // 85 -> 52). Without that this assertion could pass on a fork that stranded
    // nothing.
    assert!(
        post_fork_generations < pre_fork_generations,
        "the dust generation set did not shrink across the fork \
         ({pre_fork_generations} -> {post_fork_generations}), so no pre-fork registration was \
         stranded and there is nothing for the epoch scoping to clear"
    );

    let post_fork_created = harness.created_outputs_in_range(0, pre_fork_height).await?;
    let expected = pre_fork_created
        .iter()
        .map(|(hash, outputs)| {
            let outputs = outputs
                .iter()
                .map(|output| CreatedOutput {
                    registered_for_dust_generation: false,
                    ..output.clone()
                })
                .collect::<Vec<_>>();
            (hash.clone(), outputs)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        post_fork_created, expected,
        "after the fork wiped dust state, the {registered_before} pre-fork outputs that reported \
         registeredForDustGeneration = true must report false -- the ledger dropped their \
         generation entries, and their holders cannot pay a fee until they re-register. \
         Everything else about the rows must be untouched. Before {pre_fork_created:?}, after \
         {post_fork_created:?}"
    );
    println!(
        "[8b] {} generation entries were stranded by the wipe \
         ({pre_fork_generations} -> {post_fork_generations}); all {registered_before} pre-fork \
         outputs in blocks 0..={pre_fork_height} now report registeredForDustGeneration = false",
        pre_fork_generations - post_fork_generations
    );

    // --- 9. The chain is still usable afterwards (best effort) --------------
    //
    // The wipe takes native NIGHT's generation entries with it and the replay
    // restores only cNIGHT's slice, so the genesis dev wallets cross holding
    // NIGHT but generating no DUST and cannot pay a fee until they re-register.
    // The registration funds itself from the retroactive DUST its
    // now-generationless NIGHT accrued -- the same path a real holder takes.
    //
    // NOT a hard assertion, because it exercises the *toolkit*, not the indexer,
    // and the toolkit is flaky here: replaying a wallet from genesis across the
    // boundary panics in the ledger arena roughly half the time --
    //
    //   thread 'main' panicked at storage-core/src/arena.rs:
    //   root should be in the arena (T=...MerklePatriciaTrie<(Sp<Utxo>, Sp<UtxoMeta>),
    //   InMemoryDB, NightAnn>): BackendLoader::get(): key ... not in storage arena
    //
    // -- because its in-memory wallet state spans both ledger majors. The node's
    // own `fork_context_8_to_9` calls `dust.wipe_local_state()` for exactly this
    // reason; the toolkit's from-genesis fetch path appears not to. Failing the
    // indexer's test on that would make it useless as a regression gate, so the
    // outcome is reported and the run continues to step 10, which is the part
    // that actually tests this repo.
    println!("[9] re-registering the source wallet's DUST address (best effort)");
    let post_fork_traffic = harness
        .toolkit(
            &toolkit,
            &[
                "generate-txs",
                "--fetch-cache",
                "inmemory",
                "register-dust-address",
                "--wallet-seed",
                SOURCE_SEED,
                "-s",
                "ws://node:9944",
                "-d",
                "ws://node:9944",
            ],
        )
        .and_then(|_| {
            println!("[9] submitting a ledger-9 transaction");
            harness.toolkit(
                &toolkit,
                &[
                    "generate-txs",
                    "--fetch-cache",
                    "inmemory",
                    "-s",
                    "ws://node:9944",
                    "-d",
                    "ws://node:9944",
                    "single-tx",
                    "--shielded-amount",
                    "10",
                    "--unshielded-amount",
                    "10",
                    "--source-seed",
                    SOURCE_SEED,
                    "--destination-address",
                    "mn_shield-addr_undeployed1tth9g6jf8he6cmhgtme6arty0jde7wnypsg53qc3x5navl9za355jqqvfftm8asg986dx9puzwkmedeune9nfkuqvtmccmxtjwvlrvccwypcs",
                    "--destination-address",
                    "mn_addr_undeployed1gkasr3z3vwyscy2jpp53nzr37v7n4r3lsfgj6v5g584dakjzt0xqun4d4r",
                ],
            )
        });

    let post_fork_traffic_landed = match post_fork_traffic {
        Ok(_) => {
            println!("[9] post-fork ledger-9 transaction submitted");
            true
        }
        Err(error) => {
            let panicked = format!("{error:#}").contains("root should be in the arena");
            println!(
                "[9] WARNING: post-fork toolkit traffic failed{}; continuing. Cause:\n{error:#}",
                if panicked {
                    " (known toolkit cross-fork wallet-replay panic)"
                } else {
                    ""
                }
            );
            false
        }
    };

    // Whatever the toolkit managed, give the indexer a few more blocks so step 10
    // covers ground on both sides of the boundary.
    let from_height = harness.indexed_height().await?;
    let final_height = wait_for("further blocks", Duration::from_secs(120), || async {
        harness.assert_indexer_healthy("post-traffic")?;
        Ok(harness
            .indexed_height()
            .await
            .ok()
            .filter(|h| *h >= from_height + 4))
    })
    .await?;

    // --- 9b. The post-fork side, when there is one --------------------------
    //
    // Reported, not asserted. A post-fork UTXO is the only place a `false` could
    // show up -- the wipe stranded the dev wallets' native NIGHT, so a UTXO
    // created for one of them after the boundary should read `false` until it
    // re-registers -- but the only source of post-fork unshielded outputs on this
    // chain is step 9's toolkit, which panics across the boundary about half the
    // time. On the run that produced this test's numbers there were none at all.
    //
    // Asserting a polarity that is absent on half the runs would make the gate
    // useless, and asserting one nobody has observed would be a guess, so print
    // what is there and leave the gates to 4b and 8b.
    if post_fork_traffic_landed {
        let post_fork_created = harness
            .created_outputs_in_range(post_fork_height, final_height)
            .await?;
        if post_fork_created.is_empty() {
            println!(
                "[9b] no post-fork transaction created unshielded outputs in blocks \
                 {post_fork_height}..={final_height}"
            );
        } else {
            println!("[9b] post-fork unshielded outputs: {post_fork_created:?}");
        }
    } else {
        println!("[9b] no post-fork traffic landed, nothing to report");
    }

    // --- 10. No gap anywhere, boundary and replay window included -----------
    //
    // Staying alive at the tip is not enough: the replayed batches arrive as
    // `SystemTransactionApplied` events from a multi-block migration, and a
    // consumer that dropped that whole window would still look healthy here.
    for height in 0..=final_height {
        let data = harness
            .graphql(
                "query($h: Int!) { block(offset: { height: $h }) { height } }",
                json!({ "h": height }),
            )
            .await?;
        let indexed = data["block"]["height"].as_u64();
        assert_eq!(
            indexed,
            Some(height),
            "block {height} is missing from the indexer; the chain has a gap across the fork"
        );
    }
    harness.assert_indexer_healthy("final")?;

    println!(
        "[10] every block 0..={final_height} indexed; crossed ledger 8 -> 9 with no root mismatch"
    );

    Ok(())
}

/// Start `indexer-standalone` as a child process, logging to `dir/indexer.log`.
fn start_indexer(dir: &Path, node_rpc_port: u16, api_port: u16) -> anyhow::Result<Child> {
    let target_dir = env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| format!("{WS_DIR}/target"));
    let log = fs::File::create(dir.join("indexer.log")).context("create indexer log")?;
    let errors = log.try_clone().context("clone indexer log handle")?;

    Command::new(format!("{target_dir}/debug/indexer-standalone"))
        .env(
            "RUST_LOG",
            "indexer_standalone=info,chain_indexer=info,indexer_api=info,error",
        )
        .env(
            "CONFIG_FILE",
            format!("{WS_DIR}/indexer-standalone/config.yaml"),
        )
        .env("APP__INFRA__API__PORT", api_port.to_string())
        .env(
            "APP__INFRA__NODE__URL",
            format!("ws://localhost:{node_rpc_port}"),
        )
        .env("APP__INFRA__SPO_NODE__BLOCKFROST_ID", "hardfork-e2e-dummy")
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
        .context("spawn indexer-standalone")
}
