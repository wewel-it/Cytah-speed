use std::collections::{HashMap, HashSet};
use crate::core::BlockHash;
use crate::dag::BlockDAG;
use super::dag_traversal::DAGTraversal;
use super::blue_set::BlueSet;

/// GHOSTDAG Consensus Engine
/// Generates deterministic ordering of blocks using GHOSTDAG algorithm
#[derive(Clone, Debug)]
pub struct GHOSTDAGEngine {
    /// Parameter k: maximum anticone size for blue blocks
    pub k: usize,
    /// Reference to BlockDAG
    dag: Option<BlockDAG>,
    /// Cache for blue scores to avoid recalculation
    blue_score_cache: HashMap<BlockHash, u64>,
    /// Blue set calculator
    blue_set: BlueSet,
}

impl GHOSTDAGEngine {
    /// Create new GHOSTDAG engine with parameter k
    pub fn new(k: usize) -> Self {
        GHOSTDAGEngine {
            k,
            dag: None,
            blue_score_cache: HashMap::new(),
            blue_set: BlueSet::new(k),
        }
    }

    /// Attach BlockDAG to engine
    pub fn attach_dag(&mut self, dag: BlockDAG) {
        self.dag = Some(dag);
        self.clear_cache();
    }

    /// Get attached DAG
    pub fn get_dag(&self) -> Option<&BlockDAG> {
        self.dag.as_ref()
    }

    /// Clear blue score cache
    pub fn clear_cache(&mut self) {
        self.blue_score_cache.clear();
    }

    /// Calculate blue score of a block (immutable version)
    fn calculate_blue_score_immutable(
        &self,
        dag: &BlockDAG,
        hash: &BlockHash,
    ) -> Result<u64, String> {
        // Check cache first
        if let Some(cached_score) = self.blue_score_cache.get(hash) {
            return Ok(*cached_score);
        }

        // Get all ancestors
        let ancestors = DAGTraversal::get_ancestors(dag, hash);

        // Count blue ancestors
        let mut blue_count = 0u64;

        // Check if block itself is blue (has anticone <= k)
        let block_anticone = DAGTraversal::get_anticone(dag, hash, hash);
        if block_anticone.len() <= self.k {
            blue_count += 1; // Count self as blue
        }

        // Count blue ancestors
        for ancestor in ancestors {
            let ancestor_anticone = DAGTraversal::get_anticone(dag, &ancestor, hash);
            if ancestor_anticone.len() <= self.k {
                blue_count += 1;
            }
        }

        Ok(blue_count)
    }

    /// Annotate a block with GhostDAG-derived metadata such as blue score and chain height.
    /// This is used to ensure blocks carry deterministic ordering metadata before being inserted into the DAG.
    pub fn annotate_block(
        &mut self,
        dag: &BlockDAG,
        block: &mut crate::Block,
    ) -> Result<(), String> {
        let selected_parent = if block.header.parent_hashes.is_empty() {
            None
        } else {
            Some(self.select_parent_from_parents(dag, &block.header.parent_hashes)?)
        };

        let chain_height = if let Some(parent) = selected_parent {
            dag.get_block_height(&parent).unwrap_or(0).saturating_add(1)
        } else {
            0
        };

        let blue_score = self.calculate_blue_score_for_new_block(dag, block)?;
        let topo_index = dag.block_count() as u64;

        block.set_consensus_metadata(selected_parent, blue_score, chain_height, topo_index);

        Ok(())
    }

    /// Select best parent from a list of candidate parents using blue score and deterministic tie-breakers.
    pub fn select_parent_from_parents(
        &mut self,
        dag: &BlockDAG,
        parents: &[BlockHash],
    ) -> Result<BlockHash, String> {
        if parents.is_empty() {
            return Err("No parent candidates provided".to_string());
        }

        let mut best_parent = parents[0];
        let mut best_score = self.get_blue_score(&best_parent).unwrap_or(0);

        for parent in parents.iter().skip(1) {
            let score = self.get_blue_score(parent).unwrap_or(0);
            if score > best_score || (score == best_score && parent < &best_parent) {
                best_parent = *parent;
                best_score = score;
            }
        }

        Ok(best_parent)
    }

    /// Calculate blue score for a block that is not yet inserted into the DAG.
    fn calculate_blue_score_for_new_block(
        &mut self,
        dag: &BlockDAG,
        block: &crate::Block,
    ) -> Result<u64, String> {
        // Basic heuristic: blue score = max parent blue score + 1
        let mut max_parent_score = 0u64;
        for parent_hash in &block.header.parent_hashes {
            let score = self.get_blue_score(parent_hash).unwrap_or(0);
            max_parent_score = max_parent_score.max(score);
        }
        Ok(max_parent_score.saturating_add(1))
    }

