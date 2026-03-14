use crate::core::{Block, BlockHash};
use std::collections::{BTreeMap, HashSet, HashMap, BTreeSet};
use parking_lot::RwLock;
use std::sync::Arc;
use std::cell::RefCell;

/// Interval tree node for reachability queries
#[derive(Debug, Clone)]
struct IntervalNode {
    /// Block hash this node represents
    block_hash: BlockHash,
    /// Height of this block
    height: u64,
    /// Score (GHOSTDAG ordering position)
    score: u64,
    /// Left child (lower heights)
    left: Option<Box<IntervalNode>>,
    /// Right child (higher heights)
    right: Option<Box<IntervalNode>>,
    /// Cached reachability information
    reachability_cache: RefCell<HashMap<BlockHash, bool>>,
}

/// Anticone cache entry
#[derive(Debug, Clone)]
struct AnticoneEntry {
    /// The anticone set
    anticone: HashSet<BlockHash>,
    /// Timestamp when this was computed
    computed_at: u64,
    /// Size of anticone (for metrics)
    size: usize,
}

/// Virtual Selected Parent (VSP) information
#[derive(Debug, Clone)]
struct VirtualSelectedParent {
    /// The VSP block hash
    block_hash: BlockHash,
    /// Height of VSP
    height: u64,
    /// Score of VSP
    score: u64,
    /// Blue score (for finality calculation)
    blue_score: u64,
}

/// Advanced GHOSTDAG finality engine with reachability optimization
#[derive(Clone)]
pub struct FinalityEngine {
    /// Confirmation depth for basic finality
    pub confirmation_depth: usize,
    /// K parameter for k-cluster grinding protection
    pub k: usize,
    /// Interval tree root for reachability queries
    interval_tree: Arc<RwLock<Option<Box<IntervalNode>>>>,
    /// Anticone calculation cache
    anticone_cache: Arc<RwLock<HashMap<BlockHash, AnticoneEntry>>>,
    /// Virtual selected parents cache
    vsp_cache: Arc<RwLock<HashMap<BlockHash, VirtualSelectedParent>>>,
    /// Finalized blocks by height
    finalized_blocks: Arc<RwLock<BTreeMap<u64, HashSet<BlockHash>>>>,
    /// Block heights cache
    block_heights: Arc<RwLock<HashMap<BlockHash, u64>>>,
    /// Block scores cache
    block_scores: Arc<RwLock<HashMap<BlockHash, u64>>>,
    /// Blue scores for probabilistic finality
    blue_scores: Arc<RwLock<HashMap<BlockHash, u64>>>,
    /// Cache size limits
    max_cache_size: usize,
}

impl FinalityEngine {
    /// Create new advanced finality engine
    ///
    /// By default, uses a conservative k-cluster protection parameter.
    pub fn new(confirmation_depth: usize) -> Self {
        Self {
            confirmation_depth,
            k: 8,
            interval_tree: Arc::new(RwLock::new(None)),
            anticone_cache: Arc::new(RwLock::new(HashMap::new())),
            vsp_cache: Arc::new(RwLock::new(HashMap::new())),
            finalized_blocks: Arc::new(RwLock::new(BTreeMap::new())),
            block_heights: Arc::new(RwLock::new(HashMap::new())),
            block_scores: Arc::new(RwLock::new(HashMap::new())),
            blue_scores: Arc::new(RwLock::new(HashMap::new())),
            max_cache_size: 10000,
        }
    }

    /// Create new finality engine with explicit k parameter
    pub fn new_with_k(confirmation_depth: usize, k: usize) -> Self {
        Self {
            confirmation_depth,
            k,
            interval_tree: Arc::new(RwLock::new(None)),
            anticone_cache: Arc::new(RwLock::new(HashMap::new())),
            vsp_cache: Arc::new(RwLock::new(HashMap::new())),
            finalized_blocks: Arc::new(RwLock::new(BTreeMap::new())),
            block_heights: Arc::new(RwLock::new(HashMap::new())),
            block_scores: Arc::new(RwLock::new(HashMap::new())),
            blue_scores: Arc::new(RwLock::new(HashMap::new())),
            max_cache_size: 10000,
        }
    }

