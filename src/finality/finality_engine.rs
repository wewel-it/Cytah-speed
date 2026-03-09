use crate::core::BlockHash;
use std::collections::{BTreeMap, HashSet};
use parking_lot::RwLock;
use std::sync::Arc;

/// Engine untuk menghitung finality blok berdasarkan GHOSTDAG ordering
#[derive(Clone)]
pub struct FinalityEngine {
    /// Kedalaman konfirmasi untuk menentukan finality
    pub confirmation_depth: usize,
    /// Blok yang telah finalized (disimpan per height)
    finalized_blocks: Arc<RwLock<BTreeMap<u64, HashSet<BlockHash>>>>,
    /// Cache height blok untuk quick lookup
    block_heights: Arc<RwLock<std::collections::HashMap<BlockHash, u64>>>,
}

impl FinalityEngine {
    /// Buat finality engine baru
    pub fn new(confirmation_depth: usize) -> Self {
        Self {
            confirmation_depth,
            finalized_blocks: Arc::new(RwLock::new(BTreeMap::new())),
            block_heights: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Hitung finality dari GHOSTDAG ordering
    /// 
    /// Logika:
    /// 1. Ambil ordering dari GHOSTDAG
    /// 2. Hitung posisi setiap blok dalam ordering
    /// 3. Blok adalah finalized jika jarak dari HEAD > confirmation_depth
    pub fn compute_finality(&self, ordering: &[BlockHash]) -> Result<(), String> {
        if ordering.is_empty() {
            return Err("Empty ordering".to_string());
        }

        let mut finalized = BTreeMap::new();
        let mut heights = std::collections::HashMap::new();

        // Assign heights dari ordering (kepala = 0)
        for (index, block_hash) in ordering.iter().enumerate() {
            let height = ordering.len().saturating_sub(index + 1);
            heights.insert(block_hash.clone(), height as u64);
        }

        // Tentukan finalization point
        let current_head_height = (ordering.len() as u64).saturating_sub(1);
        let finalization_height = current_head_height.saturating_sub(self.confirmation_depth as u64);

        // Blok dengan height <= finalization_height adalah finalized
        for (i, block_hash) in ordering.iter().enumerate() {
            let height = ordering.len().saturating_sub(i + 1) as u64;
            if height <= finalization_height {
                finalized
                    .entry(height)
                    .or_insert_with(HashSet::new)
                    .insert(block_hash.clone());
            }
        }

        // Update internal state
        *self.block_heights.write() = heights;
        *self.finalized_blocks.write() = finalized;

        Ok(())
    }

    /// Cek apakah blok sudah finalized
    pub fn is_finalized(&self, block_hash: &BlockHash) -> bool {
        let finalized = self.finalized_blocks.read();
        finalized.values().any(|set| set.contains(block_hash))
    }

    /// Dapatkan semua blok yang finalized pada height tertentu
    pub fn get_finalized_at_height(&self, height: u64) -> Option<HashSet<BlockHash>> {
        self.finalized_blocks.read().get(&height).cloned()
    }

    /// Dapatkan finalization height saat ini
    pub fn get_finalization_height(&self) -> Option<u64> {
        self.finalized_blocks.read().keys().last().copied()
    }

    /// Reset finality engine
    pub fn reset(&self) {
        self.finalized_blocks.write().clear();
        self.block_heights.write().clear();
    }

    /// Dapatkan height dari blok
    pub fn get_block_height(&self, block_hash: &BlockHash) -> Option<u64> {
        self.block_heights.read().get(block_hash).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_blocks() -> Vec<BlockHash> {
        vec![
            "block1".to_string(),
            "block2".to_string(),
            "block3".to_string(),
            "block4".to_string(),
            "block5".to_string(),
            "block6".to_string(),
            "block7".to_string(),
            "block8".to_string(),
            "block9".to_string(),
            "block10".to_string(),
        ]
    }

    #[test]
    fn test_finality_engine_creation() {
        let engine = FinalityEngine::new(3);
        assert_eq!(engine.confirmation_depth, 3);
    }

    #[test]
    fn test_compute_finality() {
        let engine = FinalityEngine::new(2);
        let blocks = create_test_blocks();

        let result = engine.compute_finality(&blocks);
        assert!(result.is_ok());
    }

    #[test]
    fn test_is_finalized() {
        let engine = FinalityEngine::new(2);
        let blocks = create_test_blocks();

        engine.compute_finality(&blocks).unwrap();

        // Blok di awal (depth > 2) harus finalized
        assert!(engine.is_finalized(&blocks[0]));
        assert!(engine.is_finalized(&blocks[1]));
        assert!(engine.is_finalized(&blocks[2]));

        // Blok di akhir mungkin belum finalized
        // (tergantung posisinya dalam ordering)
    }

    #[test]
    fn test_get_finalization_height() {
        let engine = FinalityEngine::new(3);
        let blocks = create_test_blocks();

        assert_eq!(engine.get_finalization_height(), None);

        engine.compute_finality(&blocks).unwrap();
        let height = engine.get_finalization_height();
        assert!(height.is_some());
    }

    #[test]
    fn test_reset_finality() {
        let engine = FinalityEngine::new(2);
        let blocks = create_test_blocks();

        engine.compute_finality(&blocks).unwrap();
        assert!(engine.is_finalized(&blocks[0]));

        engine.reset();
        assert!(!engine.is_finalized(&blocks[0]));
    }

    #[test]
    fn test_get_block_height() {
        let engine = FinalityEngine::new(2);
        let blocks = create_test_blocks();

        engine.compute_finality(&blocks).unwrap();

        let height0 = engine.get_block_height(&blocks[0]);
        let height9 = engine.get_block_height(&blocks[9]);

        assert!(height0.is_some());
        assert!(height9.is_some());
        assert!(height0.unwrap() < height9.unwrap());
    }
}
