# Test Data for indexer-standalone

This directory contains mock data for development and testing purposes.

## Contents

- `sqlite/mock-data.sh` - SQLite mock data script that populates the database with comprehensive test data (11 transactions matching PostgreSQL, cNIGHT/DUST generation scenario with DUST UTXOs and merkle trees)

## Usage

This mock data is automatically loaded by the Docker container when:
1. The database is empty (no blocks exist)
2. The service is started with the development entrypoint

The script expects the `DATABASE_FILE` environment variable to be set to the SQLite database path.

## Note

This is a temporary solution until proper integration with midnight-node and midnight-ledger is available.
The mock data should NOT be used in production environments.