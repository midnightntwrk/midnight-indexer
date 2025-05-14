# Wallet Indexer

The Wallet Indexer is a stream processor associating and storing wallets with relevant transactions.

It receives Pub/Sub messages from two channels, one for wallet connections ane one for transactions. For wallet connections the message holds the viewing key for a connected wallet, for transactions the message is just an empty signal.

When a viewing key is received, its session ID is derived, the wallet is marked "Active" in the database and the session ID is pushed downstream. When a transaction is signaled, the session IDs of all active wallets are loaded from the database and pushed downstream.

For each session ID, all transactions in a batch starting after the one that was last processed (for the respective session ID) are checked whether they are relevant for the respective wallet. All relevant transactions are then saved in the database.
