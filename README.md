# Cytah-Speed Blockchain

A high-performance, DAG-based blockchain implementation written in Rust, featuring WebAssembly smart contracts and a comprehensive developer ecosystem.

## Features

- **DAG-based Consensus**: Advanced blockDAG architecture for high throughput
- **WebAssembly Smart Contracts**: Secure, sandboxed contract execution
- **P2P Networking**: Robust peer-to-peer communication with libp2p
- **RocksDB Storage**: Persistent storage for blockchain state and contracts
- **Complete Developer Ecosystem**:
  - Rust SDK with full node capabilities
  - JavaScript SDK for browser dApps
  - Mobile SDK for iOS/Android
  - Real-time event system
  - WebSocket streaming API
  - High-performance blockchain indexer
  - Local devnet tools
- **Production Ready**: Comprehensive error handling, security, and scalability

## Architecture

```
src/
├── core/          # Core blockchain types (blocks, transactions)
├── consensus/     # DAG consensus and mining
├── network/       # P2P networking and RPC
├── storage/       # Database interfaces
├── execution/     # Transaction and contract execution
├── vm/           # WebAssembly runtime
├── wallet/       # Wallet functionality
├── rpc/          # RPC server with WebSocket support
├── events/       # Real-time event system
├── indexer/      # High-performance blockchain indexer
│   ├── block_indexer.rs
│   ├── tx_indexer.rs
│   └── address_indexer.rs
└── node/         # Node runtime

sdk/
├── rust/         # Rust SDK crate (cytah-sdk)
│   ├── src/
│   │   ├── client.rs         # RPC client
│   │   ├── wallet.rs         # Wallet management
│   │   ├── transaction.rs    # Transaction builder
│   │   ├── contract.rs       # Smart contract interface
│   │   ├── network.rs        # Network utilities
│   │   ├── crypto.rs         # Cryptographic utilities
│   │   ├── mobile/           # Mobile-optimized SDK
│   │   │   ├── mobile_client.rs
│   │   │   └── mobile_wallet.rs
│   │   ├── errors.rs         # Unified error handling
│   │   └── lib.rs
│   └── Cargo.toml
└── javascript/   # JavaScript SDK
    ├── provider/
    │   ├── provider.js
    │   ├── events.js
    │   └── wallet_bridge.js
    └── mobile/
```

## Developer Ecosystem

### SDK Components

#### Rust SDK
Full-featured SDK for desktop and server applications:

```rust
use cytah_sdk::{Client, Wallet, TransactionBuilder, EventBus, EventListener};

// Connect and interact with the blockchain
let client = Client::new("http://localhost:8080");
let wallet = Wallet::create_wallet()?;

// Real-time event subscriptions
let event_bus = EventBus::new();
let listener = EventListener::new(event_bus, "my_app".to_string());
listener.subscribe_new_blocks(|event| async move {
    println!("New block: {:?}", event);
}).await;
```

#### JavaScript SDK
Browser-compatible Web3 provider for dApps:

```javascript
// Connect to Cytah-Speed
const provider = new CytahProvider('http://localhost:8080');
const walletBridge = new CytahWalletBridge(provider);

// Connect wallet and send transactions
await walletBridge.connectWallet();
const tx = await walletBridge.signAndSendTransaction({
    to: recipient,
    amount: 1000
});
```

#### Mobile SDK
Lightweight SDK optimized for mobile devices:

```rust
use cytah_sdk::mobile::{MobileClient, MobileWallet};

let client = MobileClient::new("http://api.cytah-speed.com".to_string());
let wallet = MobileWallet::create()?;

// Efficient mobile operations with caching
let balance = wallet.get_balance(&client).await?;
```

### Real-time Events

Subscribe to blockchain activity with the event system:

```rust
use cytah_core::events::{EventBus, EventListener, EventHandler};

#[async_trait::async_trait]
impl EventHandler for MyApp {
    async fn on_new_block(&self, event: Event) { /* handle block */ }
    async fn on_new_transaction(&self, event: Event) { /* handle tx */ }
    async fn on_contract_event(&self, event: Event) { /* handle contract */ }
}
```

### WebSocket Streaming

Real-time data streaming via WebSocket:

```javascript
const ws = new WebSocket('ws://localhost:8080/events');
ws.onmessage = (event) => {
    const data = JSON.parse(event.data);
    console.log('Real-time event:', data);
};
```

### Blockchain Indexer

High-performance indexing for explorers and analytics:

```rust
use cytah_core::indexer::{BlockIndexer, TransactionIndexer, AddressIndexer};

let block_indexer = BlockIndexer::new("./data/blocks")?;
// Index and query blockchain data efficiently
```

