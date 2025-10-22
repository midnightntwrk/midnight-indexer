# DUST Generation Scripts

## Overview
DUST generation is **automatic** when wallets have Night tokens. No registration is needed.

## Required Scripts

### 1. `toolkit-start`
Starts the toolkit container with the correct version.
```bash
bash qa/scripts/mn-toolkit/toolkit-start undeployed 0.17.1-47a8ea28
```

### 2. `toolkit-fund-wallet`
Funds a wallet with Night tokens to enable DUST generation.
```bash
bash qa/scripts/mn-toolkit/toolkit-fund-wallet undeployed <wallet-seed> <amount> <source-seed> 0.17.1-47a8ea28
```

### 3. `toolkit-show-wallet`
Shows wallet state including DUST generation data.
```bash
bash qa/scripts/mn-toolkit/toolkit-show-wallet undeployed <wallet-seed> 0.17.1-47a8ea28
```

## Complete DUST Workflow

1. **Fund wallet with Night tokens:**
   ```bash
   bash qa/scripts/mn-toolkit/toolkit-fund-wallet undeployed 0000000000000000000000000000000000000000000000000000000000000009 10000000 0000000000000000000000000000000000000000000000000000000000000001 0.17.1-47a8ea28
   ```

2. **Check DUST generation status:**
   ```bash
   bash qa/scripts/mn-toolkit/toolkit-show-wallet undeployed 0000000000000000000000000000000000000000000000000000000000000009 0.17.1-47a8ea28
   ```

## Key Points

- **DUST is automatic** - no registration command needed
- **Version compatibility is critical** - use `0.17.1-47a8ea28`
- **DUST generation begins immediately** after funding
- **Check wallet state** to see DUST generation data
