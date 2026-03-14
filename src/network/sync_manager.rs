use std::collections::HashSet;
use std::sync::Arc;
use parking_lot::RwLock;
use tokio::sync::mpsc;
use libp2p::PeerId;
use crate::core::{Block, BlockHash};
use crate::core::block::BlockHeader;
use crate::dag::blockdag::BlockDAG;
use crate::network::message::NetworkMessage;
use crate::network::PeerManager;
use crate::state::state_manager::StateManager;

/// Sync manager for DAG synchronization across peers
pub struct SyncManager {
    /// Reference to the local DAG
    dag: Arc<RwLock<BlockDAG>>,
    /// Peer manager reference
    peer_manager: Arc<PeerManager>,
    /// Channel for sending network messages
    message_sender: mpsc::UnboundedSender<(PeerId, NetworkMessage)>,
    /// Set of blocks we're currently requesting
    pending_requests: Arc<RwLock<HashSet<BlockHash>>>,
    /// Optional state manager for fast sync
    pub state: Arc<parking_lot::Mutex<StateManager>>,
}

impl SyncManager {
    /// Create a new sync manager
    pub fn new(
        dag: Arc<RwLock<BlockDAG>>,
        peer_manager: Arc<PeerManager>,
        message_sender: mpsc::UnboundedSender<(PeerId, NetworkMessage)>,
        state: Arc<parking_lot::Mutex<StateManager>>,
    ) -> Self {
        Self {
            dag,
            peer_manager,
            message_sender,
            pending_requests: Arc::new(RwLock::new(HashSet::new())),
            state,
        }
    }

    /// Start the sync process
    pub async fn start_sync(&self) -> Result<(), String> {
        tracing::info!("Starting DAG synchronization...");

        // Request the entire DAG from a sync peer
        let sync_peers = self.peer_manager.get_sync_peers(1);
        if let Some(peer) = sync_peers.first() {
            let message = NetworkMessage::RequestDag;
            self.message_sender.send((peer.clone(), message))
                .map_err(|e| format!("Failed to send sync request: {}", e))?;
        }

        Ok(())
    }

    /// Handle incoming DAG response
    pub async fn handle_dag_response(&self, blocks: Vec<Block>) -> Result<(), String> {
        tracing::info!("Received {} blocks in DAG sync response", blocks.len());

        let mut dag = self.dag.write();

        for block in blocks {
            // Validate and insert block
            if block.validate_basic().is_ok() {
                dag.insert_block(block)?;
            } else {
                tracing::warn!("Received invalid block during sync");
            }
        }

        tracing::info!("DAG synchronization completed");
        Ok(())
    }

    /// Detect missing blocks by comparing with peers
    /// This function queries sync-capable peers for their DAG tips,
    /// compares with local tips, and identifies blocks that we're missing.
    pub async fn detect_missing_blocks(&self) -> Result<Vec<BlockHash>, String> {
        let _local_tips = {
            let dag = self.dag.read();
            dag.get_tips()
        };

        let sync_peers = self.peer_manager.get_sync_peers(3);
        if sync_peers.is_empty() {
            return Ok(Vec::new()); // No sync peers available
        }

        let missing_blocks = HashSet::new();
        let dag_read = self.dag.read();

        // Query each peer for their tips and request missing ancestors
        for peer in &sync_peers {
            // Request headers starting from our local tips
            // The peer will respond with blocks we don't have
            let message = NetworkMessage::GetHeaders {
                from: [0u8; 32], // Request from genesis (all new blocks)
                max: 1000,       // Request up to 1000 headers at a time
            };
            
            if let Err(e) = self.message_sender.send((peer.clone(), message)) {
                tracing::warn!("Failed to request headers from peer {}: {}", peer, e);
                continue;
            }
        }

        drop(dag_read); // Release lock

        // NOTE: The actual block comparison happens when we receive
        // headers from peers in handle_headers(). This function initiates
        // the discovery process. In a more sophisticated implementation,
        // we could maintain bidirectional state and keep track of peer DAG
        // heights to make smarter decisions about which blocks to request.

        Ok(missing_blocks.into_iter().collect())
    }

