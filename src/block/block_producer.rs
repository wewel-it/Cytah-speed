use crate::core::{Block, Transaction};
use crate::mempool::TxDagMempool;
use crate::dag::blockdag::BlockDAG;
use parking_lot::RwLock;
use std::sync::Arc;

/// Block producer untuk membuat blok baru dari transaksi mempool
#[derive(Clone)]
pub struct BlockProducer {
    /// Reference ke mempool
    mempool: Arc<TxDagMempool>,
    /// Reference ke BlockDAG
    dag: Arc<RwLock<BlockDAG>>,
    /// Producer ID/address
    pub producer_id: [u8; 20],
    /// Nonce untuk blok berikutnya
    nonce: Arc<RwLock<u64>>,
    /// Minimum transaksi dalam blok
    min_transactions: usize,
    /// Maximum transaksi dalam blok
    max_transactions: usize,
}

impl BlockProducer {
    /// Buat block producer baru
    pub fn new(
        mempool: Arc<TxDagMempool>,
        dag: Arc<RwLock<BlockDAG>>,
        producer_id: [u8; 20],
        min_transactions: usize,
        max_transactions: usize,
    ) -> Self {
        Self {
            mempool,
            dag,
            producer_id,
            nonce: Arc::new(RwLock::new(0)),
            min_transactions,
            max_transactions,
        }
    }

    /// Buat blok baru
    /// 
    /// Proses:
    /// 1. Ambil ready transactions dari mempool
    /// 2. Filter untuk max_transactions
    /// 3. Pilih parent blocks dari DAG tips
    /// 4. Buat blok baru
    /// 5. Return blok yang dibuat
    /// Create a candidate block using the supplied difficulty target (in bits).
    /// The returned block is *not* mined; its nonce may be arbitrary.  Mining is
    /// handled by `mine_block`.
    pub fn create_block(&self, difficulty: u32, base_fee: u64, _state_root: [u8;32]) -> Result<Block, String> {
        // Ambil ready transactions
        let mut ready_txs = (*self.mempool).get_ready_transactions();

        // Filter untuk minimum transaksi
        if ready_txs.len() < self.min_transactions && ready_txs.len() > 0 {
            // Bisa create blok dengan sedikit transaksi jika tidak ada lagi (mempool kosong)
            // Tapi kalau ada transaksi, tunggu sampai minimum
            if (*self.mempool).size() > self.min_transactions {
                return Err(format!(
                    "Not enough ready transactions: {} < {}",
                    ready_txs.len(),
                    self.min_transactions
                ));
            }
        }

        // Limit ke max_transactions
        if ready_txs.len() > self.max_transactions {
            ready_txs.truncate(self.max_transactions);
        }

        // Sort by priority fee (gas_price - base_fee) descending
        ready_txs.sort_by_key(|mt| std::u64::MAX - mt.transaction.gas_price.saturating_sub(base_fee));
        // Extract hanya transaction objects (bukan MempoolTransaction wrapper)
        let transactions: Vec<Transaction> = ready_txs
            .into_iter()
            .map(|mt| mt.transaction.clone())
            .collect();

        // Tentukan parent blocks
        let dag = self.dag.read();
        let parent_hashes = dag.get_tips().to_vec();
        drop(dag);

        // Buat timestamp
        let timestamp = chrono::Utc::now().timestamp() as u64;

        // Increment nonce
        let block_nonce = {
            let mut nonce = self.nonce.write();
            let current = *nonce;
            *nonce += 1;
            current
        };

        // Create block with provided difficulty
        let state_root = {
            // fetch current state root from DAG if available, otherwise zero
            [0u8;32]
        };
        let block = Block::new(parent_hashes, timestamp, transactions, block_nonce, difficulty, base_fee, self.producer_id, state_root);

        // Validasi block
        block.validate_basic()?;

        Ok(block)
    }

    /// Attempt to mine a block by iterating nonces until the PoW target is met.
    /// Uses Rayon to parallelize the nonce search over the full 64‑bit space.
    pub fn mine_block(&self, difficulty: u32, base_fee: u64, state_root: [u8;32]) -> Result<Block, String> {
        // create a base block template with nonce=0
        let mut base = self.create_block(difficulty, base_fee, state_root)?;

        // closure to test difficulty
        let meets = |b: &Block| crate::consensus::meets_difficulty(&b.calculate_hash(), b.header.difficulty);

        // search in parallel
        use rayon::prelude::*;

        if let Some(nonce) = (0u64..u64::MAX)
            .into_par_iter()
            .find_any(|&n| {
                let mut candidate = base.clone();
                candidate.header.nonce = n;
                meets(&candidate)
            })
        {
            base.header.nonce = nonce;
            base.hash = base.calculate_hash();
            // validate reward and pow again
            base.validate_basic()?;
            return Ok(base);
        }

        Err("unable to find valid nonce".to_string())
    }

    /// Dapatkan transaksi yang siap digunakan di blok
    pub fn get_pending_transactions(&self) -> Vec<Transaction> {
        (*self.mempool)
            .get_ready_transactions()
            .into_iter()
            .take(self.max_transactions)
            .map(|mt| mt.transaction)
            .collect()
    }

    /// Reset producer nonce
    pub fn reset_nonce(&self) {
        *self.nonce.write() = 0;
    }

