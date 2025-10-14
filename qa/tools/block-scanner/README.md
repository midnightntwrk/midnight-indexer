# Block Scanner

This is a Bun-based implementation of the block scanner for the midnight network block chain. It provides the ability to scan a midnight blockchain using the midnight Indexer. It collects all the blocks with some information (discarding the empty blocks) and stores the transactions in a data file, useful for later processing.

## Use case

Right now there are 2 use cases for this tool:
1. It gives the ability to scan the chain for data
2. It is used to prepare test data for the QA tests that target the midnight Indexer

## Prerequisites

- [Bun](https://bun.sh) installed on your system
- Access to a midnight network indexer endpoint (or undeployed environment setup locally)

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

# Run to generate test data for the indexer tests
bun run src/scanner.ts <path/to/test/data/folder>
```

### Available Environments (these might change in future)

- `undeployed` - Local development (ws://localhost:8088/api/v3/graphql/ws)
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
bun run scan

# Run the scanner against the selected target environment
TARGET_ENV=undeployed bun run scan
```

## Configuration

The scanner uses this information as configuration input:

- Environment variables for target environment (`TARGET_ENV`, defaults to `undeployed`)

other information are provided already, but could require future updates based on the midnight indexer and midnight network evolution
- WebSocket URLs for different networks (automatically extracted from the target environment)
- GraphQL subscription queries (available in a separate GraphQL file)


## Output

The scanner creates:
- `tmp_scan/` directory for temporary files
- `{TARGET_ENV}_blocks.jsonl` - Raw block data stored for post processing
- `*.jsonc` - A number of jsonc files created from the templates which are needed as Indexer test data

## Troubleshooting

### Common Issues

1. **WebSocket Connection Issues**: Ensure the indexer endpoint is accessible
2. **Permission Errors**: Make sure you have write permissions for the output directory
3. **Memory Issues**: For large datasets, consider processing in smaller batches

### Debug Mode

Enable debug logging by setting the log level:

```bash
DEBUG=* bun run scan
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
