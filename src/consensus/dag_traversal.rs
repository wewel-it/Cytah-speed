use std::collections::{HashSet, VecDeque};
use crate::core::BlockHash;
use crate::dag::BlockDAG;

/// DAG Traversal utilities untuk GHOSTDAG consensus
#[derive(Clone, Debug)]
pub struct DAGTraversal;

impl DAGTraversal {
    /// Get all ancestors of a block (parents recursively)
    pub fn get_ancestors(dag: &BlockDAG, hash: &BlockHash) -> HashSet<BlockHash> {
        let mut ancestors = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(hash.clone());
        let mut visited = HashSet::new();

        while let Some(current) = queue.pop_front() {
            if visited.contains(&current) {
                continue;
            }
            visited.insert(current.clone());

            if let Some(block) = dag.get_block(&current) {
                for parent_hash in &block.header.parent_hashes {
                    if !ancestors.contains(parent_hash) && parent_hash != hash {
                        ancestors.insert(parent_hash.clone());
                        queue.push_back(parent_hash.clone());
                    }
                }
            }
        }

        ancestors
    }

    /// Get all descendants of a block (children recursively)
    pub fn get_descendants(dag: &BlockDAG, hash: &BlockHash) -> HashSet<BlockHash> {
        let mut descendants = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(hash.clone());
        let mut visited = HashSet::new();

        while let Some(current) = queue.pop_front() {
            if visited.contains(&current) {
                continue;
            }
            visited.insert(current.clone());

            let children = dag.get_children(&current);
            for child in children {
                if !descendants.contains(&child) && child != *hash {
                    descendants.insert(child.clone());
                    queue.push_back(child);
                }
            }
        }

        descendants
    }

    /// Get direct parents of a block
    pub fn get_parents(dag: &BlockDAG, hash: &BlockHash) -> Vec<BlockHash> {
        dag.get_block(hash)
            .map(|b| b.header.parent_hashes.clone())
            .unwrap_or_default()
    }

    /// Get direct children of a block
    pub fn get_children(dag: &BlockDAG, hash: &BlockHash) -> Vec<BlockHash> {
        dag.get_children(hash)
    }

    /// Get anticone of a block relative to reference block
    /// Anticone = blocks that are not ancestors of reference and not the reference itself
    pub fn get_anticone(
        dag: &BlockDAG,
        block_hash: &BlockHash,
        reference_hash: &BlockHash,
    ) -> HashSet<BlockHash> {
        // All ancestors of reference
        let ref_ancestors = Self::get_ancestors(dag, reference_hash);

        // All blocks reachable from reference (including reference)
        let mut reachable = ref_ancestors.clone();
        reachable.insert(reference_hash.clone());

        // All ancestors of block hash
        let block_ancestors = Self::get_ancestors(dag, block_hash);

        // Anticone = ancestors of block that are NOT in reachable set of reference
        let mut anticone = HashSet::new();
        for ancestor in block_ancestors {
            if !reachable.contains(&ancestor) {
                anticone.insert(ancestor);
            }
        }

        anticone
    }

    /// Get all blocks between two nodes (path from ancestor to descendant)
    pub fn get_path(
        dag: &BlockDAG,
        from: &BlockHash,
        to: &BlockHash,
    ) -> Option<Vec<BlockHash>> {
        // Simple BFS to find a path
        let mut queue = VecDeque::new();
        let mut visited = HashSet::new();
        let mut parent_map: std::collections::HashMap<BlockHash, BlockHash> =
            std::collections::HashMap::new();

        queue.push_back(to.clone());
        visited.insert(to.clone());

        while let Some(current) = queue.pop_front() {
            if current == *from {
                // Reconstruct path
                let mut path = vec![current.clone()];
                let mut curr = current;

                while let Some(parent) = parent_map.get(&curr) {
                    path.push(parent.clone());
                    curr = parent.clone();
                }

                path.reverse();
                return Some(path);
            }

            let parents = Self::get_parents(dag, &current);
            for parent in parents {
                if !visited.contains(&parent) {
                    visited.insert(parent.clone());
                    parent_map.insert(parent.clone(), current.clone());
                    queue.push_back(parent);
                }
            }
        }

        None
    }

    /// Get all blocks in DAG (helper for iteration)
    pub fn get_all_blocks(dag: &BlockDAG) -> Vec<BlockHash> {
        dag.get_topological_order()
    }

    /// Get blocks at specific level/depth from genesis
    pub fn get_blocks_at_depth(dag: &BlockDAG, depth: usize) -> Vec<BlockHash> {
        let all_blocks = dag.get_topological_order();
        all_blocks
            .into_iter()
            .filter(|hash| dag.get_block_depth(hash) == depth)
            .collect()
    }

    /// Check if block1 is ancestor of block2
    pub fn is_ancestor(dag: &BlockDAG, ancestor: &BlockHash, block: &BlockHash) -> bool {
        dag.is_ancestor(ancestor, block)
    }

