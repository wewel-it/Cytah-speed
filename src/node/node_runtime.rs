use std::sync::Arc;
use parking_lot::RwLock;
use tokio::time::{Duration, interval};

use crate::core::{Transaction, BlockHash};
use crate::dag::blockdag::BlockDAG;
use crate::consensus::ghostdag::GHOSTDAGEngine;
use crate::state::state_manager::StateManager;
use crate::execution::transaction_executor::TransactionExecutor;
use crate::mempool::TxDagMempool;
use crate::block::BlockProducer;
use crate::finality::FinalityEngine;

/// Node runtime yang menjalankan blockchain node
/// 
/// Komponen:
/// - BlockDAG: struktur DAG dari blok
/// - GHOSTDAG: consensus engine
/// - StateManager: state execution
/// - TxDagMempool: transaksi mempool
/// - BlockProducer: menghasilkan blok baru
/// - FinalityEngine: menghitung finality
#[derive(Clone)]
pub struct NodeRuntime {
    /// BlockDAG
    pub blockdag: Arc<RwLock<BlockDAG>>,
    /// GHOSTDAG consensus engine (wrapped for mutability)
    pub ghostdag: Arc<RwLock<GHOSTDAGEngine>>,
    /// State manager
    pub state_manager: Arc<parking_lot::Mutex<StateManager>>,
    /// Transaction mempool
    pub mempool: Arc<TxDagMempool>,
    /// Block producer
    pub block_producer: Arc<BlockProducer>,
    /// Finality engine
    pub finality_engine: Arc<FinalityEngine>,
    /// Execution engine
    pub executor: Arc<parking_lot::Mutex<TransactionExecutor>>,
    /// Node ID
    pub node_id: [u8; 20],
    /// Block production interval (ms)
    pub block_interval: u64,
}

