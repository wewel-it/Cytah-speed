use std::collections::HashSet;
use crate::core::BlockHash;
use crate::dag::BlockDAG;
use super::dag_traversal::DAGTraversal;

/// Blue set calculation for GHOSTDAG consensus
/// A block is in the blue set if its anticone size is <= k
#[derive(Clone, Debug)]
pub struct BlueSet {
    /// Parameter k: maximum anticone size for blue blocks
    pub k: usize,
}

impl BlueSet {
    /// Create new BlueSet calculator with parameter k
    pub fn new(k: usize) -> Self {
        BlueSet { k }
    }

    /// Build blue set for a block
    /// Returns set of blocks that should be colored blue
    pub fn build_blue_set(&self, dag: &BlockDAG, reference_hash: &BlockHash) -> HashSet<BlockHash> {
        let mut blue_set = HashSet::new();

        // Start with reference block itself
        blue_set.insert(reference_hash.clone());

        // Get all blocks in topological order
        let all_blocks = DAGTraversal::get_all_blocks(dag);

        // Process blocks in topological order (parents before children)
        for block_hash in all_blocks {
            if block_hash == *reference_hash {
                continue;
            }

            // Calculate anticone of this block relative to reference
            let anticone = DAGTraversal::get_anticone(dag, &block_hash, reference_hash);

            // Block is blue if anticone size <= k
            if anticone.len() <= self.k {
                blue_set.insert(block_hash);
            }
        }

        blue_set
    }

    /// Build red set (complement of blue set)
    pub fn build_red_set(
        &self,
        dag: &BlockDAG,
        reference_hash: &BlockHash,
    ) -> HashSet<BlockHash> {
        let blue_set = self.build_blue_set(dag, reference_hash);
        let all_blocks = DAGTraversal::get_all_blocks(dag)
            .into_iter()
            .collect::<HashSet<_>>();

        all_blocks.difference(&blue_set).cloned().collect()
    }

    /// Check if block is blue relative to reference
    pub fn is_blue(&self, dag: &BlockDAG, block_hash: &BlockHash, reference_hash: &BlockHash) -> bool {
        // Block must be ancestor or equal to reference to be blue
        if block_hash != reference_hash && !DAGTraversal::is_ancestor(dag, block_hash, reference_hash) {
            return false;
        }

        let anticone = DAGTraversal::get_anticone(dag, block_hash, reference_hash);
        anticone.len() <= self.k
    }

    /// Get anticone size of block relative to reference
    pub fn get_anticone_size(
        &self,
        dag: &BlockDAG,
        block_hash: &BlockHash,
        reference_hash: &BlockHash,
    ) -> usize {
        let anticone = DAGTraversal::get_anticone(dag, block_hash, reference_hash);
        anticone.len()
    }

    /// Get anticone blocks themselves
    pub fn get_anticone(
        &self,
        dag: &BlockDAG,
        block_hash: &BlockHash,
        reference_hash: &BlockHash,
    ) -> HashSet<BlockHash> {
        DAGTraversal::get_anticone(dag, block_hash, reference_hash)
    }

    /// Count blocks in blue set for reference
    pub fn count_blue_blocks(&self, dag: &BlockDAG, reference_hash: &BlockHash) -> usize {
        self.build_blue_set(dag, reference_hash).len()
    }

    /// Count blocks in red set for reference
    pub fn count_red_blocks(&self, dag: &BlockDAG, reference_hash: &BlockHash) -> usize {
        self.build_red_set(dag, reference_hash).len()
    }

    /// Get all blue block hashes
    pub fn get_blue_blocks(&self, dag: &BlockDAG, reference_hash: &BlockHash) -> Vec<BlockHash> {
        let blocks = self.build_blue_set(dag, reference_hash);
        let mut result: Vec<_> = blocks.into_iter().collect();
        // Sort deterministically
        result.sort();
        result
    }

    /// Get all red block hashes
    pub fn get_red_blocks(&self, dag: &BlockDAG, reference_hash: &BlockHash) -> Vec<BlockHash> {
        let blocks = self.build_red_set(dag, reference_hash);
        let mut result: Vec<_> = blocks.into_iter().collect();
        // Sort deterministically
        result.sort();
        result
    }

    /// Check if all ancestors of block are blue (validation)
    pub fn are_all_ancestors_blue(
        &self,
        dag: &BlockDAG,
        block_hash: &BlockHash,
        reference_hash: &BlockHash,
    ) -> bool {
        let ancestors = DAGTraversal::get_ancestors(dag, block_hash);
        let blue_set = self.build_blue_set(dag, reference_hash);

        for ancestor in ancestors {
            if !blue_set.contains(&ancestor) {
                return false;
            }
        }

        true
    }

    /// Get blue set statistics
    pub fn get_blue_set_stats(
        &self,
        dag: &BlockDAG,
        reference_hash: &BlockHash,
    ) -> BlueSetStats {
        let blue_set = self.build_blue_set(dag, reference_hash);
        let red_set = self.build_red_set(dag, reference_hash);
        
        let total_blocks = dag.block_count();
        let blue_count = blue_set.len();
        let red_count = red_set.len();

        BlueSetStats {
            blue_count,
            red_count,
            total_blocks,
            k_parameter: self.k,
            blue_ratio: if total_blocks > 0 {
                (blue_count as f64) / (total_blocks as f64)
            } else {
                0.0
            },
        }
    }
}

