Suggestions from Sean

You need to first deploy a contract:
- Deploy: generate-txs ... contract-calls deploy
- Extract address: contract-address --network [appropriate-network] --src-file deploy.mn --dest-file address.mn
- Send to chain: generate-txs --src-files deploy.mn --dest-url wss://... send
- Then make calls: generate-txs ... contract-calls call --contract-address address.mn


High level generate-tx 

midnight-node-toolkit generate-txs <SRC_ARGS> <DEST_ARGS> <PROVER_ARG> batches <BUILDER_ARGS>

Locally generate contract calls

generate-txs -s ws://127.0.0.1:9944 contract-calls call --contract-address res/test-contract/contract_address_devnet.mn