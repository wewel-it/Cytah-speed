use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use rocksdb::{DB, ColumnFamilyDescriptor, Options, WriteBatch, IteratorMode};
use serde::{Serialize, Deserialize};
use crate::core::{Block, BlockHash};
use crate::state::state_manager::StateManager;
use crate::mempool::TxDagMempool;
use tracing::{info, warn, error, debug};

/// Column family names for RocksDB
const BLOCKS_CF: &str = "blocks";
const TRANSACTIONS_CF: &str = "transactions";
const METADATA_CF: &str = "metadata";

/// Block metadata for pruning and indexing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockMetadata {
    pub height: u64,
    pub timestamp: u64,
    pub transaction_count: usize,
    pub size_bytes: usize,
}

/// Persistent block storage using RocksDB
/// Supports column families for efficient storage and retrieval
#[derive(Clone, Debug)]
pub struct BlockStore {
    /// RocksDB instance
    db: Arc<DB>,
    /// In-memory cache for frequently accessed blocks
    cache: Arc<RwLock<HashMap<BlockHash, Block>>>,
    /// Cache size limit
    cache_size_limit: usize,
    /// Current cache size
    cache_size: Arc<RwLock<usize>>,
}

impl BlockStore {
    /// Create new block store with RocksDB backend using the default data path.
    ///
    /// In test runs, we use a per-process/per-thread temporary directory to avoid
    /// RocksDB locking issues when tests are executed in parallel.
    ///
    /// This helper panics on failure; callers that need error handling should
    /// use `new_with_path`.
    pub fn new() -> Self {
        // Detect test execution via `RUST_TEST_THREADS` which is set by the test harness.
        let default_path = if std::env::var("RUST_TEST_THREADS").is_ok() {
            // `ThreadId::as_u64` is unstable; use the Debug string representation instead.
            let thread_id = format!("{:?}", std::thread::current().id());
            format!("./data/block_store_test_{}_{}", std::process::id(), thread_id)
        } else {
            "./data/block_store".to_string()
        };

        Self::new_with_path(&default_path)
            .expect("Failed to create default BlockStore")
    }

    /// Create a new block store with an explicit path.
    pub fn new_with_path(path: &str) -> Result<Self, String> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.set_max_open_files(1000);
        opts.set_write_buffer_size(64 * 1024 * 1024); // 64MB
        opts.set_max_write_buffer_number(3);
        opts.set_target_file_size_base(64 * 1024 * 1024); // 64MB

        // Create column family descriptors
        let cf_descriptors = vec![
            ColumnFamilyDescriptor::new(BLOCKS_CF, Options::default()),
            ColumnFamilyDescriptor::new(TRANSACTIONS_CF, Options::default()),
            ColumnFamilyDescriptor::new(METADATA_CF, Options::default()),
        ];

        let db = match DB::open_cf_descriptors(&opts, path, cf_descriptors) {
            Ok(db) => db,
            Err(e) => {
                // Try to open without CFs first (for migration)
                match DB::open(&opts, path) {
                    Ok(mut db) => {
                        // Create CFs if they don't exist
                        if db.cf_handle(BLOCKS_CF).is_none() {
                            let _ = db.create_cf(BLOCKS_CF, &Options::default());
                        }
                        if db.cf_handle(TRANSACTIONS_CF).is_none() {
                            let _ = db.create_cf(TRANSACTIONS_CF, &Options::default());
                        }
                        if db.cf_handle(METADATA_CF).is_none() {
                            let _ = db.create_cf(METADATA_CF, &Options::default());
                        }
                        db
                    }
                    Err(_) => return Err(format!("Failed to open RocksDB: {}", e)),
                }
            }
        };