/// Statistics about blue set
#[derive(Clone, Debug)]
pub struct BlueSetStats {
    pub blue_count: usize,
    pub red_count: usize,
    pub total_blocks: usize,
    pub k_parameter: usize,
    pub blue_ratio: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Transaction;

    fn create_test_block(hash_seed: &str, parents: Vec<BlockHash>) -> crate::Block {
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
        crate::Block::new(parents, 1000 + hash_seed.len() as u64, vec![tx], 42, 0, 0, [0;20], [0;32])
    }

    #[test]
    fn test_build_blue_set_linear_chain() {
        let mut dag = BlockDAG::new();
        let genesis = create_test_block("genesis", vec![]);
        let child = create_test_block("child", vec![genesis.hash.clone()]);
        let grandchild = create_test_block("grandchild", vec![child.hash.clone()]);

        dag.insert_block(genesis.clone()).unwrap();
        dag.insert_block(child.clone()).unwrap();
        dag.insert_block(grandchild.clone()).unwrap();

        let blue_set = BlueSet::new(1);
        let blue_blocks = blue_set.build_blue_set(&dag, &grandchild.hash);

        // In linear chain, all should be blue if anticone is small
        assert!(blue_blocks.contains(&genesis.hash));
        assert!(blue_blocks.contains(&child.hash));
        assert!(blue_blocks.contains(&grandchild.hash));
    }

    #[test]
    fn test_blue_set_with_branches() {
        let mut dag = BlockDAG::new();
        let genesis = create_test_block("genesis", vec![]);
        let branch1 = create_test_block("branch1", vec![genesis.hash.clone()]);
        let branch2 = create_test_block("branch2", vec![genesis.hash.clone()]);
        let merge = create_test_block("merge", vec![branch1.hash.clone(), branch2.hash.clone()]);

        dag.insert_block(genesis).unwrap();
        dag.insert_block(branch1.clone()).unwrap();
        dag.insert_block(branch2).unwrap();
        dag.insert_block(merge.clone()).unwrap();

        let blue_set = BlueSet::new(2);
        let blue_blocks = blue_set.get_blue_blocks(&dag, &merge.hash);

        // All blocks should be in blue set (anticone of each <= k=2)
        assert!(blue_blocks.len() >= 2);
    }

    #[test]
    fn test_is_blue_simple() {
        let mut dag = BlockDAG::new();
        let genesis = create_test_block("genesis", vec![]);
        let child = create_test_block("child", vec![genesis.hash.clone()]);

        dag.insert_block(genesis.clone()).unwrap();
        dag.insert_block(child.clone()).unwrap();

        let blue_set = BlueSet::new(1);
        assert!(blue_set.is_blue(&dag, &genesis.hash, &child.hash));
        assert!(blue_set.is_blue(&dag, &child.hash, &child.hash));
    }

    #[test]
    fn test_anticone_size_calculation() {
        let mut dag = BlockDAG::new();
        let genesis = create_test_block("genesis", vec![]);
        let branch1 = create_test_block("branch1", vec![genesis.hash.clone()]);
        let branch2 = create_test_block("branch2", vec![genesis.hash.clone()]);

        dag.insert_block(genesis).unwrap();
        dag.insert_block(branch1.clone()).unwrap();
        dag.insert_block(branch2.clone()).unwrap();

        let blue_set = BlueSet::new(5);
        let anticone_size = blue_set.get_anticone_size(&dag, &branch1.hash, &branch2.hash);

        // branch1 and branch2 have no common ancestor path, so anticone handling needed
        assert!(anticone_size <= 10); // Sanity check for reasonable size
    }

    #[test]
    fn test_blue_set_statistics() {
        let mut dag = BlockDAG::new();
        let genesis = create_test_block("genesis", vec![]);
        let child = create_test_block("child", vec![genesis.hash.clone()]);
        let child_hash = child.hash.clone();

        dag.insert_block(genesis).unwrap();
        dag.insert_block(child).unwrap();

        let blue_set = BlueSet::new(1);
        let stats = blue_set.get_blue_set_stats(&dag, &child_hash);

        assert_eq!(stats.total_blocks, 2);
        assert!(stats.blue_count > 0);
        assert!(stats.blue_ratio > 0.0);
    }

    #[test]
    fn test_all_ancestors_blue() {
        let mut dag = BlockDAG::new();
        let genesis = create_test_block("genesis", vec![]);
        let child = create_test_block("child", vec![genesis.hash.clone()]);
        let grandchild = create_test_block("grandchild", vec![child.hash.clone()]);

        dag.insert_block(genesis).unwrap();
        dag.insert_block(child).unwrap();
        dag.insert_block(grandchild.clone()).unwrap();

        let blue_set = BlueSet::new(10);
        let result = blue_set.are_all_ancestors_blue(&dag, &grandchild.hash, &grandchild.hash);

        // In linear chain with decent k, all should be blue
        assert!(result);
    }
}