    /// Dapatkan current nonce
    pub fn get_nonce(&self) -> u64 {
        *self.nonce.read()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secp256k1::SecretKey;

    fn create_test_transaction(from: [u8; 20], to: [u8; 20], amount: u64, nonce: u64) -> Transaction {
        let mut tx = Transaction::new(from, to, amount, nonce, 21000, 1);
        let secret_key = SecretKey::from_slice(&[1; 32]).unwrap();
        tx.sign(&secret_key).ok();
        tx
    }

    #[test]
    fn test_block_producer_creation() {
        let state = Arc::new(parking_lot::Mutex::new(StateManager::new()));
        let mempool = Arc::new(TxDagMempool::new(1000, state.clone(), crate::mempool::tx_dag_mempool::DEFAULT_MIN_GAS_PRICE));
        let dag = Arc::new(RwLock::new(BlockDAG::new()));
        let producer_id = [1u8; 20];

        dag.write().create_genesis_if_empty();

        let producer = BlockProducer::new(mempool.clone(), dag, producer_id, 1, 10);
        assert_eq!(producer.producer_id, producer_id);
    }

    #[test]
    fn test_create_block_empty_mempool() {
        let state = Arc::new(parking_lot::Mutex::new(StateManager::new()));
        let mempool = Arc::new(TxDagMempool::new(1000, state.clone(), crate::mempool::tx_dag_mempool::DEFAULT_MIN_GAS_PRICE));
        let dag = Arc::new(RwLock::new(BlockDAG::new()));
        let producer_id = [1u8; 20];

        dag.write().create_genesis_if_empty();

        let producer = BlockProducer::new(mempool.clone(), dag, producer_id, 0, 10);

        let result = producer.create_block(0, [0;32]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_block_with_transactions() {
        let state = Arc::new(parking_lot::Mutex::new(StateManager::new()));
        let mempool = Arc::new(TxDagMempool::new(1000, state.clone(), crate::mempool::tx_dag_mempool::DEFAULT_MIN_GAS_PRICE));
        let dag = Arc::new(RwLock::new(BlockDAG::new()));
        let producer_id = [1u8; 20];

        dag.write().create_genesis_if_empty();

        let from = [5u8; 20];
        let to = [6u8; 20];
        let tx = create_test_transaction(from, to, 100, 0);

        mempool.add_transaction(tx, vec![], None).unwrap();

        let mut producer = BlockProducer::new(mempool.clone(), dag, producer_id, 1, 10);
        let result = producer.create_block(0, [0;32]);

        assert!(result.is_ok());
        if let Ok(block) = result {
            assert_eq!(block.transactions.len(), 1);
            assert_eq!(block.producer, producer_id);
        }
    }

    #[test]
    fn test_create_block_respects_max_transactions() {
        let state = Arc::new(parking_lot::Mutex::new(StateManager::new()));
        let mempool = Arc::new(TxDagMempool::new(1000, state.clone(), crate::mempool::tx_dag_mempool::DEFAULT_MIN_GAS_PRICE));
        let dag = Arc::new(RwLock::new(BlockDAG::new()));
        let producer_id = [1u8; 20];

        dag.write().create_genesis_if_empty();

        // Add 10 transactions
        for i in 0..10 {
            let from = [1u8; 20];
            let to = [2u8; 20];
            let tx = create_test_transaction(from, to, 100 + i, i as u64);
            mempool.add_transaction(tx, vec![], None).unwrap();
        }

        let producer = BlockProducer::new(mempool.clone(), dag, producer_id, 1, 5);
        let block = producer.create_block(0, [0;32]).unwrap();

        assert_eq!(block.transactions.len(), 5);
        assert_eq!(block.producer, producer_id);
    }

    #[test]
    fn test_block_producer_nonce() {
        let state = Arc::new(parking_lot::Mutex::new(StateManager::new()));
        let mempool = Arc::new(TxDagMempool::new(1000, state.clone(), crate::mempool::tx_dag_mempool::DEFAULT_MIN_GAS_PRICE));
        let dag = Arc::new(RwLock::new(BlockDAG::new()));
        let producer_id = [1u8; 20];

        dag.write().create_genesis_if_empty();

        let producer = BlockProducer::new(mempool.clone(), dag, producer_id, 0, 10);
        assert_eq!(producer.get_nonce(), 0);

        let from = [5u8; 20];
        let to = [6u8; 20];
        let tx = create_test_transaction(from, to, 100, 0);
        mempool.add_transaction(tx, vec![], None).unwrap();

        let _block1 = producer.create_block(0, [0;32]);
        assert_eq!(producer.get_nonce(), 1);

        let _block2 = producer.create_block(0, [0;32]);
        assert_eq!(producer.get_nonce(), 2);
    }

    #[test]
    fn test_mine_block_finds_nonce() {
        let state = Arc::new(parking_lot::Mutex::new(StateManager::new()));
        let mempool = Arc::new(TxDagMempool::new(1000, state.clone(), crate::mempool::tx_dag_mempool::DEFAULT_MIN_GAS_PRICE));
        let dag = Arc::new(RwLock::new(BlockDAG::new()));
        let producer_id = [1u8; 20];
        dag.write().create_genesis_if_empty();

        let producer = BlockProducer::new(mempool.clone(), dag, producer_id, 0, 10);
        // choose a very low difficulty so mining finishes quickly
        let block = producer.mine_block(4, [0;32]).expect("mining should succeed");
        assert!(crate::consensus::meets_difficulty(&block.hash, 4));
        assert_eq!(block.producer, producer_id);
    }
}
