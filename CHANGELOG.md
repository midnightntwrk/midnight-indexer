# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [3.0.0-alpha.5] - 2025-10-10

### 🚀 Features

- Unify configuration across components (#20)
- Extend PostgreSQL pool configuration (#50)
- *(chain-indexer)* Start supporting node 0.13 (#21)
- *(indexer-api)* Remove deprecated health endpoint (#52)
- Rename ApplyStage to TransactionResult and add segment results (#54)
- Remove support for node 0.12 (#60)
- Add unshielded token support to indexer (merging feat/ut to main) (#62)
- Enhance unshielded token subscription with sync progress data (PM-17159) (#73)
- Enhance transaction metadata with status, fees, and execution results (#82)
- *(indexer-api)* Node-like default ordering of database query results (#102)
- Implement unshielded token ownership by contracts (#97)
- Add transaction ID offset parameter to unshielded subscription (#120)
- Improve error handling for network ID mismatches in unshielded address queries (#137)
- Support multiple ledger versions (#134)
- *(indexer-api)* Introduce ApiError with client and server errors (#147)
- *(indexer-api)* Remove obsolete metrics, add wallets connected gauge (#151)
- Rework unshielded API and implementation (#156)
- Enable tracing for GraphQL subscriptions (#179)
- Make checkmarx issues visible in github (#208)
- Pin actions (#211)
- Replace checkmarx.yaml with composite action (#268)
- Upgrade checkout action to latest version and pin to hash (#319)
- *(chain-indexer)* Remove hex decoding for transactions (#320)
- *(indexer-api)* Rework shielded transactions (#315)
- End to end support for ledger events (#359)
- Add dustLedgerEvents subscription (#366)
- *(indexer-api)* Support system transactions in API (#375)
- Use untagged serialization for viewing key (#378)
- Store and expose ledger parameters in GraphQL API (PM-19727, PM-19761) (#382)
- Add DUST registration tracking to UnshieldedUtxo (#391)
- Correctly use and expose byte types (#398)
- *(indexer-api)* Change API version from v1 to v3 (#418)
- *(api)* Add dustGenerationStatus query for cNIGHT tracking (#419)
- Add ctime to unshielded UTXO (#425)

### 🐛 Bug Fixes

- *(indexer-api)* Make viewing update stream infinite (#32)
- *(chain-indexer)* Reset zswap state if storage is empty (#35)
- Abstract over runtime-dependent UtxoInfo type (#80)
- *(indexer-api)* Correctly determine highest relevant index (#94)
- *(indexer-api)* Correctly determine highest relevant index for standalone (#95)
- *(justfile)* Create target/data directory before running standalone indexer (#103)
- *(wallet-indexer)* Avoid race condition saving relevant transactions (#107)
- *(indexer-api)* Ensure consistent UTXO ordering by output_index in GraphQL API (#119)
- Allow target dir to be changed (#122)
- *(wallet-indexer)* Index wallets when freshly started (#124)
- Return client error for unknown block hash in subscriptions (#172)
- Correctly determine transaction relevance (#237)
- Resolve panic when querying transaction field on ContractAction interface (#243)
- Add --wait flag to docker compose commands to prevent race condition (#244)
- Configure nextest to prevent test cancellation on failure (#255)
- Update chain-indexer to use 0.13.5 metadata for node-dev-01 compatibility (#275)
- Correctly determine transaction relevance (#313)
- *(chain-indexer)* Fetch authorities for historic block (#318)
- Correct node version extraction in GitHub Actions workflow (#326)
- Restore genesis UTXO aggregation for test compatibility (#350)
- Allow manual kick off of repo (#351)
- Remove incorrect assertion on stream ordering in chain-indexer (#368)
- Handle SystemTransactionApplied events from MidnightSystem pallet (#381)
- Use same intent hash for ClaimRewards as ledger (#400)
- Use correct intent hash for spent UTXOs (#402)
- Skip UTXO creation for failed transactions (#401)
- *(chain-indexer)* Create unshielded UTXOs from system transactions (#408)

### 💼 Other

- Add support for .envrc.local (#254)
- Simplify node update process (#322)
- Add cargo-deny SARIF output to security scanning (#370)

### 🚜 Refactor

- *(wallet-indexer)* Minimize storage access (#42)
- Simpler handling of zswap state (#46)
- Remove subxt dependency from domain (#51)
- Remove redundant unshielded UTXO handling from storage (#64)
- Split indexer-api Storage into smaller parts (#69)
- *(indexer-api)* Move NoopStorage impls to respective submodules (#76)
- Align UnshieldedUtxoStorage with other storage traits (#74)
- *(indexer-api)* Break api/v1 module into smaller submodules (#143)
- Storage unification (#171)
- Remove unused code, better naming, etc. (#232)
- Rename GraphQL field from parameters to ledgerParameters (#384)

### 📚 Documentation

- Add "Running" section to README (#24)
- Add Development Setup section to README (#61)
- Correct misleading documentation in unshielded subscription (#85)
- Update the API documentation with transaction fees and unshielded progress tracking (#88)
- Update GraphQL API documentation to match current schema (#176)
- Add missing requirements to README (#253)
- Add comprehensive guide for updating node versions (#281)

### ⚙️ Miscellaneous Tasks

- Add more and consistent tracing and logging (#27)
- Add logging related to caught-up state (#53)
- Some code hygiene (#63)
- *(chain-indexer)* Improve some code style (#105)
- Remove obsolete TryFrom byte slice impl for ViewingKey (#126)
- Commit staged changes across repos (#282)
- Update midnight-node to 0.16.0-da0b6c69 (#309)
- *(cleanup)* Use where clauses for trait bounds where possible (#321)
- Improve node update process robustness (#323)
- Upgrade upload-sarif-github-action to use Checkmarx CLI v2.3.35 (#357)
- *(chain-indexer)* Cleanup SubxtNode error handling (#364)
- Cleanup ledger parameter implementation (#387)
- Some code hygiene (#404)
- *(indexer-api)* Cleanup storage implementation (#406)
- Remove unnecessary clone in ledger event storage (#420)
- Enable TLS for PostgreSQL (#422)

## [2.1.2] - 2025-05-27

### 🐛 Bug Fixes

- *(indexer-api)* Make viewing update stream infinite (#32)

### ⚙️ Miscellaneous Tasks

- Add more and consistent tracing and logging (#27)

## [2.1.1] - 2025-05-19

### 🐛 Bug Fixes

- *(indexer-api)* Queries return correct transactions and contract actions (#15)

### 🚜 Refactor

- *(chain-indexer)* Easier, more idiomatic way to apply transactions (#10)

## [2.1.0] - 2025-05-09

### 🚀 Features

- *(indexer-api)* Add permissive CORS middleware (#635)

## [2.0.0] - 2025-05-08

### 🚀 Features

- Bump node to 0.12 and ledger to 4.0 (#537)
- *(indexer-api)* Redesign wallet subscription ProgressUpdates (#591)
- *(indexer-api)* Add tracing to API (#597)
- *(indexer-api)* Add counters for all GraphQL operations (#603)
- *(indexer-api)* Clean naming and inputs (#617)
- Only support bech32m encoded keys/addresses (#602)
- *(indexer-api)* Rename contract query and subscription contract_action (#625)

### 🐛 Bug Fixes

- *(indexer-api)* Add missing logging for wallet subscription (#575)
- Add missing error logging for loading config (#577)
- Remove common-macro from Dockerfiles, update to Rust 1.86.0 (#583)
- *(indexer-api)* Skip collapsed update for failed transactions (#588)
- *(indexer-api)* Add transaction to ContractCallOrDeploy (#608)
- *(indexer-api)* Add deploy to ContractCall (#609)
- *(wallet-indexer)* Silence harmless database error in active_wallets (#619)

### 🚜 Refactor

- Use log, logforth and fastrace for telemetry (#544)
- Replace bytes attribute with byte newtypes (#574)
- Pass SessionId (Copy) by value (#594)
- Move main/run into main.rs (#596)
- *(indexer-api)* Pass block and tx hashes (arrays) by value (#610)
- *(indexer-api)* Lazy resolving of transactions and contract actions (#611)

### 📚 Documentation

- Updates, mainly reflecting API changes (#628)
- More updates/fixes for API doc (#630)

### ⚙️ Miscellaneous Tasks

- *(indexer-api)* Apply consistent error handling (#601)
- *(indexer-api)* More debug logging (#612)

## [2.0.0] - 2025-05-08

### 🚀 Features

- Bump node to 0.12 and ledger to 4.0 (#537)
- *(indexer-api)* Redesign wallet subscription ProgressUpdates (#591)
- *(indexer-api)* Add tracing to API (#597)
- *(indexer-api)* Add counters for all GraphQL operations (#603)
- *(indexer-api)* Clean naming and inputs (#617)
- Only support bech32m encoded keys/addresses (#602)
- *(indexer-api)* Rename contract query and subscription contract_action (#625)

### 🐛 Bug Fixes

- *(indexer-api)* Add missing logging for wallet subscription (#575)
- *(indexer-api)* Skip collapsed update for failed transactions (#588)
- *(indexer-api)* Add transaction to ContractCallOrDeploy (#608)
- *(indexer-api)* Add deploy to ContractCall (#609)
- *(wallet-indexer)* Silence harmless database error in active_wallets (#619)

### 🚜 Refactor

- Use log, logforth and fastrace for telemetry (#544)
- Replace bytes attribute with byte newtypes (#574)
- Pass SessionId (Copy) by value (#594)
- Move main/run into main.rs (#596)
- *(indexer-api)* Pass block and tx hashes (arrays) by value (#610)
- *(indexer-api)* Lazy resolving of transactions and contract actions (#611)

### 📚 Documentation

- Updates, mainly reflecting API changes (#628)
- More updates/fixes for API doc (#630)

### ⚙️ Miscellaneous Tasks

- *(indexer-api)* Apply consistent error handling (#601)
- *(indexer-api)* More debug logging (#612)

## [1.0.1] - 2025-04-01

### 🐛 Bug Fixes

- *(indexer-api)* Send correct ProgressUpdates on reconnect (#531)
- Wallet subscription keeps wallet active (#510)

### 📚 Documentation

- *(adr)* Use only bech32m format for unshielded address and remove bls/hex mentions (#486)
- *(decision)* Record decision to replace Scala indexer docs with Rust indexer docs (#13933) (#427)

### ⚙️ Miscellaneous Tasks

- Rename local to standalone (#508)

## [1.0.0] - 2025-03-24

The Midnight Indexer 1.0.0 is the first release of the Rust-based indexer, replacing the previous Scala implementation. This version improves performance, modularity, and deployment flexibility. The indexer efficiently processes data from Midnight network, providing a GraphQL API for queries and real-time subscriptions.

<!-- generated by git-cliff -->
