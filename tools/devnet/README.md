# Cytah-Speed Devnet Tools

Local development network tools for Cytah-Speed blockchain development.

## Overview

The devnet tools provide a complete local development environment for building and testing Cytah-Speed applications. It includes commands to start/stop/reset a local network, spawn test nodes, and manage test accounts.

## Quick Start

```bash
# Start a local devnet
./tools/devnet/devnet.sh start

# Check status
./tools/devnet/devnet.sh status

# Reset the network
./tools/devnet/devnet.sh reset

# Stop the devnet
./tools/devnet/devnet.sh stop
```

## Commands

### Basic Network Management

- `start` - Start the devnet with default configuration
- `stop` - Stop the running devnet
- `reset` - Stop devnet and clear all data
- `status` - Check if devnet is running

### Advanced Features

- `mint-tokens` - Mint test tokens to configured accounts
- `spawn-nodes <count>` - Spawn additional test nodes (default: 2)
- `stop-nodes` - Stop all test nodes
- `logs` - Show devnet logs in real-time

## Configuration

The devnet creates a `devnet/` directory in the project root with:

- `config.toml` - Devnet configuration
- `devnet.log` - Main log file
- `node.log` - Node-specific logs
- `data/` - Blockchain data directory

### Example Configuration

```toml
[network]
listen_addr = "/ip4/127.0.0.1/tcp/0"
bootstrap_peers = []

[rpc]
enabled = true
address = "127.0.0.1:8080"

[consensus]
mining_enabled = true
devnet_mode = true

[devnet]
data_dir = "./devnet/data"
test_accounts = [
    { address = "0x1234567890123456789012345678901234567890", balance = "1000000" },
    { address = "0x0987654321098765432109876543210987654321", balance = "500000" }
]
```

## Test Accounts

The devnet comes pre-configured with test accounts that have initial balances:

- `0x1234567890123456789012345678901234567890` - 1,000,000 tokens
- `0x0987654321098765432109876543210987654321` - 500,000 tokens

## Multi-Node Testing

For testing network features, spawn multiple nodes:

```bash
# Spawn 3 additional test nodes
./tools/devnet/devnet.sh spawn-nodes 3

# Check all nodes
./tools/devnet/devnet.sh status
```

Each test node will have its own:
- RPC endpoint (8081, 8082, 8083, etc.)
- Data directory
- Log file

## Development Workflow

1. **Setup**: `./tools/devnet/devnet.sh start`
2. **Develop**: Build your dApp using the SDK
3. **Test**: Deploy contracts, send transactions
4. **Debug**: Check logs with `./tools/devnet/devnet.sh logs`
5. **Reset**: `./tools/devnet/devnet.sh reset` when needed
6. **Cleanup**: `./tools/devnet/devnet.sh stop`

## Integration with SDK

The devnet works seamlessly with the Cytah-Speed SDK:

```rust
use cytah_core::sdk::Client;

// Connect to devnet
let client = Client::new("http://127.0.0.1:8080");

// Use SDK as normal
let balance = client.get_balance(wallet.address).await?;
```

## Troubleshooting

### Devnet Won't Start

- Check if port 8080 is available
- Ensure the project is built: `cargo build --release`
- Check logs: `./tools/devnet/devnet.sh logs`

### Connection Issues

- Verify devnet is running: `./tools/devnet/devnet.sh status`
- Check RPC endpoint configuration
- Review firewall settings

### Data Corruption

- Reset the devnet: `./tools/devnet/devnet.sh reset`
- Check available disk space

## Advanced Usage

### Custom Configuration

Edit `devnet/config.toml` before starting to customize:

- Network ports
- Mining settings
- Test account balances
- Data directory location

### Environment Variables

- `CYTAH_DEVNET_PORT` - Override default RPC port
- `CYTAH_DEVNET_DATA_DIR` - Custom data directory
- `CYTAH_DEVNET_LOG_LEVEL` - Set log verbosity

### Integration Testing

Use the devnet in CI/CD pipelines:

```yaml
# Example GitHub Actions
- name: Start Devnet
  run: ./tools/devnet/devnet.sh start

- name: Run Tests
  run: cargo test --features integration

- name: Stop Devnet
  run: ./tools/devnet/devnet.sh stop
```

## Contributing

When adding new devnet features:

1. Update this README
2. Add tests for new commands
3. Follow the existing script structure
4. Test on multiple platforms

## Support

- Check logs: `./tools/devnet/devnet.sh logs`
- View configuration: `cat devnet/config.toml`
- Reset if issues: `./tools/devnet/devnet.sh reset`