    /// Compute finality using advanced GHOSTDAG algorithm
    /// Includes reachability index, anticone caching, and k-cluster protection
    pub fn compute_finality(&self, dag: &crate::dag::blockdag::BlockDAG) -> Result<(), String> {
        let ordering = dag.get_ordering();
        if ordering.is_empty() {
            return Err("Empty DAG ordering".to_string());
        }

        // Build interval tree for reachability queries
        self.build_interval_tree(&ordering, dag)?;

        // Compute virtual selected parents
        self.compute_virtual_selected_parents(dag)?;

        // Calculate finality using score-based approach
        self.calculate_score_based_finality(dag)?;

        Ok(())
    }

    /// Build interval tree for efficient reachability queries
    fn build_interval_tree(&self, ordering: &[BlockHash], dag: &crate::dag::blockdag::BlockDAG) -> Result<(), String> {
        if ordering.is_empty() {
            return Ok(());
        }

        // Create interval nodes from ordering
        let mut nodes: Vec<IntervalNode> = Vec::new();
        let mut heights = HashMap::new();
        let mut scores = HashMap::new();

        for (index, block_hash) in ordering.iter().enumerate() {
            let height = dag.get_block_height(block_hash)
                .ok_or_else(|| format!("Block height not found for {:?}", block_hash))?;
            let score = index as u64;

            heights.insert(*block_hash, height);
            scores.insert(*block_hash, score);

            nodes.push(IntervalNode {
                block_hash: *block_hash,
                height,
                score,
                left: None,
                right: None,
                reachability_cache: RefCell::new(HashMap::new()),
            });
        }

        // Build balanced interval tree
        let tree = self.build_balanced_tree(nodes);

        // Update caches
        *self.interval_tree.write() = Some(tree);
        *self.block_heights.write() = heights;
        *self.block_scores.write() = scores;

        Ok(())
    }

    /// Build balanced interval tree from nodes
    fn build_balanced_tree(&self, mut nodes: Vec<IntervalNode>) -> Box<IntervalNode> {
        nodes.sort_by_key(|n| n.height);

        if nodes.len() == 1 {
            return Box::new(nodes.into_iter().next().unwrap());
        }

        let mid = nodes.len() / 2;
        let node = nodes.swap_remove(mid);

        let (left_nodes, right_nodes) = nodes.into_iter()
            .partition::<Vec<_>, _>(|n| n.height < node.height);

        let mut tree_node = Box::new(node);
        if !left_nodes.is_empty() {
            tree_node.left = Some(self.build_balanced_tree(left_nodes));
        }
        if !right_nodes.is_empty() {
            tree_node.right = Some(self.build_balanced_tree(right_nodes));
        }

        tree_node
    }

    /// Check if block A can reach block B using interval tree
    pub fn can_reach(&self, from: &BlockHash, to: &BlockHash) -> Result<bool, String> {
        let tree = self.interval_tree.read();
        if tree.is_none() {
            return Err("Interval tree not built".to_string());
        }

        // Check cache first
        if let Some(node) = self.find_node(&tree.as_ref().unwrap(), from) {
            if let Some(&cached) = node.reachability_cache.borrow().get(to) {
                return Ok(cached);
            }
        }

        // Compute reachability
        let can_reach = self.compute_reachability(from, to)?;

        // Cache result
        if let Some(node) = self.find_node(&tree.as_ref().unwrap(), from) {
            node.reachability_cache.borrow_mut().insert(*to, can_reach);
            // Limit cache size
            if node.reachability_cache.borrow().len() > self.max_cache_size / 100 {
                node.reachability_cache.borrow_mut().clear();
            }
        }

        Ok(can_reach)
    }

    /// Compute reachability between two blocks
    fn compute_reachability(&self, from: &BlockHash, to: &BlockHash) -> Result<bool, String> {
        // Get heights and scores
        let heights = self.block_heights.read();
        let scores = self.block_scores.read();

        let from_height = heights.get(from).ok_or("From block height not found")?;
        let to_height = heights.get(to).ok_or("To block height not found")?;
        let from_score = scores.get(from).ok_or("From block score not found")?;
        let to_score = scores.get(to).ok_or("To block score not found")?;

        // Basic reachability: from must come before to in both height and score
        Ok(from_height <= to_height && from_score <= to_score)
    }

