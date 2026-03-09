use std::collections::HashSet;
use crate::core::{Block, BlockHash, Transaction};
use crate::storage::BlockStore;
use crate::dag::dag_index::DAGIndex;

/// Main BlockDAG Engine
/// Manages the entire DAG structure, validation, and operations
#[derive(Clone, Debug)]
pub struct BlockDAG {
    store: BlockStore,
    index: DAGIndex,
}

impl BlockDAG {
    /// Create new empty BlockDAG
    pub fn new() -> Self {
        BlockDAG {
            store: BlockStore::new(),
            index: DAGIndex::new(),
        }
    }

    /// Create BlockDAG with genesis block
    pub fn with_genesis(genesis: Block) -> Result<Self, String> {
        let mut dag = BlockDAG::new();
        
        if !genesis.is_genesis() {
            return Err("Block must be genesis block (no parents)".to_string());
        }

        dag.insert_block(genesis)?;
        Ok(dag)
    }

    /// Create automatic genesis block if DAG is empty
    pub fn create_genesis_if_empty(&mut self) {
        if self.store.block_count() == 0 {
            let _genesis = self.create_genesis_block();
        }
    }

    /// Create automatic genesis block
    pub fn create_genesis_block(&mut self) -> Block {
        let genesis = Block::new(
            vec![],
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            vec![],
            0,
        );

        let _ = self.insert_block(genesis.clone());
        genesis
    }

    /// Insert new block into DAG
    /// Performs full validation and updates DAG state
    pub fn insert_block(&mut self, block: Block) -> Result<(), String> {
        // Step 1: Validate block structure
        block.validate_basic()?;

        // Step 2: Check if block already exists
        if self.store.block_exists(&block.hash) {
            return Err(format!("Block {} already exists in DAG", hex::encode(block.hash)));
        }

        // Step 3: Validate references - all parents must exist
        if !block.parent_hashes.is_empty() {
            for parent_hash in &block.parent_hashes {
                if !self.store.block_exists(parent_hash) {
                    return Err(format!("Parent block {} does not exist", hex::encode(parent_hash)));
                }
            }
        } else {
            // Only genesis block can have no parents
            if self.store.block_count() > 0 {
                return Err("Non-genesis block cannot have zero parents".to_string());
            }
        }

        // Step 4: Check for duplicate blocks (same content)
        for existing_block in self.store.get_all_blocks() {
            if existing_block.hash == block.hash {
                return Err("Duplicate block detected".to_string());
            }
        }

        // Step 5: Store block
        self.store.insert_block(block.clone())?;

        // Step 6: Update DAG index
        self.index.update_tips_after_insert(&block);

        Ok(())
    }

    /// Insert multiple blocks at once (batch operation)
    pub fn insert_blocks(&mut self, blocks: Vec<Block>) -> Result<usize, String> {
        let mut inserted = 0;

        for block in blocks {
            match self.insert_block(block) {
                Ok(()) => inserted += 1,
                Err(e) => {
                    // Return error but report how many succeeded
                    return Err(format!("Failed to insert block {}: {}. Successfully inserted {} blocks", 
                        inserted, e, inserted));
                }
            }
        }

        Ok(inserted)
    }

    /// Get block by hash
    pub fn get_block(&self, hash: &BlockHash) -> Option<Block> {
        self.store.get_block(hash)
    }

    /// Get all blocks
    pub fn get_all_blocks(&self) -> Vec<Block> {
        self.store.get_all_blocks()
    }

    /// Get all tips (leaf blocks)
    pub fn get_tips(&self) -> Vec<BlockHash> {
        self.index.get_tips()
    }

    /// Get tip blocks (actual Block objects)
    pub fn get_tip_blocks(&self) -> Vec<Block> {
        self.get_tips()
            .iter()
            .filter_map(|hash| self.get_block(hash))
            .collect()
    }

    /// Get children of a block
    pub fn get_children(&self, hash: &BlockHash) -> Vec<BlockHash> {
        self.index.get_children(hash)
    }

