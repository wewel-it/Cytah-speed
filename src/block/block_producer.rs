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
    pub fn create_block(&self) -> Result<Block, String> {
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

        // Create block
        let block = Block::new(parent_hashes, timestamp, transactions, block_nonce);

        // Validasi block
        block.validate_basic()?;

        Ok(block)
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
        let mut tx = Transaction::new(from, to, amount, nonce, 21000);
        let secret_key = SecretKey::from_slice(&[1; 32]).unwrap();
        tx.sign(&secret_key).ok();
        tx
    }

    #[test]
    fn test_block_producer_creation() {
        let mempool = Arc::new(TxDagMempool::new(1000));
        let dag = Arc::new(RwLock::new(BlockDAG::new()));
        let producer_id = [1u8; 20];

        dag.write().create_genesis_if_empty();

        let producer = BlockProducer::new(mempool.clone(), dag, producer_id, 1, 10);
        assert_eq!(producer.producer_id, producer_id);
    }

    #[test]
    fn test_create_block_empty_mempool() {
        let mempool = Arc::new(TxDagMempool::new(1000));
        let dag = Arc::new(RwLock::new(BlockDAG::new()));
        let producer_id = [1u8; 20];

        dag.write().create_genesis_if_empty();

        let producer = BlockProducer::new(mempool.clone(), dag, producer_id, 0, 10);

        let result = producer.create_block();
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_block_with_transactions() {
        let mempool = Arc::new(TxDagMempool::new(1000));
        let dag = Arc::new(RwLock::new(BlockDAG::new()));
        let producer_id = [1u8; 20];

        dag.write().create_genesis_if_empty();

        let from = [5u8; 20];
        let to = [6u8; 20];
        let tx = create_test_transaction(from, to, 100, 0);

        mempool.add_transaction(tx, vec![]).unwrap();

        let mut producer = BlockProducer::new(mempool.clone(), dag, producer_id, 1, 10);
        let result = producer.create_block();

        assert!(result.is_ok());
        if let Ok(block) = result {
            assert_eq!(block.transactions.len(), 1);
        }
    }

    #[test]
    fn test_create_block_respects_max_transactions() {
        let mempool = Arc::new(TxDagMempool::new(1000));
        let dag = Arc::new(RwLock::new(BlockDAG::new()));
        let producer_id = [1u8; 20];

        dag.write().create_genesis_if_empty();

        // Add 10 transactions
        for i in 0..10 {
            let from = [1u8; 20];
            let to = [2u8; 20];
            let tx = create_test_transaction(from, to, 100 + i, i as u64);
            mempool.add_transaction(tx, vec![]).unwrap();
        }

        let producer = BlockProducer::new(mempool.clone(), dag, producer_id, 1, 5);
        let block = producer.create_block().unwrap();

        assert_eq!(block.transactions.len(), 5);
    }

    #[test]
    fn test_block_producer_nonce() {
        let mempool = Arc::new(TxDagMempool::new(1000));
        let dag = Arc::new(RwLock::new(BlockDAG::new()));
        let producer_id = [1u8; 20];

        dag.write().create_genesis_if_empty();

        let producer = BlockProducer::new(mempool.clone(), dag, producer_id, 0, 10);
        assert_eq!(producer.get_nonce(), 0);

        let from = [5u8; 20];
        let to = [6u8; 20];
        let tx = create_test_transaction(from, to, 100, 0);
        mempool.add_transaction(tx, vec![]).unwrap();

        let _block1 = producer.create_block();
        assert_eq!(producer.get_nonce(), 1);

        let _block2 = producer.create_block();
        assert_eq!(producer.get_nonce(), 2);
    }
}