    /// Calculate blue score of a block
    /// Blue score = number of blue ancestors + 1
    pub fn calculate_blue_score(&mut self, hash: &BlockHash) -> Result<u64, String> {
        let dag = self.dag.as_ref().ok_or("DAG not attached")?;
        
        // Check cache first
        if let Some(cached_score) = self.blue_score_cache.get(hash) {
            return Ok(*cached_score);
        }

        // Get all ancestors
        let ancestors = DAGTraversal::get_ancestors(dag, hash);

        // Count blue ancestors
        let mut blue_count = 0u64;

        // Check if block itself is blue (has anticone <= k)
        let block_anticone = DAGTraversal::get_anticone(dag, hash, hash);
        if block_anticone.len() <= self.k {
            blue_count += 1; // Count self as blue
        }

        // Count blue ancestors
        for ancestor in ancestors {
            let ancestor_anticone = DAGTraversal::get_anticone(dag, &ancestor, hash);
            if ancestor_anticone.len() <= self.k {
                blue_count += 1;
            }
        }

        // Cache the result
        self.blue_score_cache.insert(hash.clone(), blue_count);

        Ok(blue_count)
    }

    /// Get blue score from cache or calculate
    pub fn get_blue_score(&mut self, hash: &BlockHash) -> Result<u64, String> {
        if let Some(cached) = self.blue_score_cache.get(hash) {
            return Ok(*cached);
        }
        self.calculate_blue_score(hash)
    }

    /// Select best parent based on blue score
    /// Returns parent with highest blue score, ties broken by hash ordering
    pub fn select_parent(&mut self, block_hash: &BlockHash) -> Result<BlockHash, String> {
        let dag = self.dag.as_ref().ok_or("DAG not attached")?;

        let parents = DAGTraversal::get_parents(dag, block_hash);

        if parents.is_empty() {
            return Err("Block has no parents".to_string());
        }

        if parents.len() == 1 {
            return Ok(parents[0].clone());
        }

        // Score each parent
        let mut parent_scores: Vec<(BlockHash, u64)> = Vec::new();

        for parent in parents {
            let score = self.calculate_blue_score(&parent)?;
            parent_scores.push((parent, score));
        }

        // Sort by score (descending), then by hash (ascending) for determinism
        parent_scores.sort_by(|a, b| {
            match b.1.cmp(&a.1) {
                std::cmp::Ordering::Equal => a.0.cmp(&b.0), // Deterministic tiebreaker
                other => other,
            }
        });

        Ok(parent_scores[0].0.clone())
    }

    /// Generate topological ordering using GHOSTDAG
    pub fn generate_ordering(&mut self) -> Result<Vec<BlockHash>, String> {
        let dag = self.dag.as_ref().ok_or("DAG not attached")?;

        // First, collect all blocks and calculate their scores
        let all_blocks = DAGTraversal::get_all_blocks(dag);
        let mut block_scores = HashMap::new();

        for block_hash in &all_blocks {
            let score = self.calculate_blue_score_immutable(dag, block_hash)?;
            block_scores.insert(block_hash.clone(), score);
        }

        // Now use the scores for ordering
        let mut ordering = Vec::new();

        if all_blocks.is_empty() {
            return Ok(ordering);
        }

        // Start with genesis blocks (no parents)
        let mut genesis_blocks: Vec<_> = all_blocks
            .iter()
            .filter(|h| {
                dag.get_block(h)
                    .map(|b| b.header.parent_hashes.is_empty())
                    .unwrap_or(false)
            })
            .cloned()
            .collect();

        genesis_blocks.sort(); // Deterministic order
        ordering.extend(genesis_blocks);

        // Process remaining blocks
        let mut processed = ordering.iter().cloned().collect::<HashSet<_>>();

        while processed.len() < all_blocks.len() {
            let mut next_blocks = Vec::new();

            for block_hash in &all_blocks {
                if processed.contains(block_hash) {
                    continue;
                }

                // Check if all parents are processed
                let parents = DAGTraversal::get_parents(dag, block_hash);
                if parents.iter().all(|p| processed.contains(p)) {
                    next_blocks.push(block_hash.clone());
                }
            }

            if next_blocks.is_empty() {
                break; // No more blocks can be processed (shouldn't happen in valid DAG)
            }

            // Score blocks and sort
            let mut scored_blocks: Vec<(BlockHash, u64)> = Vec::new();

            for block in next_blocks {
                let score = block_scores.get(&block).copied().unwrap_or(0);
                scored_blocks.push((block, score));
            }

            // Sort by blue score (descending), then hash (ascending)
            scored_blocks.sort_by(|a, b| match b.1.cmp(&a.1) {
                std::cmp::Ordering::Equal => a.0.cmp(&b.0),
                other => other,
            });

            for (block, _) in scored_blocks {
                ordering.push(block.clone());
                processed.insert(block);
            }
        }

        Ok(ordering)
    }

