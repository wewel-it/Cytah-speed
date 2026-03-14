use rocksdb::{DB, Options, ColumnFamilyDescriptor};
use std::sync::Arc;
use serde::{Serialize, Deserialize};
use crate::state::state_manager::StateManager;
use crate::storage::block_store::BlockStore;
use tracing::{info, warn, error, debug};

/// Column family names for pruning database
const STATE_SNAPSHOTS_CF: &str = "state_snapshots";
const PRUNING_METADATA_CF: &str = "pruning_metadata";

/// State snapshot for preserving account balances during pruning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    pub height: u64,
    pub timestamp: u64,
    pub state_root: [u8; 32],
    pub total_supply: u64,
    pub active_accounts: u64,
}

/// Pruning metadata for tracking pruning operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruningMetadata {
    pub last_pruned_height: u64,
    pub total_blocks_pruned: u64,
    pub total_size_pruned: u64,
    pub last_snapshot_height: u64,
    pub snapshot_count: u32,
}

/// Rolling window pruner with state snapshots
/// Implements the protocol completion guide requirements:
/// - Rolling pruning windows to maintain bounded storage
/// - State snapshots before pruning to preserve account balances
/// - Efficient batch deletion of old blocks
/// - Metadata tracking for pruning operations
#[derive(Clone)]
pub struct RollingWindowPruner {
    /// RocksDB instance for pruning metadata and snapshots
    db: Arc<DB>,
    /// Block store reference for pruning operations
    block_store: BlockStore,
    /// Window size (blocks to keep)
    window_size: u64,
    /// Snapshot interval (take snapshot every N blocks)
    snapshot_interval: u64,
    /// Minimum height before pruning starts
    pruning_threshold: u64,
}

impl RollingWindowPruner {
    /// Create new rolling window pruner
    pub fn new(
        path: &str,
        block_store: BlockStore,
        window_size: u64,
        snapshot_interval: u64,
        pruning_threshold: u64,
    ) -> Result<Self, String> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.set_max_open_files(500);
        opts.set_write_buffer_size(32 * 1024 * 1024); // 32MB

        // Create column family descriptors
        let cf_descriptors = vec![
            ColumnFamilyDescriptor::new(STATE_SNAPSHOTS_CF, Options::default()),
            ColumnFamilyDescriptor::new(PRUNING_METADATA_CF, Options::default()),
        ];

        let db = match DB::open_cf_descriptors(&opts, path, cf_descriptors) {
            Ok(db) => db,
            Err(e) => {
                // Try to open without CFs first (for migration)
                match DB::open(&opts, path) {
                    Ok(mut db) => {
                        // Create CFs if they don't exist
                        if db.cf_handle(STATE_SNAPSHOTS_CF).is_none() {
                            let _ = db.create_cf(STATE_SNAPSHOTS_CF, &Options::default());
                        }
                        if db.cf_handle(PRUNING_METADATA_CF).is_none() {
                            let _ = db.create_cf(PRUNING_METADATA_CF, &Options::default());
                        }
                        db
                    }
                    Err(_) => return Err(format!("Failed to open pruning database: {}", e)),
                }
            }
        };

