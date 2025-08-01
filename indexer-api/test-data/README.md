# Test Data for indexer-api

This directory contains mock data for development and testing purposes.

## Contents

- `postgres/mock-data.sql` - PostgreSQL mock data script that populates the database with test data (11 transactions, cNIGHT/DUST generation scenario matching midnight-explorer)

## Usage

This mock data is automatically loaded by:
1. Docker container when the database is empty (no blocks exist)
2. `just run-indexer-api` command during local development

The script includes:
- 10 blocks with proper 32-byte hashes
- 11 transactions distributed across blocks
- 5 cNIGHT registrations matching midnight-explorer test keys
- 6 DUST generation info entries
- Unshielded UTXOs and wallet data

## Note

This is a temporary solution until proper integration with midnight-node and midnight-ledger is available.
The mock data should NOT be used in production environments.