    /// Find node in interval tree
    fn find_node<'a>(&self, tree: &'a Box<IntervalNode>, target: &BlockHash) -> Option<&'a IntervalNode> {
        if tree.block_hash == *target {
            return Some(tree);
        }

        if let Some(ref left) = tree.left {
            if let Some(node) = self.find_node(left, target) {
                return Some(node);
            }
        }

        if let Some(ref right) = tree.right {
            if let Some(node) = self.find_node(right, target) {
                return Some(node);
            }
        }

        None
    }

    /// Compute virtual selected parents for the DAG
    fn compute_virtual_selected_parents(&self, dag: &crate::dag::blockdag::BlockDAG) -> Result<(), String> {
        let mut vsp_cache = HashMap::new();

        // For each block, compute its virtual selected parent
        for block_hash in dag.get_all_block_hashes() {
            let vsp = self.compute_single_vsp(&block_hash, dag)?;
            vsp_cache.insert(block_hash, vsp);
        }

        *self.vsp_cache.write() = vsp_cache;
        Ok(())
    }

    /// Compute virtual selected parent for a single block
    fn compute_single_vsp(&self, block_hash: &BlockHash, dag: &crate::dag::blockdag::BlockDAG) -> Result<VirtualSelectedParent, String> {
        // Get block's parents
        let parents = dag.get_block_parents(block_hash)
            .ok_or_else(|| format!("Parents not found for block {:?}", block_hash))?;

        if parents.is_empty() {
            // Genesis block
            return Ok(VirtualSelectedParent {
                block_hash: *block_hash,
                height: 0,
                score: 0,
                blue_score: 0,
            });
        }

        // Find parent with highest blue score
        let mut best_parent = None;
        let mut best_blue_score = 0;

        for parent in &parents {
            let blue_score = self.get_blue_score(parent, dag)?;
            if blue_score > best_blue_score {
                best_blue_score = blue_score;
                best_parent = Some(*parent);
            }
        }

        let selected_parent = best_parent.unwrap_or(parents[0]);
        let height = dag.get_block_height(&selected_parent)
            .ok_or("Selected parent height not found")?;
        let score = self.block_scores.read().get(&selected_parent).copied().unwrap_or(0);

        Ok(VirtualSelectedParent {
            block_hash: selected_parent,
            height,
            score,
            blue_score: best_blue_score,
        })
    }

    /// Get blue score for a block (used in finality calculation)
    fn get_blue_score(&self, block_hash: &BlockHash, dag: &crate::dag::blockdag::BlockDAG) -> Result<u64, String> {
        // Check cache first
        if let Some(&score) = self.blue_scores.read().get(block_hash) {
            return Ok(score);
        }

        // Compute blue score based on GHOSTDAG blue set
        let blue_set = dag.get_blue_set(block_hash)
            .ok_or_else(|| format!("Blue set not found for block {:?}", block_hash))?;

        let score = blue_set.len() as u64;

        // Cache result
        self.blue_scores.write().insert(*block_hash, score);

        Ok(score)
    }

    /// Calculate score-based probabilistic finality
    fn calculate_score_based_finality(&self, dag: &crate::dag::blockdag::BlockDAG) -> Result<(), String> {
        let mut finalized = BTreeMap::new();
        let scores = self.block_scores.read();

        // Get current head score
        let head_score = scores.values().max().copied().unwrap_or(0);

        // Calculate finality threshold based on k-cluster protection
        let finality_threshold = self.calculate_finality_threshold(head_score, dag)?;

        // Mark blocks as finalized based on score threshold
        for (block_hash, score) in scores.iter() {
            if *score <= finality_threshold {
                let height = self.block_heights.read().get(block_hash).copied().unwrap_or(0);
                finalized.entry(height)
                    .or_insert_with(HashSet::new)
                    .insert(*block_hash);
            }
        }

        *self.finalized_blocks.write() = finalized;
        Ok(())
    }

    /// Calculate finality threshold with k-cluster grinding protection
    fn calculate_finality_threshold(&self, head_score: u64, dag: &crate::dag::blockdag::BlockDAG) -> Result<u64, String> {
        // Use k-cluster protection: require score difference > k
        let threshold = head_score.saturating_sub(self.k as u64);

        // Additional probabilistic safety margin
        let safety_margin = (head_score as f64 * 0.1) as u64; // 10% safety margin
        let final_threshold = threshold.saturating_sub(safety_margin);

        Ok(final_threshold)
    }

    /// Compute anticone of a block with caching
    pub fn get_anticone(&self, block_hash: &BlockHash, dag: &crate::dag::blockdag::BlockDAG) -> Result<HashSet<BlockHash>, String> {
        // Check cache first
        let cache_key = *block_hash;
        {
            let cache = self.anticone_cache.read();
            if let Some(entry) = cache.get(&cache_key) {
                return Ok(entry.anticone.clone());
            }
        }

        // Compute anticone
        let anticone = self.compute_anticone(block_hash, dag)?;

        // Cache result
        let entry = AnticoneEntry {
            anticone: anticone.clone(),
            computed_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            size: anticone.len(),
        };

        let mut cache = self.anticone_cache.write();
        cache.insert(cache_key, entry);

        // Limit cache size
        if cache.len() > self.max_cache_size {
            // Remove oldest entries (simple LRU approximation)
            let mut keys: Vec<BlockHash> = cache.keys().cloned().collect();
            keys.sort_by_key(|k| cache.get(k).map(|entry| entry.computed_at).unwrap_or(0));
            let remove_count = cache.len() / 10;
            for key in keys.into_iter().take(remove_count) {
                cache.remove(&key);
            }
        }

        Ok(anticone)
    }

    /// Compute anticone of a block
    fn compute_anticone(&self, block_hash: &BlockHash, dag: &crate::dag::blockdag::BlockDAG) -> Result<HashSet<BlockHash>, String> {
        let mut anticone = HashSet::new();
        let block_height = self.block_heights.read().get(block_hash)
            .copied().ok_or("Block height not found")?;

        // Get all blocks at same or higher heights that don't descend from this block
        for other_hash in dag.get_all_block_hashes() {
            if other_hash == *block_hash {
                continue;
            }

            let other_height = self.block_heights.read().get(&other_hash)
                .copied().unwrap_or(0);

            if other_height >= block_height {
                // Check if other block is in the future cone of this block
                if !self.can_reach(block_hash, &other_hash)? {
                    anticone.insert(other_hash);
                }
            }
        }

        Ok(anticone)
    }

    /// Check if block is finalized using advanced criteria
    pub fn is_finalized(&self, block_hash: &BlockHash) -> bool {
        let finalized = self.finalized_blocks.read();
        finalized.values().any(|set| set.contains(block_hash))
    }

    /// Get finality confidence score (0.0 to 1.0)
    pub fn get_finality_confidence(&self, block_hash: &BlockHash) -> f64 {
        let scores = self.block_scores.read();
        let heights = self.block_heights.read();

        let block_score = match scores.get(block_hash) {
            Some(&score) => score,
            None => return 0.0,
        };

        let head_score = scores.values().max().copied().unwrap_or(0);
        if head_score == 0 {
            return 0.0;
        }

        // Confidence based on score distance from head
        let distance = head_score.saturating_sub(block_score);
        let confidence = 1.0 - (distance as f64 / head_score as f64);

        confidence.max(0.0).min(1.0)
    }

    /// Get virtual selected parent for a block
    pub fn get_virtual_selected_parent(&self, block_hash: &BlockHash) -> Option<VirtualSelectedParent> {
        self.vsp_cache.read().get(block_hash).cloned()
    }

    /// Get finalized blocks at specific height
    pub fn get_finalized_at_height(&self, height: u64) -> Option<HashSet<BlockHash>> {
        self.finalized_blocks.read().get(&height).cloned()
    }

    /// Get current finalization height
    pub fn get_finalization_height(&self) -> Option<u64> {
        self.finalized_blocks.read().keys().last().copied()
    }

    /// Get block height
    pub fn get_block_height(&self, block_hash: &BlockHash) -> Option<u64> {
        self.block_heights.read().get(block_hash).copied()
    }

    /// Get block score
    pub fn get_block_score(&self, block_hash: &BlockHash) -> Option<u64> {
        self.block_scores.read().get(block_hash).copied()
    }

    /// Reset all caches and state
    pub fn reset(&self) {
        *self.interval_tree.write() = None;
        self.anticone_cache.write().clear();
        self.vsp_cache.write().clear();
        self.finalized_blocks.write().clear();
        self.block_heights.write().clear();
        self.block_scores.write().clear();
        self.blue_scores.write().clear();
    }

    /// Get finality engine statistics
    pub fn get_stats(&self) -> FinalityStats {
        let anticone_cache = self.anticone_cache.read();
        let vsp_cache = self.vsp_cache.read();
        let finalized = self.finalized_blocks.read();

        let total_finalized = finalized.values()
            .map(|set| set.len())
            .sum();

        FinalityStats {
            interval_tree_built: self.interval_tree.read().is_some(),
            anticone_cache_size: anticone_cache.len(),
            vsp_cache_size: vsp_cache.len(),
            total_finalized_blocks: total_finalized,
            confirmation_depth: self.confirmation_depth,
            k_cluster_protection: self.k,
        }
    }
}