        Ok(Self {
            db: Arc::new(db),
            block_store,
            window_size,
            snapshot_interval,
            pruning_threshold,
        })
    }

    /// Check if pruning should be performed at current height
    pub fn should_prune(&self, current_height: u64) -> bool {
        current_height >= self.pruning_threshold
    }

    /// Perform pruning operation if needed
    /// Returns the number of blocks pruned
    pub fn maybe_prune(&self, current_height: u64, state: &mut StateManager) -> Result<u64, String> {
        if !self.should_prune(current_height) {
            return Ok(0);
        }

        // Take state snapshot if needed
        self.take_snapshot_if_needed(current_height, state)?;

        // Calculate pruning range
        let metadata = self.get_metadata()?;
        let prune_up_to = if current_height > self.window_size {
            current_height.saturating_sub(self.window_size)
        } else {
            0
        };

        if prune_up_to <= metadata.last_pruned_height {
            return Ok(0);
        }

        // Get blocks to prune
        let blocks_to_prune = self.block_store
            .get_blocks_by_height_range(metadata.last_pruned_height + 1, prune_up_to)?;

        if blocks_to_prune.is_empty() {
            return Ok(0);
        }

        info!("Pruning {} blocks from height {} to {}",
              blocks_to_prune.len(), metadata.last_pruned_height + 1, prune_up_to);

        // Calculate total size being pruned
        let mut total_size_pruned = 0u64;
        for block in &blocks_to_prune {
            if let Ok(Some(metadata)) = self.block_store.get_block_metadata(&block.hash) {
                total_size_pruned += metadata.size_bytes as u64;
            }
        }

        // Delete blocks in batches
        for block in &blocks_to_prune {
            self.block_store.delete_block(&block.hash)?;
        }

        // Update metadata
        let new_metadata = PruningMetadata {
            last_pruned_height: prune_up_to,
            total_blocks_pruned: metadata.total_blocks_pruned + blocks_to_prune.len() as u64,
            total_size_pruned: metadata.total_size_pruned + total_size_pruned,
            last_snapshot_height: metadata.last_snapshot_height,
            snapshot_count: metadata.snapshot_count,
        };

        self.save_metadata(&new_metadata)?;

        // Compact database after pruning
        self.block_store.compact()?;

        info!("Pruning completed: {} blocks removed, {} bytes freed",
              blocks_to_prune.len(), total_size_pruned);

        Ok(blocks_to_prune.len() as u64)
    }

    /// Take state snapshot if needed based on snapshot interval
    fn take_snapshot_if_needed(&self, current_height: u64, state: &mut StateManager) -> Result<(), String> {
        let metadata = self.get_metadata()?;

        if current_height - metadata.last_snapshot_height >= self.snapshot_interval {
            self.take_state_snapshot(current_height, state)?;
        }

        Ok(())
    }

    /// Take a state snapshot at current height
    pub fn take_state_snapshot(&self, height: u64, state: &mut StateManager) -> Result<(), String> {
        let state_root = state.get_state_root();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Get additional state statistics
        let total_supply = state.get_total_supply();
        let active_accounts = state.get_active_account_count();

        let snapshot = StateSnapshot {
            height,
            timestamp,
            state_root,
            total_supply,
            active_accounts,
        };

        let snapshot_key = format!("snapshot_{:010}", height);
        let snapshot_data = bincode::serialize(&snapshot)
            .map_err(|e| format!("Failed to serialize snapshot: {}", e))?;

        let snapshots_cf = self.db.cf_handle(STATE_SNAPSHOTS_CF)
            .ok_or("State snapshots column family not found")?;

        self.db.put_cf(&snapshots_cf, snapshot_key.as_bytes(), &snapshot_data)
            .map_err(|e| format!("Failed to save snapshot: {}", e))?;

        // Update metadata
        let mut metadata = self.get_metadata()?;
        metadata.last_snapshot_height = height;
        metadata.snapshot_count += 1;
        self.save_metadata(&metadata)?;

        info!("State snapshot taken at height {}: root={}, supply={}, accounts={}",
              height, hex::encode(&state_root[..8]), total_supply, active_accounts);

        Ok(())
    }

    /// Get state snapshot by height
    pub fn get_state_snapshot(&self, height: u64) -> Result<Option<StateSnapshot>, String> {
        let snapshots_cf = self.db.cf_handle(STATE_SNAPSHOTS_CF)
            .ok_or("State snapshots column family not found")?;

        let snapshot_key = format!("snapshot_{:010}", height);

        match self.db.get_cf(&snapshots_cf, snapshot_key.as_bytes()) {
            Ok(Some(data)) => {
                let snapshot: StateSnapshot = bincode::deserialize(&data)
                    .map_err(|e| format!("Failed to deserialize snapshot: {}", e))?;
                Ok(Some(snapshot))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(format!("Database error: {}", e)),
        }
    }

    /// Get latest state snapshot
    pub fn get_latest_snapshot(&self) -> Result<Option<StateSnapshot>, String> {
        let metadata = self.get_metadata()?;
        if metadata.last_snapshot_height == 0 {
            return Ok(None);
        }
        self.get_state_snapshot(metadata.last_snapshot_height)
    }

    /// Get pruning metadata
    pub fn get_metadata(&self) -> Result<PruningMetadata, String> {
        let meta_cf = self.db.cf_handle(PRUNING_METADATA_CF)
            .ok_or("Pruning metadata column family not found")?;

        match self.db.get_cf(&meta_cf, b"metadata") {
            Ok(Some(data)) => {
                let metadata: PruningMetadata = bincode::deserialize(&data)
                    .map_err(|e| format!("Failed to deserialize metadata: {}", e))?;
                Ok(metadata)
            }
            Ok(None) => {
                // Return default metadata if none exists
                Ok(PruningMetadata {
                    last_pruned_height: 0,
                    total_blocks_pruned: 0,
                    total_size_pruned: 0,
                    last_snapshot_height: 0,
                    snapshot_count: 0,
                })
            }
            Err(e) => Err(format!("Database error: {}", e)),
        }
    }

    /// Save pruning metadata
    fn save_metadata(&self, metadata: &PruningMetadata) -> Result<(), String> {
        let meta_cf = self.db.cf_handle(PRUNING_METADATA_CF)
            .ok_or("Pruning metadata column family not found")?;

        let metadata_data = bincode::serialize(metadata)
            .map_err(|e| format!("Failed to serialize metadata: {}", e))?;

        self.db.put_cf(&meta_cf, b"metadata", &metadata_data)
            .map_err(|e| format!("Failed to save metadata: {}", e))?;

        Ok(())
    }

    /// Get pruning statistics
    pub fn get_stats(&self) -> Result<PruningStats, String> {
        let metadata = self.get_metadata()?;
        let block_stats = self.block_store.get_stats()?;

        Ok(PruningStats {
            window_size: self.window_size,
            pruning_threshold: self.pruning_threshold,
            snapshot_interval: self.snapshot_interval,
            last_pruned_height: metadata.last_pruned_height,
            total_blocks_pruned: metadata.total_blocks_pruned,
            total_size_pruned: metadata.total_size_pruned,
            snapshot_count: metadata.snapshot_count,
            current_block_count: block_stats.block_count,
            current_storage_size: block_stats.total_size_bytes,
        })
    }

    /// Force compaction of pruning database
    pub fn compact(&self) -> Result<(), String> {
        let snapshots_cf = self.db.cf_handle(STATE_SNAPSHOTS_CF)
            .ok_or("State snapshots column family not found")?;
        let meta_cf = self.db.cf_handle(PRUNING_METADATA_CF)
            .ok_or("Pruning metadata column family not found")?;

        // Compact all column families
        self.db.compact_range_cf(&snapshots_cf, None::<&[u8]>, None::<&[u8]>);
        self.db.compact_range_cf(&meta_cf, None::<&[u8]>, None::<&[u8]>);

        info!("Pruning database compaction completed");
        Ok(())
    }
}