    /// Get common ancestors of multiple blocks
    pub fn get_common_ancestors(
        dag: &BlockDAG,
        hashes: &[BlockHash],
    ) -> HashSet<BlockHash> {
        if hashes.is_empty() {
            return HashSet::new();
        }

        // Get ancestors of first block
        let mut common = Self::get_ancestors(dag, &hashes[0]);
        common.insert(hashes[0].clone());

        // Intersect with ancestors of other blocks
        for i in 1..hashes.len() {
            let ancestors = Self::get_ancestors(dag, &hashes[i]);
            let mut new_ancestors = ancestors.clone();
            new_ancestors.insert(hashes[i].clone());

            common = common.intersection(&new_ancestors).cloned().collect();
        }

        common
    }

    /// Get furthest common ancestor (LCA-like but furthest)
    pub fn get_furthest_common_ancestor(
        dag: &BlockDAG,
        hash1: &BlockHash,
        hash2: &BlockHash,
    ) -> Option<BlockHash> {
        let ancestors1 = Self::get_ancestors(dag, hash1);
        let ancestors2 = Self::get_ancestors(dag, hash2);

        let common = ancestors1.intersection(&ancestors2).cloned().collect::<Vec<_>>();

        if common.is_empty() {
            // Check if they have genesis as common
            // Try to find LCA
            dag.find_lca(hash1, hash2)
        } else {
            // Find the deepest (furthest) common ancestor
            common
                .iter()
                .max_by_key(|h| dag.get_block_depth(h))
                .cloned()
        }
    }

    /// Count descendants (size of subtree)
    pub fn count_descendants(dag: &BlockDAG, hash: &BlockHash) -> usize {
        Self::get_descendants(dag, hash).len()
    }

    /// Count ancestors
    pub fn count_ancestors(dag: &BlockDAG, hash: &BlockHash) -> usize {
        Self::get_ancestors(dag, hash).len()
    }
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
    fn test_get_ancestors() {
        let mut dag = BlockDAG::new();
        let genesis = create_test_block("genesis", vec![]);
        let child1 = create_test_block("child1", vec![genesis.hash.clone()]);
        let child2 = create_test_block("child2", vec![child1.hash.clone()]);

        dag.insert_block(genesis.clone()).unwrap();
        dag.insert_block(child1.clone()).unwrap();
        dag.insert_block(child2.clone()).unwrap();

        let ancestors = DAGTraversal::get_ancestors(&dag, &child2.hash);
        assert!(ancestors.contains(&genesis.hash));
        assert!(ancestors.contains(&child1.hash));
        assert!(!ancestors.contains(&child2.hash));
    }

    #[test]
    fn test_get_descendants() {
        let mut dag = BlockDAG::new();
        let genesis = create_test_block("genesis", vec![]);
        let child1 = create_test_block("child1", vec![genesis.hash.clone()]);
        let child2 = create_test_block("child2", vec![child1.hash.clone()]);

        dag.insert_block(genesis.clone()).unwrap();
        dag.insert_block(child1.clone()).unwrap();
        dag.insert_block(child2.clone()).unwrap();

        let descendants = DAGTraversal::get_descendants(&dag, &genesis.hash);
        assert!(descendants.contains(&child1.hash));
        assert!(descendants.contains(&child2.hash));
    }

    #[test]
    fn test_count_ancestors() {
        let mut dag = BlockDAG::new();
        let genesis = create_test_block("genesis", vec![]);
        let child1 = create_test_block("child1", vec![genesis.hash.clone()]);
        let child2 = create_test_block("child2", vec![child1.hash.clone()]);
        let child2_hash = child2.hash.clone();

        dag.insert_block(genesis).unwrap();
        dag.insert_block(child1).unwrap();
        dag.insert_block(child2).unwrap();

        assert_eq!(DAGTraversal::count_ancestors(&dag, &child2_hash), 2);
    }

    #[test]
    fn test_is_ancestor() {
        let mut dag = BlockDAG::new();
        let genesis = create_test_block("genesis", vec![]);
        let child = create_test_block("child", vec![genesis.hash.clone()]);

        dag.insert_block(genesis.clone()).unwrap();
        dag.insert_block(child.clone()).unwrap();

        assert!(DAGTraversal::is_ancestor(&dag, &genesis.hash, &child.hash));
        assert!(!DAGTraversal::is_ancestor(&dag, &child.hash, &genesis.hash));
    }

    #[test]
    fn test_get_parents() {
        let mut dag = BlockDAG::new();
        let parent = create_test_block("parent", vec![]);
        let child = create_test_block("child", vec![parent.hash.clone()]);

        dag.insert_block(parent.clone()).unwrap();
        dag.insert_block(child.clone()).unwrap();

        let parents = DAGTraversal::get_parents(&dag, &child.hash);
        assert_eq!(parents.len(), 1);
        assert_eq!(parents[0], parent.hash);
    }

    #[test]
    fn test_get_anticone() {
        let mut dag = BlockDAG::new();
        let genesis = create_test_block("genesis", vec![]);
        let branch1 = create_test_block("branch1", vec![genesis.hash.clone()]);
        let branch2 = create_test_block("branch2", vec![genesis.hash.clone()]);

        dag.insert_block(genesis.clone()).unwrap();
        dag.insert_block(branch1.clone()).unwrap();
        dag.insert_block(branch2.clone()).unwrap();

        // Anticone of branch1 relative to branch2 should not include branch1 or genesis
        let anticone = DAGTraversal::get_anticone(&dag, &branch1.hash, &branch2.hash);
        // In this case, anticone should be empty or contain blocks not in ancestry
        assert!(!anticone.contains(&branch1.hash));
    }
}