/// Finality engine statistics
#[derive(Debug, Clone)]
pub struct FinalityStats {
    pub interval_tree_built: bool,
    pub anticone_cache_size: usize,
    pub vsp_cache_size: usize,
    pub total_finalized_blocks: usize,
    pub confirmation_depth: usize,
    pub k_cluster_protection: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::blockdag::BlockDAG;

    fn create_test_blocks() -> Vec<Block> {
        (1u8..=10u8)
            .map(|i| {
                let parents = if i == 1 { vec![] } else { vec![[i-1; 32]] };
                Block::new(parents, 1000 + i as u64, vec![], 0, 0, 0, [0;20], [0;32])
            })
            .collect()
    }

    fn create_test_dag() -> BlockDAG {
        let mut dag = BlockDAG::new();
        let blocks = create_test_blocks();

        // Add genesis
        dag.insert_block(blocks[0].clone()).unwrap();

        // Add some blocks with parents
        for i in 1..blocks.len() {
            dag.insert_block(blocks[i].clone()).unwrap();
        }

        dag
    }

    #[test]
    fn test_finality_engine_creation() {
        let engine = FinalityEngine::new_with_k(3, 5);
        assert_eq!(engine.confirmation_depth, 3);
        assert_eq!(engine.k, 5);
    }

