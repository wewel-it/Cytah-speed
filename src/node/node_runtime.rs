use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use parking_lot::RwLock;
use tokio::time::{Duration, interval};

use crate::core::{Transaction, BlockHash};
use crate::dag::blockdag::BlockDAG;
use crate::consensus::ghostdag::GHOSTDAGEngine;
use crate::state::state_manager::StateManager;
use crate::execution::transaction_executor::TransactionExecutor;
use crate::mempool::TxDagMempool;
use crate::storage::BlockStore;
use crate::block::BlockProducer;
use crate::finality::FinalityEngine;
use crate::contracts::ContractRegistry;
use crate::network::{DiscoveryManager, StateSyncManager};

/// Node runtime yang menjalankan blockchain node
/// 
/// Komponen:
/// - BlockDAG: struktur DAG dari blok
/// - GHOSTDAG: consensus engine
/// - StateManager: state execution
/// - TxDagMempool: transaksi mempool
/// - BlockProducer: menghasilkan blok baru
/// - FinalityEngine: menghitung finality
/// - ContractRegistry: persistent contract storage
/// - DiscoveryManager: peer discovery orchestration
/// - StateSyncManager: fast state synchronization
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
    /// Optional pruning helper (rolling window)
    pub pruner: Option<Arc<parking_lot::Mutex<crate::storage::pruning::RollingWindowPruner>>>,
    /// Contract registry for persistent smart contract storage
    pub contract_registry: Arc<parking_lot::Mutex<ContractRegistry>>,
    /// Discovery manager for network peer orchestration
    pub discovery_manager: Arc<DiscoveryManager>,
    /// State sync manager for fast catchup
    pub state_sync_manager: Option<Arc<parking_lot::Mutex<StateSyncManager>>>,
    /// Node ID
    pub node_id: [u8; 20],
    /// Block production interval (ms)
    pub block_interval: u64,
    /// Whether mining is enabled (producing blocks)
    pub mining_enabled: Arc<AtomicBool>,
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
        Self::new_with_config_and_db(node_id, block_interval, confirmation_depth, _max_peers, mempool_size, "./data/contracts.db")
    }

    pub fn new_with_config_and_db(
        node_id: [u8; 20],
        block_interval: u64,
        confirmation_depth: usize,
        _max_peers: usize,
        mempool_size: usize,
        db_path: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let blockdag = Arc::new(RwLock::new(BlockDAG::new()));
        // create state manager first so we can hand a reference to the mempool
        let state_manager = Arc::new(parking_lot::Mutex::new(StateManager::new()));
        let mempool = Arc::new(TxDagMempool::new(
            mempool_size,
            state_manager.clone(),
            crate::mempool::tx_dag_mempool::DEFAULT_MIN_GAS_PRICE,
        ));
        let ghostdag = Arc::new(RwLock::new(GHOSTDAGEngine::new(2)));

        // Derive per-node DB paths to avoid test collisions when running in parallel
        let executor_db_path = format!("{}-executor", db_path);
        let registry_db_path = format!("{}-registry", db_path);
        let pruner_db_path = format!("{}-pruner", db_path);

        let executor = Arc::new(parking_lot::Mutex::new(TransactionExecutor::new_with_db_path(&executor_db_path)));
        let finality_engine = Arc::new(FinalityEngine::new(confirmation_depth));
        // pruning is optional; we always create a pruner but wrap in Arc+Mutex for
        // interior mutability since `execute_block` borrows &self.
        let pruner = Some(Arc::new(parking_lot::Mutex::new(
            crate::storage::pruning::RollingWindowPruner::new(
                &pruner_db_path,
                BlockStore::new_with_path(&format!("{}-blocks", db_path)).unwrap(),
                100_000,
                1000,
                1000,
            ).unwrap(),
        )));

        let block_producer = Arc::new(BlockProducer::new(
            mempool.clone(),
            blockdag.clone(),
            state_manager.clone(),
            node_id,
            0,
            100,
            crate::consensus::mining::DaaConfig::default(),
        ));

        // Initialize contract registry with persistent RocksDB backend
        let contract_registry = Arc::new(parking_lot::Mutex::new(
            ContractRegistry::new(&registry_db_path)
                .map_err(|e| format!("Failed to initialize contract registry: {}", e))?
        ));

        // Initialize discovery manager for network peer orchestration
        let discovery_manager = Arc::new(DiscoveryManager::new(libp2p::PeerId::random()));

        // Note: StateSyncManager requires mpsc channel from P2P node
        // It will be initialized in P2PNode and can be accessed via RPC state
        let state_sync_manager = None;

        Ok(Self {
            blockdag,
            ghostdag,
            state_manager,
            mempool,
            block_producer,
            finality_engine,
            executor,
            pruner,
            contract_registry,
            discovery_manager,
            state_sync_manager,
            node_id,
            block_interval,
            mining_enabled: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Initialize node
    pub async fn initialize(&self) -> Result<(), String> {
        tracing::info!("Initializing node...");

        // Create genesis block and ensure state includes genesis allocation
        self.blockdag.write().create_genesis_if_empty();
        {
            let mut state = self.state_manager.lock();
            state.initialize_tokenomics();
        }
        tracing::info!("✓ Genesis block created and tokenomics initialized");

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

        // Only produce blocks when mining is enabled
        if !self.mining_enabled.load(Ordering::Relaxed) {
            return Ok(());
        }

        // Only produce block if there are transactions in the mempool
        if (*self.mempool).size() == 0 {
            return Ok(());
        }

        tracing::debug!("Attempting to produce block...");

        drop(dag); // Release read lock

        // Determine difficulty using DAA and recent DAG state
        let difficulty = crate::consensus::next_difficulty(&self.blockdag.read());
        // Determine base fee using fee market calculation
        let base_fee = crate::consensus::next_base_fee(&self.blockdag.read());
        // update mempool with current base fee so it can enforce and order
        self.mempool.set_base_fee(base_fee);
        let state_root = self.state_manager.lock().get_state_root();
        match self.block_producer.create_block(difficulty, base_fee, state_root) {
            Ok(mut block) => {
                tracing::info!(
                    "Block produced: {} with {} transactions",
                    hex::encode(&block.hash[..16]),
                    block.transactions.len()
                );

                // Annotate consensus metadata (chain height, blue score, topo index)
                // based on the current DAG state.
                {
                    let dag_snapshot = self.blockdag.read().clone();
                    let mut ghostdag = self.ghostdag.write();
                    // Ensure ghostdag has the latest DAG snapshot for ordering
                    ghostdag.attach_dag(dag_snapshot.clone());
                    ghostdag.annotate_block(&dag_snapshot, &mut block)?;
                }

                // Add block to DAG
                {
                    let mut dag = self.blockdag.write();
                    dag.insert_block(block.clone())?;
                }

                // Update ghostdag engine with latest DAG state
                let dag_snapshot = self.blockdag.read().clone();
                self.ghostdag.write().attach_dag(dag_snapshot);

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
        // Validate block reward against the economic model before applying state.
        {
            let emitted = self.state_manager.lock().get_emitted_supply();
            let (timestamps, chain_progress) = {
                let dag = self.blockdag.read();
                let chain_height = dag
                    .get_tip_blocks()
                    .iter()
                    .map(|b| b.header.chain_height)
                    .max()
                    .unwrap_or(0);

                let timestamps = dag.get_recent_timestamps(
                    crate::consensus::mining::RewardConfig::default().activity_window_size,
                );

                (timestamps, chain_height as f64)
            };

            let expected_reward = crate::consensus::mining::calculate_expected_block_reward(
                emitted,
                chain_progress,
                &timestamps,
                block.transactions.len(),
                &crate::consensus::mining::RewardConfig::default(),
            );

            if expected_reward != block.reward {
                return Err(format!("Invalid block reward: expected {} but block has {}", expected_reward, block.reward));
            }
        }

        let mut exec = self.executor.lock();
        let result = exec.execute_block(block);

        if result.success {
            // apply transactions from block (state changes already applied during
            // execution).  We still call the hook in case it is used later.
            let mut state = self.state_manager.lock();
            state.apply_block(&block.transactions)?;

            // credit miner reward and collected fees
            let mut total_miner_credit: u64 = 0;
            // reward is fractional; floor for simplicity.
            total_miner_credit = total_miner_credit.saturating_add(block.reward);
            total_miner_credit = total_miner_credit.saturating_add(result.total_fees);
            state.credit_account(block.producer, total_miner_credit)?;

            // Track emitted supply: base reward + fees (fees become part of mining supply)
            state.add_emitted_supply(block.reward.saturating_add(result.total_fees));

            // perform pruning if configured (state manager helper wraps logic)
            if let Some(pruner) = &self.pruner {
                let mut p = pruner.lock();
                let height = self.blockdag.read().block_count() as u64;
                state.snapshot_and_prune(height, &mut p);
            }

            tracing::debug!("Block executed: {} txs, miner credit {}", result.executed_transactions, total_miner_credit);
        }

        Ok(())
    }



    /// Check finality based on the current DAG state
    fn check_finality(&self) -> Result<(), String> {
        // If we have no blocks yet, nothing to finalize
        if self.blockdag.read().get_all_blocks().is_empty() {
            return Ok(());
        }

        // Update ghostdag engine with latest DAG state snapshot
        let dag_snapshot = self.blockdag.read().clone();
        {
            let mut ghostdag = self.ghostdag.write();
            ghostdag.attach_dag(dag_snapshot.clone());
        }

        // Generate ordering and run finality calculation
        if let Ok(_ordering) = self.ghostdag.write().generate_ordering() {
            self.finality_engine.compute_finality(&dag_snapshot)?;
        }

        Ok(())
    }

    /// Add transaction ke mempool
    pub async fn add_transaction(&self, tx: Transaction) -> Result<(), String> {
        // Validasi
        (*self.mempool).validate_transaction(&tx)?;

        // Add ke mempool
        (*self.mempool).add_transaction(tx.clone(), vec![], None)?;

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

    /// Get connected peers (P2P disabled in this runtime)
    pub fn get_connected_peers(&self) -> Vec<String> {
        Vec::new()
    }

    /// Get peer count
    pub fn get_peer_count(&self) -> usize {
        // In this simplified runtime, we don't have actual P2P peers
        // This would be implemented in the full P2P node
        0
    }

    /// Get detailed peer information
    pub async fn get_detailed_peers(&self) -> Result<Vec<PeerInfo>, String> {
        // In this simplified runtime, return empty list
        // Full implementation would query P2P network
        Ok(Vec::new())
    }

    /// Get account balance from current state
    pub fn get_balance(&self, address: &crate::core::transaction::Address) -> u64 {
        self.state_manager.lock().get_balance(*address)
    }

    /// Get account nonce from current state
    pub fn get_nonce(&self, address: &crate::core::transaction::Address) -> u64 {
        self.state_manager
            .lock()
            .get_account(address)
            .map(|acc| acc.nonce)
            .unwrap_or(0)
    }

    /// Start mining process
    pub async fn start_mining(&self) -> Result<(), String> {
        tracing::info!("Starting mining process...");
        self.mining_enabled.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Stop mining process
    pub async fn stop_mining(&self) -> Result<(), String> {
        tracing::info!("Stopping mining process...");
        self.mining_enabled.store(false, Ordering::Relaxed);
        Ok(())
    }

    /// Get system metrics
    pub async fn get_metrics(&self) -> Result<SystemMetrics, String> {
        // Calculate basic metrics
        let mempool_size = self.get_mempool_size();
        let block_count = self.blockdag.read().get_all_blocks().len();
        let finality_height = self.get_finality_height().unwrap_or(0);

        Ok(SystemMetrics {
            tps: 0.0, // Would need transaction rate tracking
            network_latency_ms: 0.0, // Would need network monitoring
            memory_mb: 0, // Would need system monitoring
            storage_mb: 0, // Would need storage monitoring
            mempool_size,
            block_count,
            finality_height,
        })
    }

    /// Prune storage to specified window
    pub async fn prune_storage(&self, window: usize) -> Result<usize, String> {
        if let Some(pruner) = &self.pruner {
            // For manual pruning, we'll simulate pruning by calling maybe_prune
            // with a high current height to trigger pruning
            let mut pruner_guard = pruner.lock();
            let mut state = self.state_manager.lock();
            pruner_guard.maybe_prune(200_000 + window as u64, &mut state);
            Ok(window) // Return window size as indication of pruning
        } else {
            Err("Pruning not configured".to_string())
        }
    }

    /// Validate transaction by hash
    pub async fn validate_transaction(&self, tx_hash: &[u8; 32]) -> Result<bool, String> {
        // Check if transaction exists in mempool or blocks
        if (*self.mempool).get_transaction(tx_hash).is_some() {
            return Ok(true);
        }

        // Check in block history (simplified check)
        let blocks = self.blockdag.read().get_all_blocks();
        for block in blocks {
            for tx in &block.transactions {
                if &tx.hash() == tx_hash {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }
}

/// Peer information structure
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub id: String,
    pub address: String,
    pub status: String,
}

/// System metrics structure
#[derive(Debug, Clone)]
pub struct SystemMetrics {
    pub tps: f64,
    pub network_latency_ms: f64,
    pub memory_mb: u64,
    pub storage_mb: u64,
    pub mempool_size: usize,
    pub block_count: usize,
    pub finality_height: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_node_creation() {
        let node = NodeRuntime::new_with_config_and_db([1; 20], 500, 3, 10, 1000, "./data/test_node_create.db").unwrap();
        assert_eq!(node.node_id, [1; 20]);
        assert_eq!(node.block_interval, 500);
    }

    #[tokio::test]
    async fn test_node_initialization() {
        let node = NodeRuntime::new_with_config_and_db([2; 20], 500, 3, 10, 1000, "./data/test_node_init.db").unwrap();
        let result = node.initialize().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_tips() {
        let node = NodeRuntime::new_with_config_and_db([3; 20], 500, 3, 10, 1000, "./data/test_node_tips.db").unwrap();
        node.initialize().await.unwrap();
        let tips = node.get_tips();
        assert!(!tips.is_empty());
    }

    #[tokio::test]
    async fn test_get_mempool_size() {
        let node = NodeRuntime::new_with_config_and_db([4; 20], 500, 3, 10, 1000, "./data/test_node_mempool.db").unwrap();
        assert_eq!(node.get_mempool_size(), 0);
    }

    #[tokio::test]
    async fn test_get_connected_peers() {
        let node = NodeRuntime::new_with_config_and_db([5; 20], 500, 3, 10, 1000, "./data/test_node_peers.db").unwrap();
        let peers = node.get_connected_peers();
        assert_eq!(peers.len(), 0);
    }

    #[tokio::test]
    async fn test_miner_reward_credit() {
        // construct a simple environment and manually execute a block with one tx
        let node = NodeRuntime::new_with_config_and_db([6; 20], 500, 3, 10, 1000, "./data/test_node_miner.db").unwrap();
        node.initialize().await.unwrap();

        // give sender enough balance
        let sender: crate::core::transaction::Address = [9; 20];
        node.state_manager.lock().state_tree.update_account(sender, crate::state::state_tree::Account::new(1000000, 0));
        let receiver: crate::core::transaction::Address = [8; 20];

        let mut tx = crate::core::transaction::Transaction::new_transfer(sender, receiver, 100, 0, 21000, 1);
        tx.sign(&secp256k1::SecretKey::from_slice(&[1; 32]).unwrap()).unwrap();

        let state_root = node.state_manager.lock().get_state_root();
        let block = crate::core::Block::new(vec![], 0, vec![tx], 0, 0, 0, node.node_id, state_root);
        // call the private helper which includes miner credit logic
        node.execute_block(&block).unwrap();

        // Miner should receive block reward + transaction fees
        let expected_credit = block.reward.saturating_add(21000);
        let state = node.state_manager.lock();
        let miner_acc = state.get_account(&block.producer).unwrap();
        assert_eq!(miner_acc.balance, expected_credit);
    }

    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}

    #[test]
    fn test_node_runtime_send_sync() {
        assert_send::<NodeRuntime>();
        assert_sync::<NodeRuntime>();
    }
}
