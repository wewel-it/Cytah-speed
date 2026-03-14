use std::sync::Arc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use crate::state::state_manager::StateManager;
use crate::core::BlockHash;
use libp2p::PeerId;
use tokio::sync::mpsc;
use crate::network::message::NetworkMessage;

/// State synchronization untuk fast catch-up
/// Memungkinkan node yang jauh tertinggal untuk sync state secara cepat
/// daripada replay semua transaksi dari genesis
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct StateSnapshot {
    /// Root hash dari state tree yang valid
    pub state_root: [u8; 32],
    /// Block height saat snapshot dibuat
    pub block_height: u64,
    /// Block hash yang sesuai dengan state ini
    pub block_hash: BlockHash,
    /// Serialized state data
    pub state_data: Vec<u8>,
}

impl StateSnapshot {
    /// Buat snapshot dari current state manager
    pub fn from_state_manager(state: &StateManager, block_height: u64, block_hash: BlockHash) -> Result<Self, String> {
        let state_root = state.get_state_root();
        let state_data = bincode::serialize(state)
            .map_err(|e| format!("Failed to serialize state: {}", e))?;

        Ok(StateSnapshot {
            state_root,
            block_height,
            block_hash,
            state_data,
        })
    }

    /// Restore state dari snapshot
    pub fn restore_to_state_manager(&self) -> Result<StateManager, String> {
        bincode::deserialize(&self.state_data)
            .map_err(|e| format!("Failed to deserialize state: {}", e))
    }

    /// Validate snapshot integrity
    pub fn validate(&self) -> Result<(), String> {
        // Verify state can be deserialized
        let _state = self.restore_to_state_manager()?;
        
        // Verify state root matches
        if _state.get_state_root() != self.state_root {
            return Err("State root mismatch after deserialization".to_string());
        }

        Ok(())
    }

    /// Get snapshot size in bytes
    pub fn size_bytes(&self) -> usize {
        self.state_data.len()
    }
}

/// State synchronization manager untuk coordinating state sync operations
pub struct StateSyncManager {
    /// Current state reference
    state: Arc<RwLock<StateManager>>,
    /// Last synced state root (untuk tracking progress)
    last_synced_root: Arc<RwLock<[u8; 32]>>,
    /// Request sender untuk network messages
    message_sender: mpsc::UnboundedSender<(PeerId, NetworkMessage)>,
    /// Sync in progress flag
    sync_in_progress: Arc<RwLock<bool>>,
}

impl StateSyncManager {
    /// Buat state sync manager baru
    pub fn new(
        state: Arc<RwLock<StateManager>>,
        message_sender: mpsc::UnboundedSender<(PeerId, NetworkMessage)>,
    ) -> Self {
        let current_root = state.read().get_state_root();
        
        Self {
            state,
            last_synced_root: Arc::new(RwLock::new(current_root)),
            message_sender,
            sync_in_progress: Arc::new(RwLock::new(false)),
        }
    }

    /// Initiate state sync dengan peer
    /// Kirim request untuk latest state snapshot
    pub async fn request_state_sync(&self, peer: PeerId) -> Result<(), String> {
        let mut in_progress = self.sync_in_progress.write();
        
        if *in_progress {
            return Err("State sync already in progress".to_string());
        }
        
        *in_progress = true;
        drop(in_progress);

        tracing::info!("Requesting state sync from peer {}", peer);
        
        let message = NetworkMessage::RequestState;
        self.message_sender.send((peer, message))
            .map_err(|e| format!("Failed to send state request: {}", e))?;

        Ok(())
    }

    /// Handle incoming state snapshot dari peer
    pub async fn handle_state_snapshot(&self, snapshot: StateSnapshot) -> Result<(), String> {
        // Validate snapshot
        snapshot.validate()?;

        let current_state = self.state.read().get_state_root();
        
        // Jika snapshot sudah ada di local state, skip
        if snapshot.state_root == current_state {
            tracing::debug!("Snapshot already in sync with local state");
            *self.sync_in_progress.write() = false;
            return Ok(());
        }

        tracing::info!(
            "Applying state snapshot at height {} (size: {} bytes)",
            snapshot.block_height,
            snapshot.size_bytes()
        );

        // Restore state dari snapshot
        let new_state = snapshot.restore_to_state_manager()?;

        // Write ke state manager
        let mut state = self.state.write();
        *state = new_state;

        // Update last synced root
        *self.last_synced_root.write() = snapshot.state_root;

        *self.sync_in_progress.write() = false;

        tracing::info!("State sync completed at block height {}", snapshot.block_height);
        Ok(())
    }

    /// Create snapshot dari current state (untuk sharing dengan peers)
    pub fn create_snapshot(&self, block_height: u64, block_hash: BlockHash) -> Result<StateSnapshot, String> {
        let state = self.state.read();
        StateSnapshot::from_state_manager(&*state, block_height, block_hash)
    }

    /// Check jika sync sedang berlangsung
    pub fn is_syncing(&self) -> bool {
        *self.sync_in_progress.read()
    }

    /// Get last synced state root
    pub fn get_last_synced_root(&self) -> [u8; 32] {
        *self.last_synced_root.read()
    }

    /// Get current state root
    pub fn get_current_state_root(&self) -> [u8; 32] {
        self.state.read().get_state_root()
    }

    /// Check jika state is in sync
    pub fn is_state_synced(&self) -> bool {
        let current = self.get_current_state_root();
        let last_synced = self.get_last_synced_root();
        current == last_synced
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_creation() {
        let state = StateManager::new();
        let snapshot = StateSnapshot::from_state_manager(&state, 100, [0; 32])
            .expect("Should create snapshot");
        
        assert_eq!(snapshot.block_height, 100);
        assert!(snapshot.validate().is_ok());
    }

    #[test]
    fn test_snapshot_restore() {
        let state = StateManager::new();
        let snapshot = StateSnapshot::from_state_manager(&state, 50, [1; 32])
            .expect("Should create snapshot");
        
        let restored = snapshot.restore_to_state_manager()
            .expect("Should restore state");
        
        assert_eq!(restored.get_state_root(), state.get_state_root());
    }
}
