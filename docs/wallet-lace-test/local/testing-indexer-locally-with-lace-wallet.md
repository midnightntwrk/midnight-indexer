# Testing the Rust Indexer Locally with Lace Wallet

This document explains how to run a **local** version of the Rust Indexer alongside a standalone Midnight node and a standalone proof server, then connect the Lace wallet (Chrome extension) to that standalone environment. You can create or restore test wallets, check balances, and send transactions—verifying that the Rust Indexer is correctly updating wallet states.

## Prerequisites

1. **Google Chrome (version 119 or later)**
   > Chrome derivatives like Brave may require disabling certain shields or security features to allow the standalone wallet <-> extension <-> proof server connections.

2. **[Midnight Lace Wallet (Alpha) Chrome Extension](https://docs.midnight.network/develop/tutorial/using/chrome-ext)**
    - Download the most recent version of the **testnet** Lace extension. (e.g., `midnight-lace-x.y.z.zip`)
    - Unzip it, then load it in Chrome with **Developer Mode** enabled (via **Settings > Extensions > Load unpacked**).
    - (Optional) **Pin** it in Chrome’s toolbar so you can easily access it.

3. **Local Docker environment**
    - Docker and Docker Compose installed.
    - Sufficient memory/CPU to run the node and indexer containers simultaneously.

4. **Local Rust Indexer repository**
    - Your cloned copy of the `midnight-indexer` (Rust-based).
    - Ability to build a standalone Docker image for the Indexer (optional, only if you want the latest standalone changes).
    - If you do _not_ need to build from your standalone code changes, you can pull the existing `ghcr.io/midnight-ntwrk/indexer:latest` image from GitHub Container Registry.

5. **Midnight Node & Proof Server**
    - For standalone testing, ensure you have the correct images/versions.
    - Find an official version guideline, e.g. (slack post)[https://shielded.slack.com/archives/C085QCBL2HF/p1739954453280739?thread_ts=1739265300.605929&cid=C085QCBL2HF]
    - Example reference, `ghcr.io/midnight-ntwrk/midnight-node:0.9.0-rc2` and `ghcr.io/midnight-ntwrk/proof-server:3.0.6`.

## 1. Run the Local Services

### 1.1 (Optional) Build the Indexer image from standalone source

If you have standalone changes in the Rust Indexer that you want to test, build your own Docker image:

```bash
# From your midnight-indexer root directory
just feature=standalone docker-indexer
```

- This creates (and tags) `ghcr.io/midnight-ntwrk/indexer:latest`.
- Alternatively, you can specify a custom tag if you do not want to overwrite `latest`. Then you’d update your compose file accordingly.

### 1.2 Start the Node and Rust Indexer via Docker Compose

Look for `docker-compose.yaml` in the repository.

You can either use the default 'latest' indexer tag or use your locally built image in the yaml file.

Then run:

```bash
# In the same folder as docker-compose.yaml:
docker compose up
```

This command brings up:
- **Midnight Node** (listening on `ws://localhost:9944`)
- **Rust Indexer** (HTTP GraphQL endpoint at `http://localhost:8088/api/v1/graphql`)

### 1.3 Run the Local Proof Server

In a separate terminal:

Use the appropriate version (e.g. 3.0.6)
```bash
docker run -p 6300:6300 ghcr.io/midnight-ntwrk/proof-server:[PUT VERSION NUMBER HERE] \
    -- 'midnight-proof-server --network undeployed'
```

This publishes the standalone proof server at `http://localhost:6300`.

---

## 2. Install and Configure Lace Wallet (Alpha)

1. **Load the Lace Extension** in Chrome as described in [Midnight Lace wallet docs](https://docs.midnight.network/develop/tutorial/using/chrome-ext).
2. **Pin** the Lace wallet extension in your toolbar (optional but recommended).
3. In Lace, either **create** a brand-new wallet or **restore** a prefunded test wallet.

> **Note:** If you have an older devnet version of Lace installed (version <= 1.1.5), you must remove it before installing the current alpha testnet extension.

---

## 3. Creating or Restoring a Wallet (Snippets)

Below are JavaScript snippets you can paste into Chrome’s DevTools Console after opening the Lace extension pop-up. They will automate the wallet creation/restore steps:

### 3.1 Create a New (Empty) Wallet

> **Important**: A new empty wallet may stay in "syncing" state until it **receives** some funds. This is a known behavior and might not fully show "synced" until it gets a transaction.

Open the Lace extension in Chrome, open DevTools (right-click > Inspect), switch to the **Sources** tab, and import the snippet below (create a snippet & copy and paste):

View the snippet here: [new_wallet.js](new_wallet.js)

Save, right lick and Run

After creation, the wallet is "empty."
~~Expect it to show "syncing…" until you send it some tDUST.~~ (not fixed but it can receive a fund and then becomes 'synced')

### 3.2 Restore a Prefunded Wallet

If the Node version you’re using includes specific "prefunded seeds," you can restore a wallet that already holds some tDUST or tokens. For node `0.9.0-rc2`, one known mnemonic is:

```text
[
 'abandon', 'abandon', 'abandon', 
 'abandon', 'abandon', 'abandon', 
 'abandon', 'abandon', 'abandon', 
 'abandon', 'abandon', 'abandon', 
 'abandon', 'abandon', 'abandon',
 'abandon', 'abandon', 'abandon', 
 'abandon', 'abandon', 'abandon',
 'abandon', 'abandon', 'diesel'
]
```

Paste the snippet below into 'Sources' after creating a new snippet. Make sure to update the `nodeAddress`, `indexerAddress`, `proverAddress`, and the `mnemonic` array if needed.

View the snippet here: [restore.js](restore.js)

After restoration completes, you would be seeing 'native token added' message twice and asked to type, I typed 'a', 'a' and then 'b', 'b' 
you should see a wallet that (eventually) syncs and displays its **prefunded** `tDUST` (and/or tokens).

---

## 4. Sending a Transaction

If you restored a wallet with actual tDUST or tokens, you can send to another wallet address.
For a locally created empty wallet, you can find its address on the Lace page (or copy it from logs). Then paste the snippet below into the snippet in 'Sources' tab to automate a transaction:

View the snippet here: [send.js](send.js)

If everything is configured, you should see the transaction confirm on the sending wallet, and the receiving wallet’s balance should increase (after it "syncs").

---

## 5. Troubleshooting & Known Issues

1. **Fresh Wallet Stuck in "Syncing"**
    - You might notice that brand-new empty wallets remain in "syncing" indefinitely. As soon as they receive any transaction (e.g., tDUST from a prefunded wallet), they become "synced."
    - This is a known behavior in certain Lace / Indexer combos and may be addressed in future releases.

2. **Check Correct Mnemonic for Prefunding**
    - The node’s "prefunded seeds" have changed over time. Ensure you’re using the correct mnemonics corresponding to the Node version you run (e.g., the one with `'diesel'` at the end for `0.9.0-rc2`).

3. **Indexer Logs**
    - If you’re debugging the Rust Indexer, run with `RUST_LOG=indexer=debug,...` to see more detailed logs. The logs should show subscription events for wallet updates, blocks, and transaction confirmations.

4. **Proof Server Connection**
    - If Lace complains about failing to connect to your standalone proof server, double-check the port and address in the Lace extension config. Chrome derivatives (Brave, Vivaldi) can block standalone addresses—disable "shields" or "blockers."

---

## 6. Further Reading

- **[Midnight Lace (Alpha) Documentation](https://docs.midnight.network/develop/tutorial/using/chrome-ext)**: Full instructions on installing or removing the Chrome extension.
- **[Related Resources on Slack](https://shielded.slack.com/archives/C080ARCQ8LS/p1740045780775709?thread_ts=1740045758.598839&cid=C080ARCQ8LS)**: It's a slack thread where related links and information are stated.
