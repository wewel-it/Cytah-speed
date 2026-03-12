use std::collections::HashMap;
use crate::core::{Block, BlockHash};

/// Block storage layer
/// Handles persistent and in-memory storage of blocks
#[derive(Clone, Debug)]
pub struct BlockStore {
    blocks: HashMap<BlockHash, Block>,
}

impl BlockStore {
    /// Create new empty block store
    pub fn new() -> Self {
        BlockStore {
            blocks: HashMap::new(),
        }
    }

    /// Insert block into storage
    /// Returns error if block already exists or hash mismatch
    pub fn insert_block(&mut self, block: Block) -> Result<(), String> {
        if self.blocks.contains_key(&block.hash) {
            return Err(format!("Block {:?} already exists", block.hash));
        }

        // Validate block before insertion
        block.validate_basic()?;

        self.blocks.insert(block.hash, block);
        Ok(())
    }

    /// Retrieve block by hash
    pub fn get_block(&self, hash: &BlockHash) -> Option<Block> {
        self.blocks.get(hash).cloned()
    }

    /// Check if block exists
    pub fn block_exists(&self, hash: &BlockHash) -> bool {
        self.blocks.contains_key(hash)
    }

    /// Get all blocks
    pub fn get_all_blocks(&self) -> Vec<Block> {
        self.blocks.values().cloned().collect()
    }

    /// Get total block count
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// Get all block hashes
    pub fn get_all_hashes(&self) -> Vec<BlockHash> {
        self.blocks.keys().cloned().collect()
    }

    /// Delete block by hash (used in reorg scenarios)
    pub fn delete_block(&mut self, hash: &BlockHash) -> Option<Block> {
        self.blocks.remove(hash)
    }

    /// Roll back a sequence of blocks (e.g. during a chain reorganization).
    ///
    /// The caller should supply the hashes of the blocks being removed; the
    /// store will delete them, replay the remaining blocks into `state` to
    /// regenerate the correct state root, and return all transactions from the
    /// removed blocks so they can be re‑added to the mempool.
    pub fn rollback_blocks(
        &mut self,
        hashes: &[BlockHash],
        state: &mut crate::state::state_manager::StateManager,
        mempool: &crate::mempool::TxDagMempool,
    ) -> Result<(), String> {
        let mut removed_txs = Vec::new();
        for h in hashes {
            if let Some(block) = self.blocks.remove(h) {
                for tx in &block.transactions {
                    removed_txs.push(tx.clone());
                }
            }
        }

        // rebuild state from what's left in store
        let remaining_blocks: Vec<_> = self.blocks.values().cloned().collect();
        state.rebuild_from_blocks(&remaining_blocks)?;

        // push removed txs back into mempool so they aren't lost
        for tx in removed_txs {
            let _ = mempool.add_transaction(tx, vec![], None);
        }

        Ok(())
    }

    /// Clear entire store
    pub fn clear(&mut self) {
        self.blocks.clear();
    }

    /// Get blocks by parent hash
    pub fn get_blocks_by_parent(&self, parent_hash: &BlockHash) -> Vec<Block> {
        self.blocks
            .values()
            .filter(|block| block.header.parent_hashes.contains(parent_hash))
            .cloned()
            .collect()
    }

    /// Calculate total storage size
    pub fn total_size(&self) -> usize {
        self.blocks.values().map(|b| b.size_estimate()).sum()
    }

    /// Verify store integrity
    pub fn verify_integrity(&self) -> Result<(), String> {
        for (hash, block) in &self.blocks {
            if block.hash != *hash {
                return Err(format!("Hash mismatch for block {}", hex::encode(hash)));
            }
            block.validate_basic()?;
        }
        Ok(())
    }
}

impl Default for BlockStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
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
        Block::new(parents, 1000 + hash_seed.len() as u64, vec![tx], 42, 0, [0;20], [0;32])
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
        assert!(!store.block_exists(&"nonexistent".to_string()));
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

        let deleted = store.delete_block(&hash);
        assert_eq!(deleted, Some(block));
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
        let tx1 = crate::core::Transaction::new(from, to, 10, 0, 21000, 1);
        let mut b1 = create_test_block("b1", vec![]);
        b1.transactions = vec![tx1.clone()];
        let h1 = b1.hash;

        let tx2 = crate::core::Transaction::new(from, to, 20, 1, 21000, 1);
        let mut b2 = create_test_block("b2", vec![h1]);
        b2.transactions = vec![tx2.clone()];
        let h2 = b2.hash;

        store.insert_block(b1.clone()).unwrap();
        store.insert_block(b2.clone()).unwrap();

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