    /// Get parent blocks
    pub fn get_parents(&self, hash: &BlockHash) -> Vec<Block> {
        self.get_block(hash)
            .map(|block| {
                block
                    .parent_hashes
                    .iter()
                    .filter_map(|ph| self.get_block(ph))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Validate entire DAG
    pub fn validate(&self) -> Result<(), String> {
        // Validate block store integrity
        self.store.verify_integrity()?;

        // Validate all parent references exist
        for block in self.store.get_all_blocks() {
            for parent_hash in &block.parent_hashes {
                if !self.store.block_exists(parent_hash) {
                    return Err(format!(
                        "Block {} references non-existent parent {}",
                        hex::encode(block.hash), hex::encode(parent_hash)
                    ));
                }
            }
        }

        // Validate no duplicate blocks
        let hashes = self.store.get_all_hashes();
        if hashes.len() != hashes.iter().collect::<std::collections::HashSet<_>>().len() {
            return Err("Duplicate block hashes detected".to_string());
        }

        // Validate tips
        let blocks = self.store.get_all_blocks();
        if blocks.is_empty() {
            return Err("DAG must have at least one block (genesis)".to_string());
        }

        // Check tips are actually leaves
        let tip_set: HashSet<_> = self.get_tips().into_iter().collect();
        for block in &blocks {
            if block.parent_hashes.is_empty() && tip_set.len() > 1 {
                // Genesis should not be a tip if we have other blocks
                let in_some_parent_set = blocks
                    .iter()
                    .any(|b| b.parent_hashes.contains(&block.hash));
                if in_some_parent_set {
                    return Err("Genesis block is referenced but still marked as tip".to_string());
                }
            }
        }

        Ok(())
    }

    /// Get DAG statistics
    pub fn get_stats(&self) -> DAGStats {
        let blocks = self.store.get_all_blocks();
        let tips = self.get_tips();

        let total_txs: usize = blocks.iter().map(|b| b.transactions.len()).sum();
        let avg_parents = if !blocks.is_empty() {
            blocks.iter().map(|b| b.parent_hashes.len()).sum::<usize>() / blocks.len()
        } else {
            0
        };

        DAGStats {
            total_blocks: self.store.block_count(),
            total_transactions: total_txs,
            num_tips: tips.len(),
            avg_parents_per_block: avg_parents,
            total_size_bytes: self.store.total_size(),
        }
    }

    /// Get ancestors of a block
    pub fn get_ancestors(&self, hash: &BlockHash) -> HashSet<BlockHash> {
        self.index.get_all_ancestors(hash, &self.store)
    }

    /// Get descendants of a block
    pub fn get_descendants(&self, hash: &BlockHash) -> HashSet<BlockHash> {
        self.index.get_all_descendants(hash, &self.store)
    }

    /// Check if hash1 is ancestor of hash2
    pub fn is_ancestor(&self, ancestor_hash: &BlockHash, block_hash: &BlockHash) -> bool {
        self.index.is_ancestor(ancestor_hash, block_hash, &self.store)
    }

    /// Find Lowest Common Ancestor (LCA)
    pub fn find_lca(&self, hash1: &BlockHash, hash2: &BlockHash) -> Option<BlockHash> {
        self.index.find_lca(hash1, hash2, &self.store)
    }

    /// Get topological order of blocks
    pub fn get_topological_order(&self) -> Vec<BlockHash> {
        self.index.get_topological_order(&self.store)
    }

    /// Get coparents of a block (siblings sharing parent)
    pub fn get_coparents(&self, hash: &BlockHash) -> HashSet<BlockHash> {
        self.index.get_coparents(hash, &self.store)
    }

    /// Get block depth (longest path from genesis)
    pub fn get_block_depth(&self, hash: &BlockHash) -> usize {
        self.index.get_block_depth(hash, &self.store)
    }

    /// Create new block with current tips as parents
    pub fn create_block_on_tips(&self, transactions: Vec<Transaction>) -> Block {
        let tips = self.get_tips();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Block::new(tips, timestamp, transactions, 0)
    }

    /// Rebuild index from scratch
    pub fn rebuild_index(&mut self) {
        self.index = DAGIndex::build_from_store(&self.store);
    }

    /// Get block count
    pub fn block_count(&self) -> usize {
        self.store.block_count()
    }

    /// Export full DAG structure
    pub fn export_dag(&self) -> DAGExport {
        DAGExport {
            blocks: self.store.get_all_blocks(),
            tips: self.get_tips(),
            statistics: self.get_stats(),
        }
    }
}

impl Default for BlockDAG {
    fn default() -> Self {
        Self::new()
    }
}

/// DAG Statistics
#[derive(Clone, Debug)]
pub struct DAGStats {
    pub total_blocks: usize,
    pub total_transactions: usize,
    pub num_tips: usize,
    pub avg_parents_per_block: usize,
    pub total_size_bytes: usize,
}

/// DAG Export structure
#[derive(Clone, Debug)]
pub struct DAGExport {
    pub blocks: Vec<Block>,
    pub tips: Vec<BlockHash>,
    pub statistics: DAGStats,
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_blockdag_creation() {
        let dag = BlockDAG::new();
        assert_eq!(dag.block_count(), 0);
    }

    #[test]
    fn test_insert_genesis_block() {
        let genesis = create_test_block("genesis", vec![]);
        let dag = BlockDAG::with_genesis(genesis.clone()).unwrap();

        assert_eq!(dag.block_count(), 1);
        assert_eq!(dag.get_tips(), vec![genesis.hash]);
    }

    #[test]
    fn test_insert_non_genesis_without_parent_fails() {
        let mut dag = BlockDAG::new();
        // Insert genesis first
        let genesis = create_test_block("genesis", vec![]);
        dag.insert_block(genesis).unwrap();

        // Try to insert non-genesis without parent - should fail
        let block = create_test_block("test", vec![]);
        let result = dag.insert_block(block);
        assert!(result.is_err());
    }

    #[test]
    fn test_insert_block_with_missing_parent_fails() {
        let mut dag = BlockDAG::new();
        let genesis = create_test_block("genesis", vec![]);
        dag.insert_block(genesis).unwrap();

        let block = create_test_block("test", vec!["nonexistent".to_string()]);
        let result = dag.insert_block(block);
        assert!(result.is_err());
    }

    #[test]
    fn test_insert_block_chain() {
        let mut dag = BlockDAG::new();
        let genesis = create_test_block("genesis", vec![]);
        dag.insert_block(genesis.clone()).unwrap();

        let child = create_test_block("child", vec![genesis.hash.clone()]);
        dag.insert_block(child.clone()).unwrap();

        assert_eq!(dag.block_count(), 2);
        assert_eq!(dag.get_tips(), vec![child.hash]);
    }

    #[test]
    fn test_dag_tips_update() {
        let mut dag = BlockDAG::new();
        let genesis = create_test_block("genesis", vec![]);
        dag.insert_block(genesis.clone()).unwrap();

        let child1 = create_test_block("child1", vec![genesis.hash.clone()]);
        dag.insert_block(child1.clone()).unwrap();

        let child2 = create_test_block("child2", vec![genesis.hash.clone()]);
        dag.insert_block(child2.clone()).unwrap();

        let tips = dag.get_tips();
        assert_eq!(tips.len(), 2);
        assert!(tips.contains(&child1.hash));
        assert!(tips.contains(&child2.hash));
        assert!(!tips.contains(&genesis.hash));
    }

    #[test]
    fn test_parent_child_index() {
        let mut dag = BlockDAG::new();
        let parent = create_test_block("parent", vec![]);
        dag.insert_block(parent.clone()).unwrap();

        let child1 = create_test_block("child1", vec![parent.hash.clone()]);
        let child2 = create_test_block("child2", vec![parent.hash.clone()]);

        dag.insert_block(child1.clone()).unwrap();
        dag.insert_block(child2.clone()).unwrap();

        let children = dag.get_children(&parent.hash);
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn test_multiple_parents() {
        let mut dag = BlockDAG::new();
        let genesis = create_test_block("genesis", vec![]);
        dag.insert_block(genesis.clone()).unwrap();

        let parent1 = create_test_block("parent1", vec![genesis.hash.clone()]);
        let parent2 = create_test_block("parent2", vec![genesis.hash.clone()]);

        dag.insert_block(parent1.clone()).unwrap();
        dag.insert_block(parent2.clone()).unwrap();

        let child = create_test_block(
            "child",
            vec![parent1.hash.clone(), parent2.hash.clone()],
        );
        dag.insert_block(child.clone()).unwrap();

        assert_eq!(dag.block_count(), 4);
        assert_eq!(dag.get_tips(), vec![child.hash]);
    }

    #[test]
    fn test_validate_dag() {
        let mut dag = BlockDAG::new();
        let genesis = create_test_block("genesis", vec![]);
        dag.insert_block(genesis).unwrap();

        assert!(dag.validate().is_ok());
    }

    #[test]
    fn test_get_ancestors() {
        let mut dag = BlockDAG::new();
        let genesis = create_test_block("genesis", vec![]);
        dag.insert_block(genesis.clone()).unwrap();

        let child = create_test_block("child", vec![genesis.hash.clone()]);
        dag.insert_block(child.clone()).unwrap();

        let ancestors = dag.get_ancestors(&child.hash);
        assert_eq!(ancestors.len(), 1);
        assert!(ancestors.contains(&genesis.hash));
    }

    #[test]
    fn test_is_ancestor() {
        let mut dag = BlockDAG::new();
        let genesis = create_test_block("genesis", vec![]);
        dag.insert_block(genesis.clone()).unwrap();

        let child = create_test_block("child", vec![genesis.hash.clone()]);
        dag.insert_block(child.clone()).unwrap();

        assert!(dag.is_ancestor(&genesis.hash, &child.hash));
        assert!(!dag.is_ancestor(&child.hash, &genesis.hash));
    }

    #[test]
    fn test_find_lca() {
        let mut dag = BlockDAG::new();
        let genesis = create_test_block("genesis", vec![]);
        dag.insert_block(genesis.clone()).unwrap();

        let child1 = create_test_block("child1", vec![genesis.hash.clone()]);
        let child2 = create_test_block("child2", vec![genesis.hash.clone()]);

        dag.insert_block(child1.clone()).unwrap();
        dag.insert_block(child2.clone()).unwrap();

        let lca = dag.find_lca(&child1.hash, &child2.hash);
        assert_eq!(lca, Some(genesis.hash));
    }

    #[test]
    fn test_topological_order() {
        let mut dag = BlockDAG::new();
        let genesis = create_test_block("genesis", vec![]);
        dag.insert_block(genesis.clone()).unwrap();

        let child = create_test_block("child", vec![genesis.hash.clone()]);
        dag.insert_block(child.clone()).unwrap();

        let order = dag.get_topological_order();
        assert_eq!(order.len(), 2);
        assert!(order.iter().position(|h| h == &genesis.hash) < order.iter().position(|h| h == &child.hash));
    }

    #[test]
    fn test_dag_stats() {
        let mut dag = BlockDAG::new();
        let genesis = create_test_block("genesis", vec![]);
        dag.insert_block(genesis).unwrap();

        let stats = dag.get_stats();
        assert_eq!(stats.total_blocks, 1);
        assert_eq!(stats.num_tips, 1);
    }

    #[test]
    fn test_create_genesis_if_empty() {
        let mut dag = BlockDAG::new();
        assert_eq!(dag.block_count(), 0);

        dag.create_genesis_if_empty();
        assert_eq!(dag.block_count(), 1);
    }

    #[test]
    fn test_insert_blocks_batch() {
        let mut dag = BlockDAG::new();
        let genesis = create_test_block("genesis", vec![]);
        dag.insert_block(genesis.clone()).unwrap();

        let child1 = create_test_block("child1", vec![genesis.hash.clone()]);
        let child2 = create_test_block("child2", vec![genesis.hash.clone()]);

        let blocks = vec![child1, child2];
        let inserted = dag.insert_blocks(blocks).unwrap();
        assert_eq!(inserted, 2);
        assert_eq!(dag.block_count(), 3);
    }

    #[test]
    fn test_export_dag() {
        let mut dag = BlockDAG::new();
        let genesis = create_test_block("genesis", vec![]);
        dag.insert_block(genesis).unwrap();

        let export = dag.export_dag();
        assert_eq!(export.blocks.len(), 1);
        assert_eq!(export.tips.len(), 1);
        assert_eq!(export.statistics.total_blocks, 1);
    }
}