impl NodeRuntime {
    /// Buat node runtime baru dengan default settings
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let node_id = [0u8; 20]; // Default node ID
        Self::new_with_config(node_id, 5000, 10, 100, 1000)
    }

    /// Buat node runtime baru dengan konfigurasi
    pub fn new_with_config(
        node_id: [u8; 20],
        block_interval: u64,
        confirmation_depth: usize,
        _max_peers: usize,
        mempool_size: usize,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let blockdag = Arc::new(RwLock::new(BlockDAG::new()));
        let mempool = Arc::new(TxDagMempool::new(mempool_size));
        let ghostdag = Arc::new(RwLock::new(GHOSTDAGEngine::new(2)));
        let state_manager = Arc::new(parking_lot::Mutex::new(StateManager::new()));
        let executor = Arc::new(parking_lot::Mutex::new(TransactionExecutor::new()));
        let finality_engine = Arc::new(FinalityEngine::new(confirmation_depth));

        let block_producer = Arc::new(BlockProducer::new(
            mempool.clone(),
            blockdag.clone(),
            node_id,
            0,
            100,
        ));

        Ok(Self {
            blockdag,
            ghostdag,
            state_manager,
            mempool,
            block_producer,
            finality_engine,
            executor,
            node_id,
            block_interval,
        })
    }

    /// Initialize node
    pub async fn initialize(&self) -> Result<(), String> {
        tracing::info!("Initializing node...");

        // Create genesis block
        self.blockdag.write().create_genesis_if_empty();
        tracing::info!("✓ Genesis block created");

        tracing::info!("Node initialization complete");
        Ok(())
    }

    /// Main node loop
    pub async fn run(&self) -> Result<(), String> {
        self.initialize().await?;

        let mut block_timer = interval(Duration::from_millis(self.block_interval));
        let mut finality_timer = interval(Duration::from_millis(1000)); // Check finality every 1s

        tracing::info!("Node runtime started. Node ID: {}", hex::encode(self.node_id));

        loop {
            tokio::select! {
                _ = block_timer.tick() => {
                    self.produce_block().await.ok();
                }
                _ = finality_timer.tick() => {
                    self.check_finality().ok();
                }
            }
        }
    }

    /// Produce block periodically
    async fn produce_block(&self) -> Result<(), String> {
        let dag = self.blockdag.read();
        let _state_root = self.state_manager.lock().get_state_root();

        // Hanya produce block jika ada transaksi siap atau banyak mempool
        if (*self.mempool).size() == 0 {
            return Ok(());
        }

        tracing::debug!("Attempting to produce block...");

        drop(dag); // Release read lock

        // Create block
        match self.block_producer.create_block() {
            Ok(block) => {
                tracing::info!(
                    "Block produced: {} with {} transactions",
                    hex::encode(&block.hash[..16]),
                    block.transactions.len()
                );

                // Add block ke DAG
                let mut dag = self.blockdag.write();
                dag.insert_block(block.clone())?;
                drop(dag);

                // Remove transaksi dari mempool
                for tx in &block.transactions {
                    let tx_hash_vec = tx.hash();
                    let tx_hash: [u8; 32] = tx_hash_vec.try_into()
                        .map_err(|_| "Invalid hash".to_string())?;
                    (*self.mempool).remove_transaction(&tx_hash);
                }

                // Execute block
                self.execute_block(&block)?;

                Ok(())
            }
            Err(e) => {
                tracing::debug!("Could not produce block: {}", e);
                Ok(())
            }
        }
    }

    /// Execute block dan apply state changes
    fn execute_block(&self, block: &crate::core::Block) -> Result<(), String> {
        let mut exec = self.executor.lock();
        let result = exec.execute_block(block);

        if result.success {
            // apply transactions from block
            let mut state = self.state_manager.lock();
            state.apply_block(&block.transactions)?;
            tracing::debug!("Block executed: {} txs", result.executed_transactions);
        }

        Ok(())
    }



    /// Check finality dari GHOSTDAG ordering
    fn check_finality(&self) -> Result<(), String> {
        // Dapatkan blocks dari DAG
        let dag = self.blockdag.read();
        let blocks = dag.get_all_blocks();

        if blocks.is_empty() {
            return Ok(());
        }

        drop(dag);

        // Run GHOSTDAG untuk dapatkan ordering
        let dag = self.blockdag.read();
        let _tips = dag.get_tips().to_vec();
        drop(dag);

        let ordering = self.ghostdag.write().generate_ordering();
        if let Ok(order) = ordering {
            // Compute finality
            self.finality_engine.compute_finality(&order)?;
        }

        Ok(())
    }

    /// Add transaction ke mempool
    pub async fn add_transaction(&self, tx: Transaction) -> Result<(), String> {
        // Validasi
        (*self.mempool).validate_transaction(&tx)?;

        // Add ke mempool
        (*self.mempool).add_transaction(tx.clone(), vec![])?;

        Ok(())
    }

    /// Get current state root
    pub fn get_state_root(&self) -> [u8; 32] {
        self.state_manager.lock().get_state_root()
    }

    /// Get current tips
    pub fn get_tips(&self) -> Vec<BlockHash> {
        self.blockdag.read().get_tips().to_vec()
    }

    /// Get finality status
    pub fn get_finality_height(&self) -> Option<u64> {
        self.finality_engine.get_finalization_height()
    }

    /// Get mempool size
    pub fn get_mempool_size(&self) -> usize {
        (*self.mempool).size()
    }

    /// Get connected peers
    pub fn get_connected_peers(&self) -> Vec<String> {
        // TODO: Integrate with P2PNode
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_node_creation() {
        let node = NodeRuntime::new([1; 20], 500, 3, 10, 1000);
        assert_eq!(node.node_id, [1; 20]);
        assert_eq!(node.block_interval, 500);
    }

    #[tokio::test]
    async fn test_node_initialization() {
        let node = NodeRuntime::new([2; 20], 500, 3, 10, 1000);
        let result = node.initialize().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_tips() {
        let node = NodeRuntime::new([3; 20], 500, 3, 10, 1000);
        node.initialize().await.unwrap();
        let tips = node.get_tips();
        assert!(!tips.is_empty());
    }

    #[tokio::test]
    async fn test_get_mempool_size() {
        let node = NodeRuntime::new([4; 20], 500, 3, 10, 1000);
        assert_eq!(node.get_mempool_size(), 0);
    }

    #[tokio::test]
    async fn test_get_connected_peers() {
        let node = NodeRuntime::new([5; 20], 500, 3, 10, 1000);
        let peers = node.get_connected_peers();
        assert_eq!(peers.len(), 0);
    }
}
