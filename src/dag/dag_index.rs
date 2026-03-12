use std::collections::{HashMap, HashSet, VecDeque};
use crate::core::{Block, BlockHash};
use crate::storage::BlockStore;

/// DAG Index for efficient traversal and parent/child lookups
#[derive(Clone, Debug)]
pub struct DAGIndex {
    parent_to_children: HashMap<BlockHash, Vec<BlockHash>>,
    tips: HashSet<BlockHash>,
}

impl DAGIndex {
    /// Create new empty DAG index
    pub fn new() -> Self {
        DAGIndex {
            parent_to_children: HashMap::new(),
            tips: HashSet::new(),
        }
    }

    /// Build index from block store
    pub fn build_from_store(store: &BlockStore) -> Self {
        let mut index = DAGIndex::new();
        let blocks = store.get_all_blocks();

        for block in &blocks {
            for parent_hash in &block.header.parent_hashes {
                index
                    .parent_to_children
                    .entry(*parent_hash)
                    .or_insert_with(Vec::new)
                    .push(block.hash);
            }
        }

        // Calculate tips
        let mut potential_tips: HashSet<_> = blocks.iter().map(|b| b.hash).collect();
        for block in &blocks {
            for parent in &block.header.parent_hashes {
                potential_tips.remove(parent);
            }
        }
        index.tips = potential_tips;

        index
    }

    /// Get direct children of a block
    pub fn get_children(&self, hash: &BlockHash) -> Vec<BlockHash> {
        self.parent_to_children
            .get(hash)
            .cloned()
            .unwrap_or_default()
    }

    /// Get all children recursively
    pub fn get_all_descendants(
        &self,
        hash: &BlockHash,
        _store: &BlockStore,
    ) -> HashSet<BlockHash> {
        let mut descendants = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(hash.clone());

        while let Some(current) = queue.pop_front() {
            for child in self.get_children(&current) {
                if !descendants.contains(&child) {
                    descendants.insert(child.clone());
                    queue.push_back(child);
                }
            }
        }

        descendants
    }

    /// Get all ancestors of a block recursively
    pub fn get_all_ancestors(
        &self,
        hash: &BlockHash,
        store: &BlockStore,
    ) -> HashSet<BlockHash> {
        let mut ancestors = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(hash.clone());

        while let Some(current) = queue.pop_front() {
            if let Some(block) = store.get_block(&current) {
                for parent_hash in &block.header.parent_hashes {
                    if !ancestors.contains(parent_hash) {
                        ancestors.insert(parent_hash.clone());
                        queue.push_back(parent_hash.clone());
                    }
                }
            }
        }

        ancestors
    }

    /// Get all tips (leaf blocks in DAG)
    pub fn get_tips(&self) -> Vec<BlockHash> {
        self.tips.iter().cloned().collect()
    }

    /// Check if block is a tip
    pub fn is_tip(&self, hash: &BlockHash) -> bool {
        self.tips.contains(hash)
    }

    /// Update tips after block insertion
    pub fn update_tips_after_insert(&mut self, new_block: &Block) {
        // Add new block as potential tip
        self.tips.insert(new_block.hash.clone());

        // Remove parents from tips (they now have children)
        for parent_hash in &new_block.header.parent_hashes {
            self.tips.remove(parent_hash);
        }

        // Update parent_to_children mapping
        for parent_hash in &new_block.header.parent_hashes {
            self.parent_to_children
                .entry(parent_hash.clone())
                .or_insert_with(Vec::new)
                .push(new_block.hash.clone());
        }
    }

    /// Find Lowest Common Ancestor (LCA) of two blocks
    pub fn find_lca(&self, hash1: &BlockHash, hash2: &BlockHash, store: &BlockStore) -> Option<BlockHash> {
        let ancestors1 = self.get_all_ancestors(hash1, store);
        let mut queue = VecDeque::new();
        queue.push_back(hash2.clone());
        let mut visited = HashSet::new();

        while let Some(current) = queue.pop_front() {
            if visited.contains(&current) {
                continue;
            }
            visited.insert(current.clone());

            if ancestors1.contains(&current) || hash1 == &current {
                return Some(current);
            }

            if let Some(block) = store.get_block(&current) {
                for parent in &block.header.parent_hashes {
                    if !visited.contains(parent) {
                        queue.push_back(parent.clone());
                    }
                }
            }
        }

        None
    }

    /// Get all blocks reachable from given hashes
    pub fn get_reachable_blocks(
        &self,
        hashes: &[BlockHash],
        store: &BlockStore,
    ) -> HashSet<BlockHash> {
        let mut reachable = HashSet::new();
        let mut queue = VecDeque::new();

        for hash in hashes {
            queue.push_back(hash.clone());
        }

        while let Some(current) = queue.pop_front() {
            if reachable.contains(&current) {
                continue;
            }
            reachable.insert(current.clone());

            if let Some(block) = store.get_block(&current) {
                for parent in &block.header.parent_hashes {
                    if !reachable.contains(parent) {
                        queue.push_back(parent.clone());
                    }
                }
            }
        }

        reachable
    }

