use clap::{Parser, Subcommand};
use std::sync::Arc;
use parking_lot::RwLock;
use crate::node::NodeRuntime;
use crate::wallet::wallet::Wallet;
use crate::core::Transaction;
use crate::network::p2p_node::P2PNode;
use reqwest::Client;

/// Cytah-Speed Blockchain CLI
#[derive(Parser)]
#[command(name = "cyt")]
#[command(about = "Cytah-Speed blockchain node and wallet CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Node operations
    Node {
        #[command(subcommand)]
        node_command: NodeCommands,
    },
    /// Wallet operations
    Wallet {
        #[command(subcommand)]
        wallet_command: WalletCommands,
    },
    /// Transaction operations
    Tx {
        #[command(subcommand)]
        tx_command: TxCommands,
    },
    /// Contract operations
    Contract {
        #[command(subcommand)]
        contract_command: ContractCommands,
    },
}

#[derive(Subcommand)]
pub enum NodeCommands {
    /// Start the blockchain node
    Start {
        /// Listen address for P2P network
        #[arg(short, long, default_value = "/ip4/0.0.0.0/tcp/0")]
        listen_addr: String,
        /// RPC server address
        #[arg(short, long, default_value = "127.0.0.1:3000")]
        rpc_addr: String,
    },
}

#[derive(Subcommand)]
pub enum WalletCommands {
    /// Create a new wallet
    Create {
        /// Path to save the wallet
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Get wallet balance
    Balance {
        /// Wallet address
        address: String,
        /// RPC server URL
        #[arg(short, long, default_value = "http://127.0.0.1:3000")]
        rpc_url: String,
    },
}

#[derive(Subcommand)]
pub enum TxCommands {
    /// Send a transaction
    Send {
        /// Sender wallet file
        #[arg(short, long)]
        wallet: String,
        /// Recipient address
        #[arg(short, long)]
        to: String,
        /// Amount to send
        #[arg(short, long)]
        amount: u64,
        /// RPC server URL
        #[arg(short, long, default_value = "http://127.0.0.0.1:3000")]
        rpc_url: String,
    },
}

#[derive(Subcommand)]
pub enum ContractCommands {
    /// Deploy a contract
    Deploy {
        /// Path to WASM bytecode file
        #[arg(short, long)]
        wasm: String,
        /// Wallet file path
        #[arg(short, long)]
        wallet: Option<String>,
        /// RPC server URL
        #[arg(short, long, default_value = "http://127.0.0.1:3000")]
        rpc_url: String,
    },
    /// Call a contract
    Call {
        /// Contract address
        #[arg(short, long)]
        contract: String,
        /// Method name
        #[arg(short, long)]
        method: String,
        /// Arguments (hex-encoded)
        #[arg(short, long)]
        args: Option<String>,
        /// Wallet file path
        #[arg(short, long)]
        wallet: Option<String>,
        /// RPC server URL
        #[arg(short, long, default_value = "http://127.0.0.1:3000")]
        rpc_url: String,
    },
}

/// CLI handler
pub struct CliHandler {
    client: Client,
}

impl CliHandler {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    /// Handle node start command
    pub async fn handle_node_start(&self, listen_addr: &str, rpc_addr: &str) -> Result<(), Box<dyn std::error::Error>> {
        println!("Starting Cytah-Speed node...");
        println!("P2P listen address: {}", listen_addr);
        println!("RPC server address: {}", rpc_addr);

        // Create node runtime
        let node = NodeRuntime::new().await?;
        let node_arc = Arc::new(RwLock::new(Some(node)));

        // Start RPC server in background
        let rpc_node = node_arc.clone();
        let rpc_addr = rpc_addr.to_string(); // Convert to owned String for 'static lifetime
        tokio::spawn(async move {
            // Extract node data while holding read guard, then drop the guard
            let (dag, mempool, state_manager) = {
                let read_guard = rpc_node.read();
                if let Some(node) = read_guard.as_ref() {
                    (
                        node.blockdag.clone(),
                        node.mempool.clone(),
                        node.state_manager.clone(),
                    )
                } else {
                    return; // If node is None, exit early
                }
            }; // Drop read guard here
            
            // Note: P2P node not implemented yet
            let p2p_node: Option<Arc<tokio::sync::RwLock<P2PNode>>> = None;

            if let Err(e) = crate::rpc::server::start_server(
                &rpc_addr,
                dag,
                mempool,
                state_manager,
                p2p_node,
            ).await {
                tracing::error!("RPC server error: {}", e);
            }
        });

        // Start node
        if let Some(node) = node_arc.write().take() {
            node.run().await?;
        }

        Ok(())
    }

    /// Handle wallet create command
    pub async fn handle_wallet_create(&self, output: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
        println!("Creating new wallet...");
        let wallet = Wallet::new();

        println!("Address: {}", wallet.address);
        println!("Public Key: {}", hex::encode(wallet.public_key.serialize()));

        if let Some(path) = output {
            wallet.save_to_file(path)?;
            println!("Wallet saved to: {}", path);
        } else {
            println!("Private Key: {}", hex::encode(wallet.private_key.secret_bytes()));
            println!("⚠️  WARNING: Save this private key securely!");
        }

        Ok(())
    }