/// Pruning statistics
#[derive(Debug, Clone)]
pub struct PruningStats {
    pub window_size: u64,
    pub pruning_threshold: u64,
    pub snapshot_interval: u64,
    pub last_pruned_height: u64,
    pub total_blocks_pruned: u64,
    pub total_size_pruned: u64,
    pub snapshot_count: u32,
    pub current_block_count: usize,
    pub current_storage_size: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use crate::storage::block_store::BlockStore;

    #[test]
    fn test_pruner_basic() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("blocks.db");
        let pruning_path = temp_dir.path().join("pruning.db");

        let block_store = BlockStore::new_with_path(db_path.to_str().unwrap()).unwrap();
        let pruner = RollingWindowPruner::new(
            pruning_path.to_str().unwrap(),
            block_store,
            100_000,
            10_000,
            200_000,
        ).unwrap();

        let mut state = StateManager::new();

        // Before threshold, no pruning
        let pruned = pruner.maybe_prune(199_999, &mut state).unwrap();
        assert_eq!(pruned, 0);

        // At threshold, should start pruning logic
        let pruned = pruner.maybe_prune(200_000, &mut state).unwrap();
        assert_eq!(pruned, 0); // No blocks to prune yet
    }

    #[test]
    fn test_state_snapshot() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("blocks.db");
        let pruning_path = temp_dir.path().join("pruning.db");

        let block_store = BlockStore::new_with_path(db_path.to_str().unwrap()).unwrap();
        let pruner = RollingWindowPruner::new(
            pruning_path.to_str().unwrap(),
            block_store,
            100_000,
            10_000,
            200_000,
        ).unwrap();

        let mut state = StateManager::new();

        // Take snapshot
        pruner.take_state_snapshot(1000, &mut state).unwrap();

        // Retrieve snapshot
        let snapshot = pruner.get_state_snapshot(1000).unwrap().unwrap();
        assert_eq!(snapshot.height, 1000);
    }
}