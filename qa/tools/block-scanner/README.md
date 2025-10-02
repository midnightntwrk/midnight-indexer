# Block Scanner

This is a Bun-based implementation of the block scanner for the Midnight Network block chain. It provides the ability to scan a Midtnight blockchain using the Midnight Indexer. It collects all the blocks with some information (discarding the empty blocks) and stores the transactions in a data file, useful for later processing.

## Use case

Right now there are 2 use cases for this tool:
1. It gives the ability to scan the chain for data
2. It is used to prepare test data for the QA tests that target the Midnight Indexer

## Prerequisites

- [Bun](https://bun.sh) installed on your system
- Access to a Midnight Network indexer endpoint (or local/undeployed environment)

## Installation

```bash
# Install dependencies
bun install
```

## Usage

### Basic Usage

```bash
# Run the scanner (defaults to undeployed environment)
bun run src/scanner.ts

# Run with specific environment
TARGET_ENV=devnet bun run src/scanner.ts

# Run with test data folder
bun run src/scanner.ts /path/to/test/data/folder
```

### Available Environments

- `undeployed` - Local development (ws://localhost:8088/api/v1/graphql/ws)
- `nodedev01` - Node dev environment
- `devnet` - Development network
- `qanet` - QA network
- `testnet02` - Test network

### Scripts

The following scripts are useful for development purpose
```bash
# Clean build artifacts
bun run clean

# Format code
bun run format

# Audit dependencies
bun run audit
```

These other scripts are needed to build and run the scanner
```bash
# Build the scanner
bun run build:scanner

# Run the scanner (default will target undeployed)
bun run run:scanner

# Run the scanner against the desired target environment
TARGET_ENV=undeployed bun run run:scanner
```

## Configuration

The scanner uses the same configuration as the original version:

- Environment variables for target environment
- WebSocket URLs for different networks
- GraphQL subscription queries
- File output in JSONL format

## Output

The scanner creates:
- `tmp_scan/` directory for temporary files
- `{TARGET_ENV}_blocks.jsonl` - Raw block data
- `contracts-actions.json` - Processed contract actions (if test data folder provided)

## Troubleshooting

### Common Issues

1. **WebSocket Connection Issues**: Ensure the indexer endpoint is accessible
2. **Permission Errors**: Make sure you have write permissions for the output directory
3. **Memory Issues**: For large datasets, consider processing in smaller batches

### Debug Mode

Enable debug logging by setting the log level:

```bash
DEBUG=* bun run src/scanner.ts
```

## Performance Notes

- Bun typically provides 2-3x performance improvement over Node.js
- Memory usage is generally lower
- Startup time is significantly faster
- Built-in bundling eliminates the need for separate build steps

## Contributing

When making changes:
1. Ensure code follows the existing patterns
2. Test with multiple environments
3. Update documentation as needed
4. Consider performance implications
