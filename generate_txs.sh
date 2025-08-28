#!/usr/bin/env bash

# 1 to 2/2
midnight-node-toolkit generate-txs \
    --dest-file ./target/tx_1_2_2.mn \
    --to-bytes \
    single-tx \
    --shielded-amount 10 \
    --unshielded-amount 10 \
    --source-seed "0000000000000000000000000000000000000000000000000000000000000001" \
    --destination-address mn_shield-addr_undeployed1tffkxdesnqz86wvds2aprwuprpvzvag5t3mkveddr33hr7xyhlhqxqzfqqxy54an7cyznaxnzs7p8tduku7fuje5mwqx9auvdn9e8x03kvvy5r6z \
    --destination-address mn_addr_undeployed1gkasr3z3vwyscy2jpp53nzr37v7n4r3lsfgj6v5g584dakjzt0xqun4d4r

midnight-node-toolkit get-tx-from-context \
    --src-file ./target/tx_1_2_2.mn \
    --network undeployed \
    --dest-file ./target/tx_1_2_2.raw \
    --from-bytes

# 1 to 2/3
midnight-node-toolkit generate-txs \
    --dest-file ./target/tx_1_2_3.mn \
    --to-bytes \
    single-tx \
    --shielded-amount 10 \
    --unshielded-amount 10 \
    --source-seed "0000000000000000000000000000000000000000000000000000000000000001" \
    --destination-address mn_shield-addr_undeployed1tffkxdesnqz86wvds2aprwuprpvzvag5t3mkveddr33hr7xyhlhqxqzfqqxy54an7cyznaxnzs7p8tduku7fuje5mwqx9auvdn9e8x03kvvy5r6z \
    --destination-address mn_addr_undeployed1g9nr3mvjcey7ca8shcs5d4yjndcnmczf90rhv4nju7qqqlfg4ygs0t4ngm

midnight-node-toolkit get-tx-from-context \
    --src-file ./target/tx_1_2_3.mn \
    --network undeployed \
    --dest-file ./target/tx_1_2_3.raw \
    --from-bytes
