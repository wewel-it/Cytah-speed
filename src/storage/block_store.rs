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

    /// Clear entire store
    pub fn clear(&mut self) {
        self.blocks.clear();
    }

    /// Get blocks by parent hash
    pub fn get_blocks_by_parent(&self, parent_hash: &BlockHash) -> Vec<Block> {
        self.blocks
            .values()
            .filter(|block| block.parent_hashes.contains(parent_hash))
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
        let tx = Transaction::new(from, to, amount, 0, 21000);
        Block::new(parents, 1000 + hash_seed.len() as u64, vec![tx], 42)
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
}
