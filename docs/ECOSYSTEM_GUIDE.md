# Cytah-Speed Ecosystem Guide

Welcome to the Cytah-Speed blockchain ecosystem! This comprehensive guide covers everything you need to build applications on Cytah-Speed, from wallets and dApps to explorers and analytics tools.

## Table of Contents

1. [Ecosystem Overview](#ecosystem-overview)
2. [Building Wallets](#building-wallets)
3. [Creating dApps](#creating-dapps)
4. [Smart Contract Development](#smart-contract-development)
5. [Running a Devnet](#running-a-devnet)
6. [Event System](#event-system)
7. [Building Explorers](#building-explorers)
8. [Analytics & Tools](#analytics--tools)
9. [Mobile Development](#mobile-development)
10. [Best Practices](#best-practices)

## Ecosystem Overview

Cytah-Speed provides a complete developer ecosystem:

### Core Components
- **Rust SDK** - Full-featured SDK for Rust applications
- **JavaScript SDK** - Browser-compatible Web3 provider
- **Mobile SDK** - Lightweight mobile-optimized SDK
- **Event System** - Real-time blockchain event subscriptions
- **WebSocket RPC** - Streaming API for real-time data
- **Blockchain Indexer** - High-performance data indexing
- **Devnet Tools** - Local development environment

### Supported Platforms
- **Web Browsers** - Via JavaScript SDK and Web3 provider
- **Desktop** - Rust SDK with full node capabilities
- **Mobile** - iOS/Android via mobile SDK
- **Backend** - Server-side applications and APIs

## Building Wallets

### Rust Wallet

```rust
use cytah_core::sdk::{Wallet, Client, TransactionBuilder};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create wallet
    let wallet = Wallet::create_wallet()?;

    // Connect to network
    let client = Client::new("http://localhost:8080");

    // Check balance
    let balance = client.get_balance(wallet.address).await?;
    println!("Balance: {}", balance.balance);

    // Send transaction
    let recipient = hex::decode("recipient_address")?;
    let mut recipient_array = [0u8; 20];
    recipient_array.copy_from_slice(&recipient);

    let tx = TransactionBuilder::new()
        .from(wallet.address)
        .transfer(recipient_array, 1000)
        .nonce(1)
        .gas_limit(21000)
        .gas_price(10)
        .build_and_sign(&wallet)?;

    let tx_hash = client.broadcast_transaction(&tx).await?;
    println!("Transaction sent: {}", tx_hash);

    Ok(())
}
```

### JavaScript Wallet

```javascript
// Connect to Cytah-Speed
const provider = new CytahProvider('http://localhost:8080');

// Create wallet
const wallet = CytahWallet.createWallet();
console.log('Address:', wallet.address);

// Connect wallet extension (if available)
const walletBridge = new CytahWalletBridge(provider);
await walletBridge.connectWallet();

// Send transaction
const tx = {
    from: wallet.address,
    to: recipientAddress,
    amount: 1000,
    nonce: 1,
    gasLimit: 21000,
    gasPrice: 10
};

const signedTx = await walletBridge.signAndSendTransaction(tx);
console.log('Transaction hash:', signedTx.hash);
```

### Mobile Wallet

```rust
use cytah_core::sdk::mobile::{MobileClient, MobileWallet};

#[tokio::main]
async fn mobile_wallet_example() -> Result<(), Box<dyn std::error::Error>> {
    // Create mobile wallet
    let wallet = MobileWallet::create()?;

    // Connect to mobile client
    let client = MobileClient::new("http://localhost:8080".to_string());

    // Send tokens (convenience method)
    let recipient = [1u8; 20]; // recipient address
    let tx_hash = wallet.send_tokens(
        &client,
        recipient,
        1000, // amount
        10,   // gas price
    ).await?;

    println!("Transaction sent: {}", tx_hash);
    Ok(())
}
```

## Creating dApps

### Browser dApp with JavaScript SDK

```html
<!DOCTYPE html>
<html>
<head>
    <title>Cytah-Speed dApp</title>
    <script src="cytah-provider.js"></script>
    <script src="cytah-events.js"></script>
    <script src="cytah-wallet-bridge.js"></script>
</head>
<body>
    <h1>My Cytah-Speed dApp</h1>
    <button id="connect">Connect Wallet</button>
    <div id="balance"></div>
    <button id="send">Send Tokens</button>

    <script>
        const provider = new CytahProvider('http://localhost:8080');
        const walletBridge = new CytahWalletBridge(provider);
        const events = new CytahEvents(provider);

        // Connect wallet
        document.getElementById('connect').onclick = async () => {
            try {
                await walletBridge.connectWallet();
                console.log('Wallet connected!');
                updateBalance();
            } catch (error) {
                console.error('Failed to connect wallet:', error);
            }
        };

        // Update balance
        async function updateBalance() {
            if (walletBridge.isConnected()) {
                const balance = await walletBridge.getBalance();
                document.getElementById('balance').textContent =
                    `Balance: ${balance}`;
            }
        }

        // Send tokens
        document.getElementById('send').onclick = async () => {
            const tx = {
                from: walletBridge.getAccounts()[0],
                to: '0xrecipient_address',
                amount: 100,
                nonce: 1,
                gasLimit: 21000,
                gasPrice: 10
            };

            try {
                const result = await walletBridge.signAndSendTransaction(tx);
                console.log('Transaction sent:', result);
                updateBalance();
            } catch (error) {
                console.error('Transaction failed:', error);
            }
        };

        // Listen for new blocks
        events.onNewBlock((event) => {
            console.log('New block:', event.data);
        });

        // Listen for transactions
        events.onTransaction((event) => {
            console.log('New transaction:', event.data);
        });
    </script>
</body>
</html>
```

### Rust dApp Backend

```rust
use cytah_core::sdk::{Client, EventBus, EventListener, EventHandler};
use std::sync::Arc;

#[derive(Clone)]
struct MyDApp {
    client: Client,
    event_count: Arc<std::sync::atomic::AtomicUsize>,
}

#[async_trait::async_trait]
impl EventHandler for MyDApp {
    async fn on_new_block(&self, event: cytah_core::sdk::events::Event) {
        let count = self.event_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        println!("Block #{}: {:?}", count, event);
    }

    async fn on_new_transaction(&self, event: cytah_core::sdk::events::Event) {
        println!("New transaction: {:?}", event);
    }

    async fn on_contract_event(&self, event: cytah_core::sdk::events::Event) {
        println!("Contract event: {:?}", event);
    }

    async fn on_peer_event(&self, event: cytah_core::sdk::events::Event) {
        println!("Peer event: {:?}", event);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new("http://localhost:8080");
    let event_bus = Arc::new(EventBus::new());

    let dapp = MyDApp {
        client: client.clone(),
        event_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };

    // Set up event listeners
    let listener = EventListener::new(event_bus.clone(), "my_dapp".to_string());
    let _subscriptions = cytah_core::sdk::events::create_listener_from_handler(
        event_bus,
        "my_dapp".to_string(),
        dapp,
    ).await;

    // Publish some test events (in real usage, events come from the node)
    let test_event = cytah_core::sdk::events::Event::new_block(
        cytah_core::core::Block::default(),
        1,
        "test".to_string(),
    );
    event_bus.publish(test_event).await;

    // Keep the application running
    tokio::signal::ctrl_c().await?;
    Ok(())
}
```

## Smart Contract Development

### Writing Contracts

Cytah-Speed uses WebAssembly (WASM) for smart contracts. Here's a simple token contract:

```rust
// contracts/token.rs
use cytah_core::vm::host_functions::*;

#[no_mangle]
pub extern "C" fn init() {
    // Initialize contract
    storage_write(b"total_supply", &1000000u64.to_le_bytes());
}

#[no_mangle]
pub extern "C" fn transfer() {
    let caller = get_caller();
    let args = get_args();

    // Parse arguments: recipient (20 bytes) + amount (8 bytes)
    if args.len() != 28 {
        revert();
    }

    let recipient = &args[0..20];
    let amount = u64::from_le_bytes(args[20..28].try_into().unwrap());

    // Check balance
    let caller_balance_key = [b"balance:", &caller[..]].concat();
    let caller_balance = storage_read(&caller_balance_key)
        .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
        .unwrap_or(0);

    if caller_balance < amount {
        revert();
    }

    // Update balances
    let recipient_balance_key = [b"balance:", &recipient[..]].concat();
    let recipient_balance = storage_read(&recipient_balance_key)
        .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
        .unwrap_or(0);

    storage_write(&caller_balance_key, &(caller_balance - amount).to_le_bytes());
    storage_write(&recipient_balance_key, &(recipient_balance + amount).to_le_bytes());
}
```

### Deploying Contracts

```rust
use cytah_core::sdk::{Client, Wallet, ContractClient, TransactionBuilder};

#[tokio::main]
async fn deploy_contract() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new("http://localhost:8080");
    let wallet = Wallet::create_wallet()?;
    let contract_client = ContractClient::new("http://localhost:8080");

    // Load WASM contract
    let wasm_code = std::fs::read("contracts/target/wasm32-unknown-unknown/release/token.wasm")?;

    // Deploy contract
    let result = contract_client.deploy(
        wallet.address,
        1, // nonce
        wasm_code,
        None, // no init args
    ).await?;

    println!("Contract deployed at: {}", result.contract_address);
    println!("Transaction hash: {}", result.tx_hash);

    Ok(())
}
```

### Calling Contracts

```rust
use cytah_core::sdk::ContractClient;

#[tokio::main]
async fn call_contract() -> Result<(), Box<dyn std::error::Error>> {
    let contract_client = ContractClient::new("http://localhost:8080");

    // Transfer tokens
    let recipient = [1u8; 20]; // recipient address
    let amount = 1000u64;

    let mut args = Vec::new();
    args.extend_from_slice(&recipient);
    args.extend_from_slice(&amount.to_le_bytes());

    let result = contract_client.call(
        wallet.address,
        2, // nonce
        "contract_address".to_string(),
        "transfer".to_string(),
        Some(args),
    ).await?;

    println!("Transfer result: {:?}", result);
    Ok(())
}
```

## Running a Devnet

The devnet provides a local development environment:

```bash
# Start devnet
./tools/devnet/devnet.sh start

# Check status
./tools/devnet/devnet.sh status

# View logs
./tools/devnet/devnet.sh logs

# Reset network
./tools/devnet/devnet.sh reset

# Stop devnet
./tools/devnet/devnet.sh stop
```

### Advanced Devnet Usage

```bash
# Spawn multiple test nodes
./tools/devnet/devnet.sh spawn-nodes 3

# Mint test tokens
./tools/devnet/devnet.sh mint-tokens

# Stop all test nodes
./tools/devnet/devnet.sh stop-nodes
```

## Event System

### Real-time Event Subscriptions

```rust
use cytah_core::sdk::{EventBus, EventListener, EventHandler};
use std::sync::Arc;

#[derive(Clone)]
struct EventProcessor;

#[async_trait::async_trait]
impl EventHandler for EventProcessor {
    async fn on_new_block(&self, event: cytah_core::sdk::events::Event) {
        println!("New block: {:?}", event);
    }

    async fn on_new_transaction(&self, event: cytah_core::sdk::events::Event) {
        println!("New transaction: {:?}", event);
    }

    async fn on_contract_event(&self, event: cytah_core::sdk::events::Event) {
        println!("Contract event: {:?}", event);
    }

    async fn on_peer_event(&self, event: cytah_core::sdk::events::Event) {
        println!("Peer event: {:?}", event);
    }
}

#[tokio::main]
async fn event_example() -> Result<(), Box<dyn std::error::Error>> {
    let event_bus = Arc::new(EventBus::new());
    let processor = EventProcessor {};

    // Create listener and subscribe to all events
    let subscriptions = cytah_core::sdk::events::create_listener_from_handler(
        event_bus.clone(),
        "my_app".to_string(),
        processor,
    ).await;

    println!("Subscribed to {} event types", subscriptions.len());

    // In a real application, events would come from the node
    // Here we simulate an event
    let test_event = cytah_core::sdk::events::Event::new_block(
        cytah_core::core::Block::default(),
        1,
        "node1".to_string(),
    );

    event_bus.publish(test_event).await;

    // Keep running to process events
    tokio::signal::ctrl_c().await?;
    Ok(())
}
```

### WebSocket Streaming

```javascript
// Connect to WebSocket events
const ws = new WebSocket('ws://localhost:8080/events');

ws.onopen = () => {
    console.log('Connected to Cytah-Speed events');

    // Subscribe to specific events
    ws.send(JSON.stringify({
        type: 'Subscribe',
        data: { event_types: ['new_block', 'new_transaction'] }
    }));
};

ws.onmessage = (event) => {
    const message = JSON.parse(event.data);

    if (message.type === 'Event') {
        const blockchainEvent = message.data;
        console.log('Received event:', blockchainEvent);
    }
};

ws.onclose = () => {
    console.log('WebSocket connection closed');
};
```

## Building Explorers

### Using the Blockchain Indexer

```rust
use cytah_core::indexer::{BlockIndexer, TransactionIndexer, AddressIndexer};

#[tokio::main]
async fn explorer_example() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize indexers
    let block_indexer = BlockIndexer::new("./data/blocks")?;
    let tx_indexer = TransactionIndexer::new("./data/transactions")?;
    let address_indexer = AddressIndexer::new("./data/addresses")?;

    // Index some data (normally done by background process)
    let block = cytah_core::core::Block::default();
    block_indexer.index_block(&block)?;

    // Query indexed data
    let block_data = block_indexer.get_block(&block.hash())?;
    println!("Block found: {:?}", block_data.is_some());

    // Get top addresses
    let top_addresses = address_indexer.get_top_addresses_by_transactions(10)?;
    println!("Top addresses: {}", top_addresses.len());

    Ok(())
}
```

### Building an Explorer API

```rust
use axum::{Router, routing::get, extract::Path};
use cytah_core::indexer::{BlockIndexer, TransactionIndexer, AddressIndexer};
use std::sync::Arc;

#[derive(Clone)]
struct ExplorerState {
    block_indexer: Arc<BlockIndexer>,
    tx_indexer: Arc<TransactionIndexer>,
    address_indexer: Arc<AddressIndexer>,
}

async fn get_block(
    Path(height): Path<u64>,
    state: axum::extract::State<ExplorerState>,
) -> impl axum::response::IntoResponse {
    match state.block_indexer.get_block_by_height(height) {
        Ok(Some(block)) => axum::Json(serde_json::to_value(&block).unwrap()),
        Ok(None) => axum::http::StatusCode::NOT_FOUND.into_response(),
        Err(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn get_transaction(
    Path(tx_hash): Path<String>,
    state: axum::extract::State<ExplorerState>,
) -> impl axum::response::IntoResponse {
    let hash_bytes = hex::decode(&tx_hash).unwrap_or_default();
    match state.tx_indexer.get_transaction(&hash_bytes) {
        Ok(Some(tx)) => axum::Json(serde_json::to_value(&tx).unwrap()),
        Ok(None) => axum::http::StatusCode::NOT_FOUND.into_response(),
        Err(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn get_address_stats(
    Path(address): Path<String>,
    state: axum::extract::State<ExplorerState>,
) -> impl axum::response::IntoResponse {
    let addr_bytes = hex::decode(&address).unwrap_or_default();
    let mut addr_array = [0u8; 20];
    addr_array.copy_from_slice(&addr_bytes);

    match state.address_indexer.get_address_info(&addr_array) {
        Ok(Some(info)) => axum::Json(serde_json::to_value(&info).unwrap()),
        Ok(None) => axum::http::StatusCode::NOT_FOUND.into_response(),
        Err(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[tokio::main]
async fn run_explorer() -> Result<(), Box<dyn std::error::Error>> {
    let state = ExplorerState {
        block_indexer: Arc::new(BlockIndexer::new("./data/blocks")?),
        tx_indexer: Arc::new(TransactionIndexer::new("./data/transactions")?),
        address_indexer: Arc::new(AddressIndexer::new("./data/addresses")?),
    };

    let app = Router::new()
        .route("/block/:height", get(get_block))
        .route("/transaction/:tx_hash", get(get_transaction))
        .route("/address/:address", get(get_address_stats))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;

    Ok(())
}
```

## Analytics & Tools

### Transaction Analytics

```rust
use cytah_core::indexer::TransactionIndexer;

struct TransactionAnalyzer {
    tx_indexer: TransactionIndexer,
}

impl TransactionAnalyzer {
    async fn analyze_network_activity(&self) -> Result<NetworkStats, Box<dyn std::error::Error>> {
        let total_txs = self.tx_indexer.get_transaction_count()?;

        // Analyze transaction patterns
        // This would include more sophisticated analysis

        Ok(NetworkStats {
            total_transactions: total_txs,
            average_tx_per_block: 0.0, // Calculate from block data
            unique_addresses: 0,       // Calculate from address indexer
        })
    }
}

struct NetworkStats {
    total_transactions: u64,
    average_tx_per_block: f64,
    unique_addresses: u64,
}
```

### Gas Analytics

```rust
use cytah_core::indexer::BlockIndexer;

struct GasAnalyzer {
    block_indexer: BlockIndexer,
}

impl GasAnalyzer {
    async fn analyze_gas_usage(&self) -> Result<GasStats, Box<dyn std::error::Error>> {
        // Analyze gas usage patterns across blocks
        // This would query multiple blocks and calculate statistics

        Ok(GasStats {
            average_gas_price: 0,
            total_gas_used: 0,
            gas_efficiency: 0.0,
        })
    }
}

struct GasStats {
    average_gas_price: u64,
    total_gas_used: u64,
    gas_efficiency: f64,
}
```

## Mobile Development

### iOS/Android Integration

The mobile SDK is optimized for resource-constrained environments:

```rust
use cytah_core::sdk::mobile::{MobileClient, MobileWallet};

#[tokio::main]
async fn mobile_app_logic() -> Result<(), Box<dyn std::error::Error>> {
    // Lightweight mobile client with caching
    let client = MobileClient::new("http://api.cytah-speed.com".to_string());

    // Mobile-optimized wallet
    let wallet = MobileWallet::create()?;

    // Efficient operations
    let balance = wallet.get_balance(&client).await?;
    println!("Balance: {}", balance);

    // Memory-efficient caching
    client.clear_cache(); // Free memory when needed

    Ok(())
}
```

### Mobile Event Handling

```rust
// Mobile apps can subscribe to events with battery-efficient polling
use cytah_core::sdk::mobile::MobileClient;

struct MobileEventHandler {
    client: MobileClient,
    last_block_height: u64,
}

impl MobileEventHandler {
    async fn check_for_new_blocks(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let node_info = self.client.get_node_info().await?;

        if node_info.height > self.last_block_height {
            println!("New blocks available!");
            self.last_block_height = node_info.height;
            // Handle new blocks...
        }

        Ok(())
    }
}
```

## Best Practices

### Security

1. **Never expose private keys** - Use wallet abstractions
2. **Validate all inputs** - Check addresses, amounts, etc.
3. **Use HTTPS/WebSocket Secure** - Encrypt all communications
4. **Implement rate limiting** - Prevent abuse
5. **Regular security audits** - Review code for vulnerabilities

### Performance

1. **Use caching** - Cache frequently accessed data
2. **Batch operations** - Group multiple operations
3. **Connection pooling** - Reuse connections
4. **Background processing** - Handle heavy operations asynchronously
5. **Memory management** - Clean up resources

### Development

1. **Test thoroughly** - Unit tests, integration tests, manual testing
2. **Use devnet** - Test on local network first
3. **Monitor performance** - Track metrics and optimize
4. **Handle errors gracefully** - Provide meaningful error messages
5. **Keep dependencies updated** - Stay current with security patches

### Deployment

1. **Environment separation** - Dev, staging, production
2. **Configuration management** - Secure config handling
3. **Logging and monitoring** - Track application health
4. **Backup strategies** - Regular data backups
5. **Scalability planning** - Design for growth

## Getting Help

- **Documentation**: Check docs/ directory
- **Examples**: Look at examples/ directory
- **Community**: Join Cytah-Speed Discord/Forum
- **Issues**: Report bugs on GitHub
- **Contributing**: See CONTRIBUTING.md

## Roadmap

- **Q1 2026**: Mainnet launch, enhanced mobile SDK
- **Q2 2026**: Cross-chain bridges, advanced analytics
- **Q3 2026**: Enterprise features, improved tooling
- **Q4 2026**: Mobile wallet apps, ecosystem expansion

---

**Happy building on Cytah-Speed!** 🚀

The ecosystem is designed to be developer-friendly, secure, and scalable. Whether you're building a simple wallet or a complex dApp, Cytah-Speed provides the tools and infrastructure you need.