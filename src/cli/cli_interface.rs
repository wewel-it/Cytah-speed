use std::sync::Arc;
use parking_lot::RwLock;

use crate::node::NodeRuntime;
use crate::core::Transaction;
use secp256k1::SecretKey;

/// CLI interface untuk node
pub struct CLIInterface {
    node: Arc<RwLock<Option<NodeRuntime>>>,
}

impl CLIInterface {
    pub fn new() -> Self {
        Self {
            node: Arc::new(RwLock::new(None)),
        }
    }

    /// Jalankan CLI
    pub async fn run(&self) -> Result<(), String> {
        loop {
            println!("\n=== Cytah-Speed Blockchain Node ===");
            println!("1. start-node");
            println!("2. send-transaction");
            println!("3. print-dag");
            println!("4. print-state");
            println!("5. print-finality");
            println!("6. print-mempool");
            println!("7. connect-peer");
            println!("8. exit");
            println!("=====================================");

            print!("Enter command number: ");
            use std::io::{self, Write};
            io::stdout().flush().ok();

            let mut input = String::new();
            io::stdin().read_line(&mut input).ok();
            let choice = input.trim();

            match choice {
                "1" => self.cmd_start_node().await?,
                "2" => self.cmd_send_transaction().await?,
                "3" => self.cmd_print_dag()?,
                "4" => self.cmd_print_state()?,
                "5" => self.cmd_print_finality()?,
                "6" => self.cmd_print_mempool()?,
                "7" => self.cmd_connect_peer().await?,
                "8" => {
                    println!("Exiting...");
                    break;
                }
                _ => println!("Invalid command"),
            }
        }

        Ok(())
    }

    /// Start node
    async fn cmd_start_node(&self) -> Result<(), String> {
        println!("\n--- Starting Node ---");

        // Create node asynchronously with default config
        let node = NodeRuntime::new().await.map_err(|e| e.to_string())?;
        node.initialize().await.map_err(|e| e.to_string())?;

        *self.node.write() = Some(node);

        println!("✓ Node started");
        println!("Node initialized and ready to accept transactions");

        Ok(())
    }

    /// Send transaction
    async fn cmd_send_transaction(&self) -> Result<(), String> {
        let node = self.node.read();
        let node = node.as_ref().ok_or("Node not started")?;

        println!("\n--- Send Transaction ---");

        print!("From address (hex, 40 chars): ");
        use std::io::{self, Write};
        io::stdout().flush().ok();

        let mut from_str = String::new();
        io::stdin().read_line(&mut from_str).ok();
        let from_str = from_str.trim();

        let from: [u8; 20] = hex::decode(from_str)
            .ok()
            .and_then(|v| v.try_into().ok())
            .ok_or("Invalid from address")?;

        print!("To address (hex, 40 chars): ");
        io::stdout().flush().ok();

        let mut to_str = String::new();
        io::stdin().read_line(&mut to_str).ok();
        let to_str = to_str.trim();

        let to: [u8; 20] = hex::decode(to_str)
            .ok()
            .and_then(|v| v.try_into().ok())
            .ok_or("Invalid to address")?;

        print!("Amount: ");
        io::stdout().flush().ok();

        let mut amount_str = String::new();
        io::stdin().read_line(&mut amount_str).ok();
        let amount: u64 = amount_str.trim().parse().map_err(|e: std::num::ParseIntError| e.to_string())?;

        print!("Nonce: ");
        io::stdout().flush().ok();

        let mut nonce_str = String::new();
        io::stdin().read_line(&mut nonce_str).ok();
        let nonce: u64 = nonce_str.trim().parse().map_err(|e: std::num::ParseIntError| e.to_string())?;

        // Create and sign transaction
        let mut tx = Transaction::new_transfer(from, to, amount, nonce, 21000, 1);

        // Sign dengan test key
        let secret_key = SecretKey::from_slice(&[1; 32])
            .map_err(|_| "Failed to create secret key")?;
        tx.sign(&secret_key)?;

        // Add ke node
        node.add_transaction(tx).await?;

        println!("✓ Transaction sent");
        println!("Mempool size: {}", node.get_mempool_size());

        Ok(())
    }

    /// Print DAG status
    fn cmd_print_dag(&self) -> Result<(), String> {
        let node = self.node.read();
        let node = node.as_ref().ok_or("Node not started")?;

        println!("\n--- DAG Status ---");

        let tips = node.get_tips();
        println!("Tips: {}", tips.len());
        for (i, tip) in tips.iter().enumerate() {
            println!("  [{}] {}", i, hex::encode(&tip[..8]));
        }

        let dag = node.blockdag.read();
        let all_blocks = dag.get_all_blocks();
        println!("Total blocks: {}", all_blocks.len());

        Ok(())
    }

    /// Print state
    fn cmd_print_state(&self) -> Result<(), String> {
        let node = self.node.read();
        let node = node.as_ref().ok_or("Node not started")?;

        println!("\n--- State Status ---");

        let state_root = node.get_state_root();
        println!("Current state root: {}", hex::encode(state_root));
        println!("State management active");

        Ok(())
    }

    /// Print finality
    fn cmd_print_finality(&self) -> Result<(), String> {
        let node = self.node.read();
        let node = node.as_ref().ok_or("Node not started")?;

        println!("\n--- Finality Status ---");

        match node.get_finality_height() {
            Some(height) => println!("Finalization height: {}", height),
            None => println!("No finalized blocks yet"),
        }

        println!("Confirmation depth: {}", node.finality_engine.confirmation_depth);

        Ok(())
    }

    /// Print mempool
    fn cmd_print_mempool(&self) -> Result<(), String> {
        let node = self.node.read();
        let node = node.as_ref().ok_or("Node not started")?;

        println!("\n--- Mempool Status ---");

        let size = node.get_mempool_size();
        println!("Mempool size: {}", size);

        let txs = (*node.mempool).get_all_transactions();
        println!("Transactions:");
        for (i, tx) in txs.iter().take(10).enumerate() {
            println!(
                "  [{}] {} tokens, nonce={}",
                i,
                match &tx.transaction.payload {
                    crate::core::transaction::TxPayload::Transfer { amount, .. } => *amount,
                    _ => 0,
                },
                tx.transaction.nonce
            );
        }

        Ok(())
    }

    /// Connect to peer
    async fn cmd_connect_peer(&self) -> Result<(), String> {
        let _node = self.node.read();
        let _node = _node.as_ref().ok_or("Node not started")?;

        println!("\n--- Connect Peer ---");

        print!("Peer address (IP:PORT): ");
        use std::io::{self, Write};
        io::stdout().flush().ok();

        let mut peer_addr = String::new();
        io::stdin().read_line(&mut peer_addr).ok();
        let peer_addr = peer_addr.trim().to_string();

        // Network connectivity would be handled through P2PNode
        println!("✓ Peer address to be connected: {}", peer_addr);
        println!("Note: P2P network connectivity is managed separately through P2PNode");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cli_creation() {
        let cli = CLIInterface::new();
        let node = cli.node.read();
        assert!(node.is_none());
    }

    #[test]
    fn test_address_parsing() {
        let addr_str = "0102030405060708090a0b0c0d0e0f1011121314";
        let addr: [u8; 20] = hex::decode(addr_str)
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(addr.len(), 20);
    }
}
