use std::io::{self, Write};
use std::sync::{Arc, atomic::Ordering};

use parking_lot::{Mutex, RwLock};

use crate::core::transaction::{Address, Transaction};
use crate::node::NodeRuntime;
use crate::wallet::wallet::Wallet;

/// Interactive menu-based CLI for Cytah-Speed.
///
/// This uses the real node runtime and transaction logic (no mocks).
pub struct InteractiveCli {
    /// Optional running node runtime
    node: Arc<RwLock<Option<Arc<NodeRuntime>>>>,
    /// Handle to the background node runtime task
    node_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl InteractiveCli {
    pub fn new() -> Self {
        Self {
            node: Arc::new(RwLock::new(None)),
            node_handle: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            Self::print_main_menu();
            let choice = Self::read_choice()?;
            match choice {
                1 => self.node_menu().await?,
                2 => self.transaction_menu().await?,
                3 => self.smart_contract_menu().await?,
                4 => self.sdk_menu().await?,
                5 => self.show_help(),
                6 => {
                    self.shutdown_node().await;
                    println!("Goodbye!");
                    break;
                }
                _ => println!("Invalid option, please select a number from the menu."),
            }
        }

        Ok(())
    }

    fn print_main_menu() {
        println!("\n=== CYTAH SPEED MAIN MENU ===");
        println!("1. Node & Mining");
        println!("2. Transactions");
        println!("3. Smart Contracts");
        println!("4. SDK Tools");
        println!("5. Help");
        println!("6. Exit");
        print!("Enter choice: ");
        io::stdout().flush().ok();
    }

    fn read_line(prompt: &str) -> io::Result<String> {
        print!("{}", prompt);
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        Ok(input.trim().to_string())
    }

    fn read_choice() -> io::Result<u32> {
        let input = Self::read_line("")?;
        Ok(input.trim().parse::<u32>().unwrap_or(0))
    }

    async fn node_menu(&self) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            println!("\n=== NODE MENU ===");
            println!("1. Start Node");
            println!("2. Start Mining");
            println!("3. Show Node Status");
            println!("4. Back");
            print!("Enter choice: ");
            io::stdout().flush()?;

            let choice = Self::read_choice()?;
            match choice {
                1 => self.start_node().await?,
                2 => self.start_mining().await?,
                3 => self.show_node_status().await?,
                4 => break,
                _ => println!("Invalid option"),
            }
        }
        Ok(())
    }

    async fn start_node(&self) -> Result<(), Box<dyn std::error::Error>> {
        // If already running, do nothing
        if self.node.read().is_some() {
            println!("Node already running");
            return Ok(());
        }

        println!("Starting node runtime...");
        let node = Arc::new(NodeRuntime::new().await?);

        // Spawn node runtime in background task
        let node_clone = node.clone();
        let handle = tokio::spawn(async move {
            if let Err(e) = node_clone.run().await {
                eprintln!("Node runtime error: {}", e);
            }
        });

        *self.node_handle.lock() = Some(handle);
        *self.node.write() = Some(node);

        println!("Node started.");
        Ok(())
    }

    async fn start_mining(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(node) = self.node.read().as_ref() {
            node.start_mining().await?;
            println!("Mining enabled (blocks will be produced when transactions exist).\n");
        } else {
            println!("Node not running. Please start the node first.");
        }
        Ok(())
    }

    async fn show_node_status(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(node) = self.node.read().as_ref() {
            let dag = node.blockdag.read();
            let tip_blocks = dag.get_tip_blocks();
            let height = tip_blocks.iter().map(|b| b.header.chain_height).max().unwrap_or(0);
            let tip_count = tip_blocks.len();
            let peer_count = node.get_peer_count();
            let mempool_size = node.get_mempool_size();

            println!("\n-- Node Status --");
            println!("Block height: {}", height);
            println!("DAG tips: {}", tip_count);
            println!("Peer count: {}", peer_count);
            println!("Mempool size: {}", mempool_size);
            println!("Mining enabled: {}", node.mining_enabled.load(Ordering::Relaxed));
        } else {
            println!("Node is not running.");
        }
        Ok(())
    }

    async fn transaction_menu(&self) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            println!("\n=== TRANSACTION MENU ===");
            println!("1. Transfer CTS");
            println!("2. Check Balance");
            println!("3. Show Wallet Address");
            println!("4. Back");
            print!("Enter choice: ");
            io::stdout().flush()?;

            let choice = Self::read_choice()?;
            match choice {
                1 => self.transfer_cts().await?,
                2 => self.check_balance().await?,
                3 => self.show_wallet_address().await?,
                4 => break,
                _ => println!("Invalid option"),
            }
        }
        Ok(())
    }

    async fn transfer_cts(&self) -> Result<(), Box<dyn std::error::Error>> {
        let wallet_path = Self::read_line("Wallet file path: ")?;
        let wallet = Wallet::load_from_file(&wallet_path)?;
        let to = Self::read_line("Recipient address (cyt...): ")?;
        let amount_str = Self::read_line("Amount (CTS): ")?;
        let amount: u64 = amount_str.trim().parse().unwrap_or(0);

        // derive addresses
        let from_addr = Self::address_from_wallet(&wallet)?;
        let to_addr = Self::parse_address(&to)?;

        // Determine nonce from state
        let nonce = if let Some(node) = self.node.read().as_ref() {
            node.get_nonce(&from_addr)
        } else {
            0
        };

        let mut tx = Transaction::new_transfer(from_addr, to_addr, amount, nonce, 21000, 1);
        tx.sign(&wallet.private_key)?;

        if let Some(node) = self.node.read().as_ref() {
            node.add_transaction(tx).await?;
            println!("Transaction submitted to mempool.");
        } else {
            println!("Node is not running. Start the node first.");
        }

        Ok(())
    }

    async fn check_balance(&self) -> Result<(), Box<dyn std::error::Error>> {
        let address_str = Self::read_line("Address (cyt...): ")?;
        let addr = Self::parse_address(&address_str)?;

        if let Some(node) = self.node.read().as_ref() {
            let bal = node.get_balance(&addr);
            println!("Balance: {} CTS", bal);
        } else {
            println!("Node is not running. Start the node first.");
        }

        Ok(())
    }

    async fn show_wallet_address(&self) -> Result<(), Box<dyn std::error::Error>> {
        let wallet_path = Self::read_line("Wallet file path: ")?;
        let wallet = Wallet::load_from_file(&wallet_path)?;
        println!("Address: {}", wallet.address);
        Ok(())
    }

    async fn smart_contract_menu(&self) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            println!("\n=== SMART CONTRACT MENU ===");
            println!("1. Deploy Contract");
            println!("2. Call Contract");
            println!("3. Query Contract State");
            println!("4. List Contracts");
            println!("5. Back");
            print!("Enter choice: ");
            io::stdout().flush()?;

            let choice = Self::read_choice()?;
            match choice {
                1 => self.deploy_contract().await?,
                2 => self.call_contract().await?,
                3 => self.query_contract_state().await?,
                4 => self.list_contracts().await?,
                5 => break,
                _ => println!("Invalid option"),
            }
        }
        Ok(())
    }

    async fn deploy_contract(&self) -> Result<(), Box<dyn std::error::Error>> {
        let wallet_path = Self::read_line("Wallet file path: ")?;
        let wasm_path = Self::read_line("WASM file path: ")?;

        let wallet = Wallet::load_from_file(&wallet_path)?;
        let wasm_code = std::fs::read(&wasm_path)?;

        let from_addr = Self::address_from_wallet(&wallet)?;
        let nonce = if let Some(node) = self.node.read().as_ref() {
            node.get_nonce(&from_addr)
        } else {
            0
        };

        let mut tx = Transaction::new_deploy(from_addr, wasm_code, Vec::new(), nonce, 1_000_000, 1);
        tx.sign(&wallet.private_key)?;

        if let Some(node) = self.node.read().as_ref() {
            node.add_transaction(tx).await?;
            println!("Contract deployment transaction submitted.");
        } else {
            println!("Node is not running. Start the node first.");
        }

        Ok(())
    }

    async fn call_contract(&self) -> Result<(), Box<dyn std::error::Error>> {
        let wallet_path = Self::read_line("Wallet file path: ")?;
        let contract_addr = Self::read_line("Contract address (cyt...): ")?;
        let method = Self::read_line("Method name: ")?;
        let args = Self::read_line("Args (hex or plain text): ")?;

        let wallet = Wallet::load_from_file(&wallet_path)?;
        let from_addr = Self::address_from_wallet(&wallet)?;
        let nonce = if let Some(node) = self.node.read().as_ref() {
            node.get_nonce(&from_addr)
        } else {
            0
        };

        let contract_address = Self::parse_address(&contract_addr)?;

        // Allow hex-encoded args (starting with 0x) or raw string
        let args_bytes = if args.starts_with("0x") {
            hex::decode(&args[2..]).unwrap_or_default()
        } else {
            args.into_bytes()
        };

        let mut tx = Transaction::new_call(from_addr, contract_address, method, args_bytes, nonce, 500_000, 1);
        tx.sign(&wallet.private_key)?;

        if let Some(node) = self.node.read().as_ref() {
            node.add_transaction(tx).await?;
            println!("Contract call transaction submitted.");
        } else {
            println!("Node is not running. Start the node first.");
        }

        Ok(())
    }

    async fn query_contract_state(&self) -> Result<(), Box<dyn std::error::Error>> {
        let contract_addr = Self::read_line("Contract address (cyt...): ")?;
        let contract_address = Self::parse_address(&contract_addr)?;

        if let Some(node) = self.node.read().as_ref() {
            let registry = node.contract_registry.lock();
            if let Some(info) = registry.get_contract(&contract_address) {
                println!("Contract {} found.", contract_addr);
                println!("Bytecode ({} bytes)", info.bytecode.len());
                println!("Metadata ({} bytes)", info.metadata.len());
            } else {
                println!("Contract not found in registry.");
            }
        } else {
            println!("Node is not running. Start the node first.");
        }

        Ok(())
    }

    async fn list_contracts(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(node) = self.node.read().as_ref() {
            let registry = node.contract_registry.lock();
            let list = registry.list_all_contracts()?;
            println!("Found {} contracts:", list.len());
            for c in list {
                println!(" - {} ({} bytes)", hex::encode(c.address), c.bytecode.len());
            }
        } else {
            println!("Node is not running. Start the node first.");
        }
        Ok(())
    }

    async fn sdk_menu(&self) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            println!("\n=== SDK MENU ===");
            println!("1. Generate Wallet");
            println!("2. Sign Transaction");
            println!("3. Verify Signature");
            println!("4. Export Private Key");
            println!("5. Back");
            print!("Enter choice: ");
            io::stdout().flush()?;

            let choice = Self::read_choice()?;
            match choice {
                1 => self.generate_wallet().await?,
                2 => self.sign_transaction().await?,
                3 => self.verify_signature().await?,
                4 => self.export_private_key().await?,
                5 => break,
                _ => println!("Invalid option"),
            }
        }
        Ok(())
    }

    async fn generate_wallet(&self) -> Result<(), Box<dyn std::error::Error>> {
        let wallet = Wallet::new();
        println!("Address: {}", wallet.address);
        println!("Public Key: {}", hex::encode(wallet.public_key.serialize()));
        let save = Self::read_line("Save wallet to file? (y/N): ")?;
        if save.to_lowercase().starts_with('y') {
            let path = Self::read_line("File path: ")?;
            wallet.save_to_file(&path)?;
            println!("Saved wallet to {}", path);
        }
        Ok(())
    }

    async fn sign_transaction(&self) -> Result<(), Box<dyn std::error::Error>> {
        let wallet_path = Self::read_line("Wallet file path: ")?;
        let wallet = Wallet::load_from_file(&wallet_path)?;
        let message = Self::read_line("Message to sign: ")?;
        let signature = wallet.sign_message(message.as_bytes())?;
        println!("Signature (hex): {}", hex::encode(&signature));
        Ok(())
    }

    async fn verify_signature(&self) -> Result<(), Box<dyn std::error::Error>> {
        let wallet_path = Self::read_line("Wallet file path: ")?;
        let wallet = Wallet::load_from_file(&wallet_path)?;
        let message = Self::read_line("Message: ")?;
        let signature_hex = Self::read_line("Signature (hex): ")?;
        let signature = hex::decode(signature_hex.trim())?;
        let ok = wallet.verify_signature(message.as_bytes(), &signature)?;
        println!("Valid signature: {}", ok);
        Ok(())
    }

    async fn export_private_key(&self) -> Result<(), Box<dyn std::error::Error>> {
        let wallet_path = Self::read_line("Wallet file path: ")?;
        let wallet = Wallet::load_from_file(&wallet_path)?;
        println!("Private key (hex): {}", hex::encode(wallet.private_key.secret_bytes()));
        Ok(())
    }

    fn show_help(&self) {
        println!("\n=== HELP ===");
        println!("Network ports: (default) RPC 3000 (if configured)");
        println!("Data directory: ./data/");
        println!("Mining rules: produces blocks when mempool has transactions.");
        println!("Supply rules: 600M CTS total, 100M genesis, 500M mined.");
        println!("Use the menu numbers to navigate.");
    }

    async fn shutdown_node(&self) {
        // Stop mining and abort runtime task if running
        if let Some(node) = self.node.read().as_ref() {
            let _ = node.stop_mining().await;
        }
        if let Some(handle) = self.node_handle.lock().take() {
            handle.abort();
        }
        *self.node.write() = None;
    }

    fn address_from_wallet(wallet: &Wallet) -> Result<Address, Box<dyn std::error::Error>> {
        if !wallet.address.starts_with("cyt") {
            return Err("Invalid wallet address".into());
        }
        let data = hex::decode(&wallet.address[3..])?;
        if data.len() != 20 {
            return Err("Invalid wallet address length".into());
        }
        let mut addr = [0u8; 20];
        addr.copy_from_slice(&data);
        Ok(addr)
    }

    fn parse_address(input: &str) -> Result<Address, Box<dyn std::error::Error>> {
        if !input.starts_with("cyt") {
            return Err("Address must start with 'cyt'".into());
        }
        let data = hex::decode(&input[3..])?;
        if data.len() != 20 {
            return Err("Invalid address length".into());
        }
        let mut addr = [0u8; 20];
        addr.copy_from_slice(&data);
        Ok(addr)
    }
}