    #[test]
    fn test_compute_finality() {
        let engine = FinalityEngine::new_with_k(2, 5);
        let dag = create_test_dag();

        let result = engine.compute_finality(&dag);
        assert!(result.is_ok());
    }

    #[test]
    fn test_is_finalized() {
        let engine = FinalityEngine::new_with_k(2, 5);
        let dag = create_test_dag();

        engine.compute_finality(&dag).unwrap();

        // Test some blocks are finalized
        let blocks = create_test_blocks();
        let finalized_count = blocks.iter()
            .filter(|block| engine.is_finalized(&block.hash))
            .count();

        assert!(finalized_count > 0);
    }

    #[test]
    fn test_get_finality_confidence() {
        let engine = FinalityEngine::new_with_k(2, 5);
        let dag = create_test_dag();

        engine.compute_finality(&dag).unwrap();

        let blocks = create_test_blocks();
        let confidence = engine.get_finality_confidence(&blocks[0].hash);

        assert!(confidence >= 0.0 && confidence <= 1.0);
    }

    #[test]
    fn test_get_finalization_height() {
        let engine = FinalityEngine::new_with_k(3, 5);
        let dag = create_test_dag();

        assert_eq!(engine.get_finalization_height(), None);

        engine.compute_finality(&dag).unwrap();
        let height = engine.get_finalization_height();
        assert!(height.is_some());
    }

    #[test]
    fn test_reset_finality() {
        let engine = FinalityEngine::new_with_k(2, 5);
        let dag = create_test_dag();

        engine.compute_finality(&dag).unwrap();
        assert!(engine.get_finalization_height().is_some());

        engine.reset();
        assert_eq!(engine.get_finalization_height(), None);
    }

    #[test]
    fn test_get_block_height() {
        let engine = FinalityEngine::new_with_k(2, 5);
        let dag = create_test_dag();

        engine.compute_finality(&dag).unwrap();

        let blocks = create_test_blocks();
        let height = engine.get_block_height(&blocks[0].hash);

        assert_eq!(height, Some(0));
    }

    #[test]
    fn test_get_stats() {
        let engine = FinalityEngine::new_with_k(2, 5);
        let stats = engine.get_stats();

        assert_eq!(stats.confirmation_depth, 2);
        assert_eq!(stats.k_cluster_protection, 5);
        assert!(!stats.interval_tree_built);
    }
}