    /// Validate ordering
    pub fn validate_ordering(&self, ordering: &[BlockHash]) -> Result<(), String> {
        let dag = self.dag.as_ref().ok_or("DAG not attached")?;

        let mut seen = HashSet::new();

        for (index, block_hash) in ordering.iter().enumerate() {
            // Check block exists
            if !dag.get_block(block_hash).is_some() {
                return Err(format!("Block {} not found in DAG", hex::encode(block_hash)));
            }

            // Check no duplicates
            if seen.contains(block_hash) {
                return Err(format!("Duplicate block {} at position {}", hex::encode(block_hash), index));
            }
            seen.insert(block_hash.clone());

            // Check all parents appear before child
            let parents = DAGTraversal::get_parents(dag, block_hash);
            for parent in parents {
                let parent_pos = ordering.iter().position(|h| h == &parent);
                if parent_pos.is_none() {
                    return Err(format!(
                        "Parent {} of block {} not in ordering",
                        hex::encode(parent), hex::encode(block_hash)
                    ));
                }

                if parent_pos.unwrap() >= index {
                    return Err(format!(
                        "Parent {} appears after child {} in ordering",
                        hex::encode(parent), hex::encode(block_hash)
                    ));
                }
            }
        }

        // Check all blocks are included
        if seen.len() != dag.block_count() {
            return Err(format!(
                "Ordering has {} blocks but DAG has {} blocks",
                seen.len(),
                dag.block_count()
            ));
        }

        Ok(())
    }

    /// Get GHOSTDAG statistics
    pub fn get_stats(&self) -> GHOSTDAGStats {
        if let Some(dag) = self.dag.as_ref() {
            let blue_set = self.blue_set.build_blue_set(dag, &dag.get_tips()[0]);
            let red_set = self.blue_set.build_red_set(dag, &dag.get_tips()[0]);

            GHOSTDAGStats {
                k_parameter: self.k,
                total_blocks: dag.block_count(),
                blue_blocks: blue_set.len(),
                red_blocks: red_set.len(),
                cache_size: self.blue_score_cache.len(),
            }
        } else {
            GHOSTDAGStats {
                k_parameter: self.k,
                total_blocks: 0,
                blue_blocks: 0,
                red_blocks: 0,
                cache_size: 0,
            }
        }
    }

    /// Get blue score for all blocks
    pub fn get_all_blue_scores(&self) -> Result<HashMap<BlockHash, u64>, String> {
        let dag = self.dag.as_ref().ok_or("DAG not attached")?;

        let mut scores = HashMap::new();
        let all_blocks = DAGTraversal::get_all_blocks(dag);

        for block_hash in all_blocks {
            let score = self.calculate_blue_score_immutable(dag, &block_hash)?;
            scores.insert(block_hash, score);
        }

        Ok(scores)
    }

    /// Get blue blocks for reference
    pub fn get_blue_blocks(&self, reference_hash: &BlockHash) -> Result<Vec<BlockHash>, String> {
        let dag = self.dag.as_ref().ok_or("DAG not attached")?;
        Ok(self.blue_set.get_blue_blocks(dag, reference_hash))
    }

    /// Get red blocks for reference
    pub fn get_red_blocks(&self, reference_hash: &BlockHash) -> Result<Vec<BlockHash>, String> {
        let dag = self.dag.as_ref().ok_or("DAG not attached")?;
        Ok(self.blue_set.get_red_blocks(dag, reference_hash))
    }
}

/// GHOSTDAG statistics
#[derive(Clone, Debug)]
pub struct GHOSTDAGStats {
    pub k_parameter: usize,
    pub total_blocks: usize,
    pub blue_blocks: usize,
    pub red_blocks: usize,
    pub cache_size: usize,
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
    fn test_ghostdag_engine_creation() {
        let engine = GHOSTDAGEngine::new(3);
        assert_eq!(engine.k, 3);
        assert_eq!(engine.blue_score_cache.len(), 0);
    }

    #[test]
    fn test_attach_dag() {
        let mut engine = GHOSTDAGEngine::new(3);
        let dag = BlockDAG::new();
        engine.attach_dag(dag);
        assert!(engine.get_dag().is_some());
    }