    /// Topological order of blocks (parents before children)
    pub fn get_topological_order(&self, store: &BlockStore) -> Vec<BlockHash> {
        let mut in_degree: HashMap<BlockHash, usize> = HashMap::new();
        let mut queue = VecDeque::new();
        let mut result = Vec::new();

        let blocks = store.get_all_blocks();
        for block in &blocks {
            in_degree.insert(block.hash.clone(), block.header.parent_hashes.len());
            if block.header.parent_hashes.is_empty() {
                queue.push_back(block.hash.clone());
            }
        }

        while let Some(current) = queue.pop_front() {
            result.push(current.clone());

            for child in self.get_children(&current) {
                if let Some(count) = in_degree.get_mut(&child) {
                    *count -= 1;
                    if *count == 0 {
                        queue.push_back(child);
                    }
                }
            }
        }

        result
    }

    /// Get depth of block (longest path from genesis)
    pub fn get_block_depth(&self, hash: &BlockHash, store: &BlockStore) -> usize {
        let mut depth = 0;
        let mut current_queue = vec![hash.clone()];

        while !current_queue.is_empty() {
            let mut next_queue = Vec::new();

            for block_hash in current_queue {
                if let Some(block) = store.get_block(&block_hash) {
                    if !block.header.parent_hashes.is_empty() {
                        for parent in &block.header.parent_hashes {
                            if !next_queue.contains(parent) {
                                next_queue.push(parent.clone());
                            }
                        }
                    }
                }
            }

            if next_queue.is_empty() {
                break;
            }

            current_queue = next_queue;
            depth += 1;
        }

        depth
    }

    /// Check if hash1 is ancestor of hash2
    pub fn is_ancestor(&self, ancestor_hash: &BlockHash, block_hash: &BlockHash, store: &BlockStore) -> bool {
        let ancestors = self.get_all_ancestors(block_hash, store);
        ancestors.contains(ancestor_hash)
    }

    /// Get coparents (siblings with shared parent)
    pub fn get_coparents(&self, hash: &BlockHash, store: &BlockStore) -> HashSet<BlockHash> {
        let mut coparents = HashSet::new();

        if let Some(block) = store.get_block(hash) {
            for parent_hash in &block.header.parent_hashes {
                for child in self.get_children(parent_hash) {
                    if child != *hash {
                        coparents.insert(child);
                    }
                }
            }
        }

        coparents
    }
}

impl Default for DAGIndex {
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
        Block::new(parents, 1000 + hash_seed.len() as u64, vec![tx], 42, 0, 0, [0;20], [0;32])
    }

    #[test]
    fn test_build_index_from_store() {
        let mut store = BlockStore::new();
        let block1 = create_test_block("block1", vec![]);
        let block2 = create_test_block("block2", vec![block1.hash.clone()]);

        store.insert_block(block1.clone()).unwrap();
        store.insert_block(block2.clone()).unwrap();

        let index = DAGIndex::build_from_store(&store);
        assert_eq!(index.get_tips(), vec![block2.hash]);
    }

    #[test]
    fn test_get_children() {
        let mut store = BlockStore::new();
        let parent = create_test_block("parent", vec![]);
        let child1 = create_test_block("child1", vec![parent.hash.clone()]);
        let child2 = create_test_block("child2", vec![parent.hash.clone()]);

        store.insert_block(parent.clone()).unwrap();
        store.insert_block(child1.clone()).unwrap();
        store.insert_block(child2.clone()).unwrap();

        let index = DAGIndex::build_from_store(&store);
        let children = index.get_children(&parent.hash);
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn test_get_all_descendants() {
        let mut store = BlockStore::new();
        let genesis = create_test_block("genesis", vec![]);
        let child1 = create_test_block("child1", vec![genesis.hash.clone()]);
        let child2 = create_test_block("child2", vec![child1.hash.clone()]);

        store.insert_block(genesis.clone()).unwrap();
        store.insert_block(child1.clone()).unwrap();
        store.insert_block(child2.clone()).unwrap();

        let index = DAGIndex::build_from_store(&store);
        let descendants = index.get_all_descendants(&genesis.hash, &store);
        assert_eq!(descendants.len(), 2);
    }

    #[test]
    fn test_is_ancestor() {
        let mut store = BlockStore::new();
        let genesis = create_test_block("genesis", vec![]);
        let child = create_test_block("child", vec![genesis.hash.clone()]);

        store.insert_block(genesis.clone()).unwrap();
        store.insert_block(child.clone()).unwrap();

        let index = DAGIndex::build_from_store(&store);
        assert!(index.is_ancestor(&genesis.hash, &child.hash, &store));
        assert!(!index.is_ancestor(&child.hash, &genesis.hash, &store));
    }

    #[test]
    fn test_topological_order() {
        let mut store = BlockStore::new();
        let genesis = create_test_block("genesis", vec![]);
        let child = create_test_block("child", vec![genesis.hash.clone()]);

        store.insert_block(genesis.clone()).unwrap();
        store.insert_block(child.clone()).unwrap();

        let index = DAGIndex::build_from_store(&store);
        let order = index.get_topological_order(&store);
        assert_eq!(order.len(), 2);
        assert_eq!(order[0], genesis.hash);
        assert_eq!(order[1], child.hash);
    }

    #[test]
    fn test_find_lca() {
        let mut store = BlockStore::new();
        let genesis = create_test_block("genesis", vec![]);
        let child1 = create_test_block("child1", vec![genesis.hash.clone()]);
        let child2 = create_test_block("child2", vec![genesis.hash.clone()]);

        store.insert_block(genesis.clone()).unwrap();
        store.insert_block(child1.clone()).unwrap();
        store.insert_block(child2.clone()).unwrap();

        let index = DAGIndex::build_from_store(&store);
        let lca = index.find_lca(&child1.hash, &child2.hash, &store);
        assert_eq!(lca, Some(genesis.hash));
    }
}