### Devnet Tools

Local development environment:

```bash
# Start local devnet
./tools/devnet/devnet.sh start

# Spawn test nodes
./tools/devnet/devnet.sh spawn-nodes 3

# Reset network
./tools/devnet/devnet.sh reset
```

## Quick Start

### 1. Build the Project

```bash
git clone <repository-url>
cd cytah-speed
cargo build --release
```

### 2. Start Devnet

```bash
./tools/devnet/devnet.sh start
```

### 3. Create Your First App

```rust
use cytah_sdk::{Client, Wallet};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to devnet
    let client = Client::new("http://127.0.0.1:8080");

    // Create wallet
    let wallet = Wallet::create_wallet()?;
    println!("Address: {}", hex::encode(wallet.address));

    // Check balance
    let balance = client.get_balance(wallet.address).await?;
    println!("Balance: {}", balance.balance);

    Ok(())
}
```

## Documentation

- **[SDK Guide](docs/SDK_GUIDE.md)** - Complete SDK documentation and examples
- **[Ecosystem Guide](docs/ECOSYSTEM_GUIDE.md)** - Building wallets, dApps, explorers, and tools
- **[Devnet Tools](tools/devnet/README.md)** - Local development environment
- **[API Reference](docs/)** - Detailed API documentation

## Use Cases

### Wallets
- Desktop wallets with full node capabilities
- Browser extension wallets
- Mobile wallets with biometric security
- Hardware wallet integration

### dApps
- Decentralized exchanges (DEX)
- NFT marketplaces
- DeFi protocols
- Gaming applications
- Social networks

### Explorers
- Block explorers with real-time updates
- Transaction analysis tools
- Address tracking and analytics
- Network monitoring dashboards

### Analytics & Tools
- Transaction volume analysis
- Gas usage optimization
- Network health monitoring
- Development debugging tools

## Performance

- **Throughput**: 1000+ TPS with DAG consensus
- **Latency**: Sub-second block finality
- **Storage**: Efficient RocksDB with indexing
- **Networking**: Optimized P2P with libp2p
- **Smart Contracts**: WebAssembly execution with gas metering

## Security

- **Cryptography**: secp256k1/ECDSA with secure key generation
- **Consensus**: DAG-based security with economic incentives
- **Smart Contracts**: Sandboxed WASM execution
- **Networking**: Encrypted P2P communication
- **Storage**: Tamper-proof data persistence

## Roadmap

### Phase 1 (Current)
- ✅ Core blockchain implementation
- ✅ WebAssembly smart contracts
- ✅ Complete developer SDK ecosystem
- ✅ Real-time event system
- ✅ WebSocket streaming
- ✅ Blockchain indexer
- ✅ Devnet tools

### Phase 2 (Q1 2026)
- Mainnet launch
- Enhanced mobile SDK
- Cross-chain bridges
- Advanced analytics

### Phase 3 (Q2 2026)
- Enterprise features
- Improved tooling
- Mobile wallet apps
- Ecosystem expansion

## Contributing

We welcome contributions! See our [Contributing Guide](CONTRIBUTING.md) for details.

### Development Setup

```bash
# Clone repository
git clone <repository-url>
cd cytah-speed

# Run tests
cargo test

# Check code quality
cargo clippy
cargo fmt

# Build documentation
cargo doc --open
```

### Areas for Contribution

- **SDK Improvements**: Enhanced APIs and better developer experience
- **Performance Optimization**: Faster consensus, better indexing
- **Security Audits**: Code review and security enhancements
- **Documentation**: Guides, tutorials, and examples
- **Tools**: Development tools, monitoring, analytics
- **Integrations**: Third-party service integrations

## Community

- **Discord**: Join our developer community
- **Forum**: Technical discussions and support
- **GitHub**: Issues, feature requests, and contributions
- **Blog**: Updates, tutorials, and announcements

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

Built with ❤️ using Rust and inspired by the best practices from Ethereum, Solana, and other leading blockchains.

---

**Ready to build the future of blockchain?** 🚀

Join the Cytah-Speed ecosystem and start building decentralized applications today!
6. Submit a pull request

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Roadmap

- [ ] Mainnet launch
- [ ] Additional consensus mechanisms
- [ ] Cross-chain bridges
- [ ] Enhanced smart contract features
- [ ] Mobile SDKs
- [ ] Hardware wallet support

## Support

- [Documentation](docs/)
- [Issues](https://github.com/your-org/cytah-speed/issues)
- [Discussions](https://github.com/your-org/cytah-speed/discussions)