        Ok(Self {
            db: Arc::new(db),
            cache: Arc::new(RwLock::new(HashMap::new())),
            cache_size_limit: 100, // Cache up to 100 blocks
            cache_size: Arc::new(RwLock::new(0)),
        })
    }

    /// Insert block into persistent storage
    pub fn insert_block(&self, block: Block) -> Result<(), String> {
        let block_hash = block.hash;
        let block_data = bincode::serialize(&block)
            .map_err(|e| format!("Failed to serialize block: {}", e))?;

        let metadata = BlockMetadata {
            height: block.height,
            timestamp: block.header.timestamp,
            transaction_count: block.transactions.len(),
            size_bytes: block_data.len(),
        };

        let metadata_key = format!("meta_{}", hex::encode(&block_hash));
        let metadata_data = bincode::serialize(&metadata)
            .map_err(|e| format!("Failed to serialize metadata: {}", e))?;

        // Use write batch for atomic operation
        let mut batch = WriteBatch::default();

        // Store block
        let blocks_cf = self.db.cf_handle(BLOCKS_CF)
            .ok_or("Blocks column family not found")?;
        batch.put_cf(&blocks_cf, &block_hash, &block_data);

        // Store metadata
        let meta_cf = self.db.cf_handle(METADATA_CF)
            .ok_or("Metadata column family not found")?;
        batch.put_cf(&meta_cf, metadata_key.as_bytes(), &metadata_data);

        // Store individual transactions
        let tx_cf = self.db.cf_handle(TRANSACTIONS_CF)
            .ok_or("Transactions column family not found")?;
        for tx in &block.transactions {
            let tx_hash = tx.hash();
            let tx_data = bincode::serialize(tx)
                .map_err(|e| format!("Failed to serialize transaction: {}", e))?;
            batch.put_cf(&tx_cf, tx_hash, &tx_data);
        }

        // Execute batch
        self.db.write(batch)
            .map_err(|e| format!("Failed to write block: {}", e))?;

        // Add to cache
        self.add_to_cache(block_hash, block);

        debug!("Stored block {} with {} transactions", hex::encode(&block_hash[..8]), metadata.transaction_count);
        Ok(())
    }

    /// Retrieve block by hash
    pub fn get_block(&self, hash: &BlockHash) -> Option<Block> {
        // Check cache first
        if let Some(block) = self.get_from_cache(hash) {
            return Some(block);
        }

        // Check persistent storage
        let blocks_cf = match self.db.cf_handle(BLOCKS_CF) {
            Some(cf) => cf,
            None => return None,
        };

        match self.db.get_cf(&blocks_cf, hash) {
            Ok(Some(data)) => {
                if let Ok(block) = bincode::deserialize::<Block>(&data) {
                    // Add to cache
                    self.add_to_cache(*hash, block.clone());
                    Some(block)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Check if block exists
    pub fn block_exists(&self, hash: &BlockHash) -> bool {
        // Check cache first
        if self.cache.read().contains_key(hash) {
            return true;
        }

        // Check persistent storage
        let blocks_cf = match self.db.cf_handle(BLOCKS_CF) {
            Some(cf) => cf,
            None => return false,
        };

        match self.db.get_cf(&blocks_cf, hash) {
            Ok(opt) => opt.is_some(),
            Err(_) => false,
        }
    }

    /// Get block metadata
    pub fn get_block_metadata(&self, hash: &BlockHash) -> Result<Option<BlockMetadata>, String> {
        let meta_cf = self.db.cf_handle(METADATA_CF)
            .ok_or("Metadata column family not found")?;

        let metadata_key = format!("meta_{}", hex::encode(hash));

        match self.db.get_cf(&meta_cf, metadata_key.as_bytes()) {
            Ok(Some(data)) => {
                let metadata: BlockMetadata = bincode::deserialize(&data)
                    .map_err(|e| format!("Failed to deserialize metadata: {}", e))?;
                Ok(Some(metadata))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(format!("Database error: {}", e)),
        }
    }

    /// Get all block hashes (for iteration)
    pub fn get_all_block_hashes(&self) -> Result<Vec<BlockHash>, String> {
        let blocks_cf = self.db.cf_handle(BLOCKS_CF)
            .ok_or("Blocks column family not found")?;

        let mut hashes = Vec::new();
        let iter = self.db.iterator_cf(&blocks_cf, IteratorMode::Start);

        for item in iter {
            match item {
                Ok((key, _)) => {
                    if key.len() == 32 {
                        let mut hash = [0u8; 32];
                        hash.copy_from_slice(&key);
                        hashes.push(hash);
                    }
                }
                Err(e) => return Err(format!("Iterator error: {}", e)),
            }
        }

        Ok(hashes)
    }

    /// Get all block hashes (shortcut, ignores errors)
    pub fn get_all_hashes(&self) -> Vec<BlockHash> {
        self.get_all_block_hashes().unwrap_or_default()
    }

    /// Get all blocks (shortcut, ignores errors)
    pub fn get_all_blocks(&self) -> Vec<Block> {
        self.get_all_hashes()
            .into_iter()
            .filter_map(|h| self.get_block(&h))
            .collect()
    }

    /// Get total number of stored blocks (shortcut)
    pub fn block_count(&self) -> usize {
        self.get_all_hashes().len()
    }

    /// Get estimated total storage size in bytes (shortcut)
    pub fn total_size(&self) -> usize {
        self.get_stats().map(|s| s.total_size_bytes).unwrap_or(0)
    }

    /// Get blocks by height range (for pruning)
    pub fn get_blocks_by_height_range(&self, start_height: u64, end_height: u64) -> Result<Vec<Block>, String> {
        let all_hashes = self.get_all_block_hashes()?;
        let mut blocks = Vec::new();

        for hash in all_hashes {
            if let Some(metadata) = self.get_block_metadata(&hash)? {
                if metadata.height >= start_height && metadata.height <= end_height {
                    if let Some(block) = self.get_block(&hash) {
                        blocks.push(block);
                    }
                }
            }
        }

        // Sort by height
        blocks.sort_by_key(|b| b.height);
        Ok(blocks)
    }

    /// Delete block and its metadata (for pruning)
    pub fn delete_block(&self, hash: &BlockHash) -> Result<(), String> {
        let mut batch = WriteBatch::default();

        // Remove from cache
        self.remove_from_cache(hash);

        // Remove from database
        let blocks_cf = self.db.cf_handle(BLOCKS_CF)
            .ok_or("Blocks column family not found")?;
        let meta_cf = self.db.cf_handle(METADATA_CF)
            .ok_or("Metadata column family not found")?;
        let tx_cf = self.db.cf_handle(TRANSACTIONS_CF)
            .ok_or("Transactions column family not found")?;

        batch.delete_cf(&blocks_cf, hash);

        let metadata_key = format!("meta_{}", hex::encode(hash));
        batch.delete_cf(&meta_cf, metadata_key.as_bytes());

        // Get block to remove individual transactions
        if let Some(block) = self.get_block(hash) {
            for tx in &block.transactions {
                let tx_hash = tx.hash();
                batch.delete_cf(&tx_cf, tx_hash);
            }
        }

        self.db.write(batch)
            .map_err(|e| format!("Failed to delete block: {}", e))?;

        debug!("Deleted block {}", hex::encode(&hash[..8]));
        Ok(())
    }

    /// Verify integrity of the block store
    pub fn verify_integrity(&self) -> Result<(), String> {
        for block in self.get_all_blocks() {
            let expected_hash = block.calculate_hash();
            if block.hash != expected_hash {
                return Err(format!(
                    "Block hash mismatch detected: stored={} computed={}",
                    hex::encode(block.hash),
                    hex::encode(expected_hash)
                ));
            }
        }
        Ok(())
    }

    /// Remove all blocks from the store, including cached entries.
    pub fn clear(&self) {
        let hashes = self.get_all_hashes();
        for hash in hashes {
            let _ = self.delete_block(&hash);
        }

        // Clear in-memory cache
        let mut cache = self.cache.write();
        cache.clear();
        let mut cache_size = self.cache_size.write();
        *cache_size = 0;
    }

    /// Rollback the given blocks and rebuild state/mempool accordingly.
    pub fn rollback_blocks(
        &self,
        block_hashes: &[BlockHash],
        state: &mut StateManager,
        mempool: &TxDagMempool,
    ) -> Result<Option<Block>, String> {
        let mut last_removed = None;

        for hash in block_hashes {
            if let Some(block) = self.get_block(hash) {
                last_removed = Some(block.clone());
                self.delete_block(hash)?;

                // Re-add transactions to mempool (best-effort)
                for tx in &last_removed.as_ref().unwrap().transactions {
                    let _ = mempool.add_transaction(tx.clone(), vec![], None);
                }
            }
        }

        // Rebuild state from remaining blocks in height order
        let mut remaining_blocks = self.get_all_blocks();
        remaining_blocks.sort_by_key(|b| b.height);
        state.rebuild_from_blocks(&remaining_blocks)?;

        Ok(last_removed)
    }

    /// Get blocks whose parent list includes the given hash
    pub fn get_blocks_by_parent(&self, parent_hash: &BlockHash) -> Vec<Block> {
        self.get_all_blocks()
            .into_iter()
            .filter(|block| block.header.parent_hashes.contains(parent_hash))
            .collect()
    }

    /// Get database statistics
    pub fn get_stats(&self) -> Result<StorageStats, String> {
        let blocks_cf = self.db.cf_handle(BLOCKS_CF)
            .ok_or("Blocks column family not found")?;

        let mut block_count = 0;
        let mut total_size = 0;
        let iter = self.db.iterator_cf(&blocks_cf, IteratorMode::Start);

        for item in iter {
            match item {
                Ok((_, value)) => {
                    block_count += 1;
                    total_size += value.len();
                }
                Err(e) => return Err(format!("Iterator error: {}", e)),
            }
        }

        Ok(StorageStats {
            block_count,
            total_size_bytes: total_size,
            cache_size: *self.cache_size.read(),
            cache_hit_rate: 0.0, // Would need to track hits/misses
        })
    }

    /// Compact database for optimization
    pub fn compact(&self) -> Result<(), String> {
        let blocks_cf = self.db.cf_handle(BLOCKS_CF)
            .ok_or("Blocks column family not found")?;
        let meta_cf = self.db.cf_handle(METADATA_CF)
            .ok_or("Metadata column family not found")?;
        let tx_cf = self.db.cf_handle(TRANSACTIONS_CF)
            .ok_or("Transactions column family not found")?;

        // Compact all column families
        self.db.compact_range_cf(&blocks_cf, None::<&[u8]>, None::<&[u8]>);
        self.db.compact_range_cf(&meta_cf, None::<&[u8]>, None::<&[u8]>);
        self.db.compact_range_cf(&tx_cf, None::<&[u8]>, None::<&[u8]>);

        info!("Database compaction completed");
        Ok(())
    }

    // Cache management methods
    fn add_to_cache(&self, hash: BlockHash, block: Block) {
        let mut cache = self.cache.write();
        let mut cache_size = self.cache_size.write();

        // Remove old entries if cache is full
        while *cache_size >= self.cache_size_limit && !cache.is_empty() {
            // Remove a random entry (simple LRU would be better)
            if let Some(key) = cache.keys().next().cloned() {
                cache.remove(&key);
                *cache_size -= 1;
            }
        }

        if cache.insert(hash, block).is_none() {
            *cache_size += 1;
        }
    }

    fn get_from_cache(&self, hash: &BlockHash) -> Option<Block> {
        self.cache.read().get(hash).cloned()
    }

    fn remove_from_cache(&self, hash: &BlockHash) {
        let mut cache = self.cache.write();
        if cache.remove(hash).is_some() {
            let mut cache_size = self.cache_size.write();
            *cache_size -= 1;
        }
    }
}

/// Storage statistics
#[derive(Debug, Clone)]
pub struct StorageStats {
    pub block_count: usize,
    pub total_size_bytes: usize,
    pub cache_size: usize,
    pub cache_hit_rate: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_block_store_basic() {
        let temp_dir = tempdir().unwrap();
        let store = BlockStore::new_with_path(temp_dir.path().to_str().unwrap()).unwrap();

        // Create a test block
        let block = Block::new(vec![], 0, vec![], 0, 0, 0, [1; 20], [0; 32]);

        // Store block
        store.insert_block(block.clone()).unwrap();

        // Retrieve block
        let retrieved = store.get_block(&block.hash).expect("Block not found");
        assert_eq!(retrieved.hash, block.hash);

        // Check existence
        assert!(store.block_exists(&block.hash));
        assert!(!store.block_exists(&[2; 32]));
    }

    #[test]
    fn test_storage_stats() {
        let temp_dir = tempdir().unwrap();
        let store = BlockStore::new_with_path(temp_dir.path().to_str().unwrap()).unwrap();

        let stats = store.get_stats().unwrap();
        assert_eq!(stats.block_count, 0);
        assert_eq!(stats.total_size_bytes, 0);
    }
}

impl Default for BlockStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod block_store_tests {
    use super::*;
    use crate::core::Transaction;

    fn create_test_block(hash_seed: &str, parents: Vec<BlockHash>) -> Block {
        let seed_bytes = hash_seed.as_bytes();
        let mut from: [u8; 20] = [1; 20];
        let mut to: [u8; 20] = [2; 20];
        for (i, &b) in seed_bytes.iter().take(20).enumerate() {
            from[i] = from[i].wrapping_add(b);
        }
        for (i, &b) in seed_bytes.iter().skip(20).take(20).enumerate() {
            to[i] = to[i].wrapping_add(b);
        }
        let amount = 100 + seed_bytes.len() as u64;
        let tx = Transaction::new(from, to, amount, 0, 21000, 1);
        Block::new(parents, 1000 + hash_seed.len() as u64, vec![tx], 42, 0, 0, [0;20], [0;32])
    }

    #[test]
    fn test_insert_and_retrieve_block() {
        let mut store = BlockStore::new();
        let block = create_test_block("test1", vec![]);

        let result = store.insert_block(block.clone());
        assert!(result.is_ok());
        assert_eq!(store.get_block(&block.hash), Some(block));
    }

    #[test]
    fn test_duplicate_block_rejection() {
        let mut store = BlockStore::new();
        let block = create_test_block("test1", vec![]);

        store.insert_block(block.clone()).unwrap();
        let result = store.insert_block(block);
        assert!(result.is_err());
    }

    #[test]
    fn test_block_exists() {
        let mut store = BlockStore::new();
        let block = create_test_block("test1", vec![]);
        let hash = block.hash.clone();

        store.insert_block(block).unwrap();
        assert!(store.block_exists(&hash));
        assert!(!store.block_exists(&[0; 32]));
    }

    #[test]
    fn test_get_all_blocks() {
        let mut store = BlockStore::new();
        let block1 = create_test_block("test1", vec![]);
        let block2 = create_test_block("test2", vec![block1.hash.clone()]);

        store.insert_block(block1.clone()).unwrap();
        store.insert_block(block2.clone()).unwrap();

        let blocks = store.get_all_blocks();
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn test_block_count() {
        let mut store = BlockStore::new();
        assert_eq!(store.block_count(), 0);

        let block1 = create_test_block("test1", vec![]);
        store.insert_block(block1).unwrap();
        assert_eq!(store.block_count(), 1);

        let block2 = create_test_block("test2", vec![]);
        store.insert_block(block2).unwrap();
        assert_eq!(store.block_count(), 2);
    }

    #[test]
    fn test_delete_block() {
        let mut store = BlockStore::new();
        let block = create_test_block("test1", vec![]);
        let hash = block.hash.clone();

        store.insert_block(block.clone()).unwrap();
        assert!(store.block_exists(&hash));

        store.delete_block(&hash).unwrap();
        assert!(!store.block_exists(&hash));
    }

    #[test]
    fn test_get_blocks_by_parent() {
        let mut store = BlockStore::new();
        let parent = create_test_block("parent", vec![]);
        let parent_hash = parent.hash.clone();

        let child1 = create_test_block("child1", vec![parent_hash.clone()]);
        let child2 = create_test_block("child2", vec![parent_hash.clone()]);

        store.insert_block(parent).unwrap();
        store.insert_block(child1).unwrap();
        store.insert_block(child2).unwrap();

        let children = store.get_blocks_by_parent(&parent_hash);
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn test_verify_integrity() {
        let mut store = BlockStore::new();
        let block = create_test_block("test1", vec![]);
        store.insert_block(block).unwrap();

        assert!(store.verify_integrity().is_ok());
    }

    #[test]
    fn test_clear_store() {
        let mut store = BlockStore::new();
        let block = create_test_block("test1", vec![]);
        store.insert_block(block).unwrap();
        assert_eq!(store.block_count(), 1);

        store.clear();
        assert_eq!(store.block_count(), 0);
    }

    #[test]
    fn test_rollback_blocks_restores_state_and_mempool() {
        use crate::state::state_manager::StateManager;
        use crate::mempool::TxDagMempool;
        use std::sync::Arc;

        let mut store = BlockStore::new();
        let mut state = StateManager::new();
        let mempool = TxDagMempool::new(100, Arc::new(parking_lot::Mutex::new(state.clone())), 1);

        // build two sequential blocks with one tx each
        let from = [1u8;20];
        let to = [2u8;20];
        let mut tx1 = crate::core::Transaction::new(from, to, 10, 0, 21000, 1);
        tx1.sign(&secp256k1::SecretKey::from_slice(&[1; 32]).unwrap()).unwrap();
        let mut b1 = create_test_block("b1", vec![]);
        b1.transactions = vec![tx1.clone()];
        b1.hash = b1.calculate_hash();
        let h1 = b1.hash;

        let mut tx2 = crate::core::Transaction::new(from, to, 20, 1, 21000, 1);
        tx2.sign(&secp256k1::SecretKey::from_slice(&[1; 32]).unwrap()).unwrap();
        let mut b2 = create_test_block("b2", vec![h1]);
        b2.transactions = vec![tx2.clone()];
        b2.hash = b2.calculate_hash();
        let h2 = b2.hash;

        store.insert_block(b1.clone()).unwrap();
        store.insert_block(b2.clone()).unwrap();

        // fund sender so transactions can be applied
        state.state_tree.update_account(from, crate::state::state_tree::Account::new(100000, 0));
        // apply both txs to state so it reflects full chain
        state.apply_transaction(&tx1).unwrap();
        state.apply_transaction(&tx2).unwrap();

        // rollback second block
        store.rollback_blocks(&[h2], &mut state, &mempool).unwrap();

        // block two should no longer exist
        assert!(!store.block_exists(&h2));
        // state should equal state after only tx1
        let acc = state.get_account(&from).unwrap();
        assert!(acc.balance < u64::MAX);
        // mempool should contain tx2 again
        assert_eq!(mempool.size(), 1);
    }
}