    /// Handle wallet balance command
    pub async fn handle_wallet_balance(&self, address: &str, rpc_url: &str) -> Result<(), Box<dyn std::error::Error>> {
        let url = format!("{}/balance/{}", rpc_url, address);
        let response = self.client.get(&url).send().await?;

        if response.status().is_success() {
            let balance: serde_json::Value = response.json().await?;
            println!("Address: {}", address);
            println!("Balance: {} cyt", balance["balance"]);
            println!("Nonce: {}", balance["nonce"]);
        } else {
            println!("Error: Failed to get balance");
        }

        Ok(())
    }

    /// Handle transaction send command
    pub async fn handle_tx_send(&self, wallet_path: &str, to: &str, amount: u64, rpc_url: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Load wallet
        let wallet = Wallet::load_from_file(wallet_path)?;

        // Parse recipient address (expect cyt... format)
        if !to.starts_with("cyt") {
            return Err("Invalid address format. Must start with 'cyt'".into());
        }
        let addr_bytes = hex::decode(&to[3..])?;
        if addr_bytes.len() != 20 {
            return Err("Invalid address length".into());
        }
        let mut to_addr = [0u8; 20];
        to_addr.copy_from_slice(&addr_bytes);

        // Parse sender address
        let sender_bytes = hex::decode(&wallet.address[3..])?;
        let mut from_addr = [0u8; 20];
        from_addr.copy_from_slice(&sender_bytes);

        // Get current nonce by querying the node
        let balance_url = format!("{}/balance/{}", rpc_url, wallet.address);
        let nonce = if let Ok(resp) = self.client.get(&balance_url).send().await {
            if resp.status().is_success() {
                let json: serde_json::Value = resp.json().await?;
                json["nonce"].as_u64().unwrap_or(0)
            } else {
                0
            }
        } else {
            0
        };

        // Create transaction
        let mut tx = Transaction::new_transfer(from_addr, to_addr, amount, nonce, 21000, 1); // Default gas limit and price

        // Sign transaction
        tx.sign(&wallet.private_key)?;

        // Send to RPC
        let url = format!("{}/send_tx", rpc_url);
        let request = serde_json::json!({
            "transaction": tx
        });

        let response = self.client.post(&url)
            .json(&request)
            .send()
            .await?;

        if response.status().is_success() {
            let result: serde_json::Value = response.json().await?;
            println!("Transaction sent successfully!");
            println!("Status: {}", result["status"]);
            println!("Message: {}", result["message"]);
        } else {
            println!("Error: Failed to send transaction");
        }

        Ok(())
    }

    /// Handle contract deploy command
    pub async fn handle_contract_deploy(&self, wasm_path: &str, wallet_path: Option<&str>, rpc_url: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Wallet is required for deployment; load it if provided
        let wallet = match wallet_path {
            Some(path) => Wallet::load_from_file(path)?,
            None => return Err("Wallet path required for contract operations".into()),
        };

        // Read WASM file
        let wasm_code = std::fs::read(wasm_path)?;
        let wasm_hex = hex::encode(&wasm_code);

        // fetch nonce from node
        let balance_url = format!("{}/balance/{}", rpc_url, wallet.address);
        let nonce = if let Ok(resp) = self.client.get(&balance_url).send().await {
            if resp.status().is_success() {
                let json: serde_json::Value = resp.json().await?;
                json["nonce"].as_u64().unwrap_or(0)
            } else {
                0
            }
        } else {
            0
        };

        // Send deploy request
        let url = format!("{}/contract/deploy", rpc_url);
        let request = serde_json::json!({
            "from": wallet.address,
            "nonce": nonce,
            "wasm_code": wasm_hex,
            "init_args": null
        });

        let response = self.client.post(&url)
            .json(&request)
            .send()
            .await?;

        if response.status().is_success() {
            let result: serde_json::Value = response.json().await?;
            println!("Contract deployed successfully!");
            println!("Address: {}", result["contract_address"]);
            println!("TX Hash: {}", result["tx_hash"]);
        } else {
            println!("Error: Failed to deploy contract");
        }

        Ok(())
    }

    /// Handle contract call command
    pub async fn handle_contract_call(&self, contract: &str, method: &str, args: Option<&str>, wallet_path: Option<&str>, rpc_url: &str) -> Result<(), Box<dyn std::error::Error>> {
        let wallet = match wallet_path {
            Some(path) => Wallet::load_from_file(path)?,
            None => return Err("Wallet path required for contract operations".into()),
        };

        let balance_url = format!("{}/balance/{}", rpc_url, wallet.address);
        let nonce = if let Ok(resp) = self.client.get(&balance_url).send().await {
            if resp.status().is_success() {
                let json: serde_json::Value = resp.json().await?;
                json["nonce"].as_u64().unwrap_or(0)
            } else {
                0
            }
        } else {
            0
        };

        let url = format!("{}/contract/call", rpc_url);
        let request = serde_json::json!({
            "from": wallet.address,
            "nonce": nonce,
            "contract_address": contract,
            "method": method,
            "args": args
        });

        let response = self.client.post(&url)
            .json(&request)
            .send()
            .await?;

        if response.status().is_success() {
            let result: serde_json::Value = response.json().await?;
            println!("Contract call submitted!");
            println!("Status: {}", result["status"]);
            if let Some(result_val) = result.get("result") {
                println!("Result: {}", result_val);
            }
        } else {
            println!("Error: Failed to call contract");
        }

        Ok(())
    }
}