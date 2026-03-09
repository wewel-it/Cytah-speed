use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use tokio::time::{Duration, interval};
use libp2p::PeerId;

/// Information about a connected peer
#[derive(Clone, Debug)]
pub struct PeerInfo {
    pub peer_id: PeerId,
    pub address: String,
    pub last_seen: u64,
    pub is_connected: bool,
    pub dag_height: u64,
}

/// Peer manager for maintaining peer connections and discovery
pub struct PeerManager {
    /// Map of peer ID to peer info
    peers: Arc<RwLock<HashMap<PeerId, PeerInfo>>>,
    /// Maximum number of peers
    max_peers: usize,
}

impl PeerManager {
    /// Create a new peer manager
    pub fn new(_peer_id: PeerId, max_peers: usize) -> Self {
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
            max_peers,
        }
    }

    /// Add a peer to the list
    pub fn add_peer(&self, peer_id: PeerId, address: String) -> Result<(), String> {
        let mut peers = self.peers.write();

        if peers.len() >= self.max_peers {
            return Err("Maximum peers reached".to_string());
        }

        if peers.contains_key(&peer_id) {
            return Err("Peer already exists".to_string());
        }

        let peer_info = PeerInfo {
            peer_id: peer_id.clone(),
            address,
            last_seen: chrono::Utc::now().timestamp() as u64,
            is_connected: false,
            dag_height: 0,
        };

        peers.insert(peer_id, peer_info);
        Ok(())
    }

    /// Remove a peer
    pub fn remove_peer(&self, peer_id: &PeerId) {
        let mut peers = self.peers.write();
        peers.remove(peer_id);
    }

    /// Update peer connection status
    pub fn update_peer_status(&self, peer_id: &PeerId, connected: bool) {
        let mut peers = self.peers.write();
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.is_connected = connected;
            peer.last_seen = chrono::Utc::now().timestamp() as u64;
        }
    }

    /// Update peer DAG height
    pub fn update_peer_height(&self, peer_id: &PeerId, height: u64) {
        let mut peers = self.peers.write();
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.dag_height = height;
        }
    }

    /// Get all connected peers
    pub fn get_connected_peers(&self) -> Vec<PeerId> {
        let peers = self.peers.read();
        peers.values()
            .filter(|p| p.is_connected)
            .map(|p| p.peer_id.clone())
            .collect()
    }

    /// Get all known peers
    pub fn get_all_peers(&self) -> Vec<PeerId> {
        let peers = self.peers.read();
        peers.keys().cloned().collect()
    }

    /// Get peer info
    pub fn get_peer_info(&self, peer_id: &PeerId) -> Option<PeerInfo> {
        let peers = self.peers.read();
        peers.get(peer_id).cloned()
    }

    /// Get peers with highest DAG heights for sync
    pub fn get_sync_peers(&self, count: usize) -> Vec<PeerId> {
        let peers = self.peers.read();
        let mut peer_list: Vec<_> = peers.values()
            .filter(|p| p.is_connected)
            .collect();

        peer_list.sort_by(|a, b| b.dag_height.cmp(&a.dag_height));

        peer_list.into_iter()
            .take(count)
            .map(|p| p.peer_id.clone())
            .collect()
    }

    /// Periodic cleanup of stale peers
    pub async fn cleanup_stale_peers(&self) {
        let mut interval = interval(Duration::from_secs(300)); // 5 minutes

        loop {
            interval.tick().await;

            let mut peers = self.peers.write();
            let now = chrono::Utc::now().timestamp() as u64;
            let stale_threshold = 600; // 10 minutes

            peers.retain(|_, peer| {
                if now - peer.last_seen > stale_threshold {
                    tracing::info!("Removing stale peer: {}", peer.peer_id);
                    false
                } else {
                    true
                }
            });
        }
    }

    /// Handle peer discovery (placeholder for mDNS/Kademlia)
    pub async fn discover_peers(&self) -> Result<(), String> {
        // In a real implementation, this would use mDNS or Kademlia DHT
        // For now, this is a placeholder
        tracing::debug!("Peer discovery running...");
        Ok(())
    }

    /// Attempt to reconnect to disconnected peers
    pub async fn reconnect_peers(&self) {
        let peers = self.peers.read();
        let disconnected: Vec<_> = peers.values()
            .filter(|p| !p.is_connected)
            .map(|p| p.peer_id.clone())
            .collect();

        for peer_id in disconnected {
            tracing::info!("Attempting to reconnect to peer: {}", peer_id);
            // In real implementation, attempt reconnection
        }
    }
}