    /// Request missing blocks from peers
    pub async fn request_missing_blocks(&self, block_hashes: Vec<BlockHash>) -> Result<(), String> {
        let sync_peers = self.peer_manager.get_sync_peers(3); // Request from up to 3 peers

        for hash in block_hashes {
            // Mark as pending
            {
                let mut pending = self.pending_requests.write();
                if pending.contains(&hash) {
                    continue; // Already requesting
                }
                pending.insert(hash.clone());
            }

            // Send request to peers
            for peer in &sync_peers {
                let message = NetworkMessage::RequestBlock(hash.clone());
                self.message_sender.send((peer.clone(), message))
                    .map_err(|e| format!("Failed to send block request: {}", e))?;
            }
        }

        Ok(())
    }

    /// Handle incoming block response
    pub async fn handle_block_response(&self, block: Option<Block>) -> Result<(), String> {
        if let Some(block) = block {
            // Remove from pending
            {
                let mut pending = self.pending_requests.write();
                pending.remove(&block.hash);
            }

            // Validate and insert
            if block.validate_basic().is_ok() {
                let mut dag = self.dag.write();
                dag.insert_block(block)?;
                tracing::debug!("Inserted synced block");
            } else {
                tracing::warn!("Received invalid block");
            }
        }

        Ok(())
    }

    /// Check if we're fully synced
    pub fn is_synced(&self) -> bool {
        let pending = self.pending_requests.read();
        pending.is_empty()
    }

    /// Process a batch of headers received from a peer.
    ///
    /// Each header is accompanied by the block hash so we can request missing
    /// blocks directly. This implementation detects which hashes are missing
    /// locally and requests them from available peers.
    pub async fn handle_headers(&self, headers: Vec<(BlockHash, BlockHeader)>) -> Result<(), String> {
        let mut missing = Vec::new();

        {
            let dag = self.dag.read();
            for (hash, _header) in headers {
                if dag.get_block(&hash).is_none() {
                    missing.push(hash);
                }
            }
        }

        if !missing.is_empty() {
            tracing::info!(count = missing.len(), "Detected missing blocks from headers, requesting...");
            self.request_missing_blocks(missing).await?;
        }

        Ok(())
    }

    /// Handle incoming sync-related network message
    pub async fn handle_message(&self, peer: PeerId, msg: NetworkMessage) -> Result<(), String> {
        match msg {
            NetworkMessage::GetHeaders { from, max } => {
                // locate headers after `from`
                let dag = self.dag.read();
                let order = dag.get_topological_order();
                let mut headers = Vec::new();
                let mut include = from == [0u8;32];
                for hash in order {
                    if !include {
                        if hash == from {
                            include = true;
                        }
                        continue;
                    }
                    if let Some(b) = dag.get_block(&hash) {
                        headers.push((hash, b.header.clone()));
                        if headers.len() >= max {
                            break;
                        }
                    }
                }
                let _ = self.message_sender.send((peer, NetworkMessage::Headers(headers)));
                Ok(())
            }
            NetworkMessage::Headers(headers) => {
                // Process incoming headers and request missing blocks
                self.handle_headers(headers).await
            }
            NetworkMessage::GetBlocks(hashes) => {
                let mut blocks = Vec::new();
                let dag = self.dag.read();
                for h in hashes {
                    if let Some(b) = dag.get_block(&h) {
                        blocks.push(b);
                    }
                }
                let _ = self.message_sender.send((peer, NetworkMessage::Blocks(blocks)));
                Ok(())
            }
            NetworkMessage::RequestState => {
                // serialize current state
                let snapshot = {
                    let s = self.state.lock();
                    bincode::serialize(&*s).map_err(|e| e.to_string())?
                };
                let _ = self.message_sender.send((peer, NetworkMessage::StateSnapshot(snapshot)));
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Get sync status
    pub fn get_sync_status(&self) -> (usize, usize) {
        let dag = self.dag.read();
        let local_blocks = dag.get_all_blocks().len();
        let pending = self.pending_requests.read();
        (local_blocks, pending.len())
    }

    /// Periodic sync check
    pub async fn periodic_sync_check(&self) {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));

        loop {
            interval.tick().await;

            if !self.is_synced() {
                continue; // Still syncing
            }

            // Check for missing blocks
            if let Ok(missing) = self.detect_missing_blocks().await {
                if !missing.is_empty() {
                    tracing::info!("Detected {} missing blocks, requesting...", missing.len());
                    if let Err(e) = self.request_missing_blocks(missing).await {
                        tracing::error!("Failed to request missing blocks: {}", e);
                    }
                }
            }
        }
    }
}