    #[test]
    fn test_blue_score_calculation() {
        let mut engine = GHOSTDAGEngine::new(2);
        let mut dag = BlockDAG::new();

        let genesis = create_test_block("genesis", vec![]);
        let child = create_test_block("child", vec![genesis.hash.clone()]);

        dag.insert_block(genesis.clone()).unwrap();
        dag.insert_block(child.clone()).unwrap();

        engine.attach_dag(dag);

        let genesis_score = engine.calculate_blue_score(&genesis.hash).unwrap();
        let child_score = engine.calculate_blue_score(&child.hash).unwrap();

        assert!(genesis_score > 0);
        assert!(child_score > 0);
    }

    #[test]
    fn test_select_parent() {
        let mut engine = GHOSTDAGEngine::new(2);
        let mut dag = BlockDAG::new();

        // Create genesis
        dag.create_genesis_if_empty();
        let genesis_hash = dag.get_tips()[0].clone();

        // Create parents from genesis
        let parent1 = create_test_block("parent1", vec![genesis_hash.clone()]);
        let parent2 = create_test_block("parent2", vec![genesis_hash.clone()]);
        let child = create_test_block(
            "child",
            vec![parent1.hash.clone(), parent2.hash.clone()],
        );

        dag.insert_block(parent1.clone()).unwrap();
        dag.insert_block(parent2.clone()).unwrap();
        dag.insert_block(child.clone()).unwrap();

        engine.attach_dag(dag);

        let best_parent = engine.select_parent(&child.hash).unwrap();
        assert!(best_parent == parent1.hash || best_parent == parent2.hash);
    }

    #[test]
    fn test_generate_ordering() {
        let mut engine = GHOSTDAGEngine::new(2);
        let mut dag = BlockDAG::new();

        let genesis = create_test_block("genesis", vec![]);
        let child1 = create_test_block("child1", vec![genesis.hash.clone()]);
        let child2 = create_test_block("child2", vec![genesis.hash.clone()]);

        dag.insert_block(genesis.clone()).unwrap();
        dag.insert_block(child1.clone()).unwrap();
        dag.insert_block(child2.clone()).unwrap();

        engine.attach_dag(dag);

        let ordering = engine.generate_ordering().unwrap();
        assert_eq!(ordering.len(), 3);

        // Genesis should come first
        assert_eq!(ordering[0], genesis.hash);
    }

    #[test]
    fn test_validate_ordering() {
        let mut engine = GHOSTDAGEngine::new(2);
        let mut dag = BlockDAG::new();

        let genesis = create_test_block("genesis", vec![]);
        let child = create_test_block("child", vec![genesis.hash.clone()]);

        dag.insert_block(genesis.clone()).unwrap();
        dag.insert_block(child.clone()).unwrap();

        engine.attach_dag(dag);

        let ordering = vec![genesis.hash.clone(), child.hash.clone()];
        let result = engine.validate_ordering(&ordering);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_ordering_invalid() {
        let mut engine = GHOSTDAGEngine::new(2);
        let mut dag = BlockDAG::new();

        let genesis = create_test_block("genesis", vec![]);
        let child = create_test_block("child", vec![genesis.hash.clone()]);

        dag.insert_block(genesis.clone()).unwrap();
        dag.insert_block(child.clone()).unwrap();

        engine.attach_dag(dag);

        // Invalid: child before parent
        let invalid_ordering = vec![child.hash.clone(), genesis.hash.clone()];
        let result = engine.validate_ordering(&invalid_ordering);
        assert!(result.is_err());
    }

    #[test]
    fn test_get_stats() {
        let mut engine = GHOSTDAGEngine::new(2);
        let mut dag = BlockDAG::new();

        let genesis = create_test_block("genesis", vec![]);
        dag.insert_block(genesis).unwrap();

        engine.attach_dag(dag);

        let stats = engine.get_stats();
        assert_eq!(stats.k_parameter, 2);
        assert!(stats.total_blocks > 0);
    }

    #[test]
    fn test_cache_effectiveness() {
        let mut engine = GHOSTDAGEngine::new(2);
        let mut dag = BlockDAG::new();

        let genesis = create_test_block("genesis", vec![]);
        dag.insert_block(genesis.clone()).unwrap();

        engine.attach_dag(dag);

        // Calculate same block twice
        let _ = engine.calculate_blue_score(&genesis.hash);
        let _ = engine.get_blue_score(&genesis.hash);

        assert_eq!(engine.blue_score_cache.len(), 1);
    }

    #[test]
    fn test_all_blue_scores() {
        let mut engine = GHOSTDAGEngine::new(2);
        let mut dag = BlockDAG::new();

        let genesis = create_test_block("genesis", vec![]);
        let child = create_test_block("child", vec![genesis.hash.clone()]);

        dag.insert_block(genesis).unwrap();
        dag.insert_block(child).unwrap();

        engine.attach_dag(dag);

        let scores = engine.get_all_blue_scores().unwrap();
        assert_eq!(scores.len(), 2);
    }
}
