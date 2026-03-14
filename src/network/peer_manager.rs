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
    /// Reputation score; peers start with 100 and lose points for bad data.
    pub score: i32,
}

/// Peer manager for maintaining peer connections and discovery
pub struct PeerManager {
    /// Map of peer ID to peer info
    peers: Arc<RwLock<HashMap<PeerId, PeerInfo>>>,
    /// Simple ban list (peer -> expiration timestamp)
    bans: Arc<RwLock<HashMap<PeerId, u64>>>,
    /// Maximum number of peers
    max_peers: usize,
}

impl PeerManager {
    /// Create a new peer manager
    pub fn new(_peer_id: PeerId, max_peers: usize) -> Self {
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
            bans: Arc::new(RwLock::new(HashMap::new())),
            max_peers,
        }
    }

    /// Add a peer to the list
    pub fn add_peer(&self, peer_id: PeerId, address: String) -> Result<(), String> {
        let mut peers = self.peers.write();

        if peers.len() >= self.max_peers {
            return Err("Maximum peers reached".to_string());
        }

        // reject if peer is currently banned
        let now = chrono::Utc::now().timestamp() as u64;
        if let Some(exp) = self.bans.read().get(&peer_id) {
            if *exp > now {
                return Err("Peer is temporarily banned".to_string());
            } else {
                // ban expired; remove
                self.bans.write().remove(&peer_id);
            }
        }

        // If the peer exists but is disconnected, allow re-adding (e.g. after ban expiration)
        if let Some(existing) = peers.get_mut(&peer_id) {
            if existing.is_connected {
                return Err("Peer already exists".to_string());
            }
            existing.address = address;
            existing.last_seen = now;
            existing.is_connected = true;
            existing.dag_height = 0;
            existing.score = 100;
            return Ok(());
        }

        let peer_info = PeerInfo {
            peer_id: peer_id.clone(),
            address,
            last_seen: now,
            is_connected: false,
            dag_height: 0,
            score: 100,
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

    /// Perform peer discovery and log known peers.
    ///
    /// Discovery is primarily driven by the networking layer (`P2PNode`) via
    /// mDNS events. This helper iterates the current peer table and emits
    /// debug information; it can later be extended to query DNS seeds,
    /// bootstrap nodes, or other discovery mechanisms.
    pub async fn discover_peers(&self) -> Result<(), String> {
        let peers = self.peers.read();
        tracing::debug!("Running peer discovery over {} known peers", peers.len());
        for peer in peers.values() {
            tracing::debug!(
                "Known peer: {} (connected={}, last_seen={})",
                peer.peer_id,
                peer.is_connected,
                peer.last_seen
            );
        }
        Ok(())
    }

    /// Attempt to reconnect to disconnected peers.
    ///
    /// The actual P2P layer is responsible for establishing connections. This
    /// method logs attempts and updates the peer table state so that other
    /// components see a consistent view. It helps keep `peer_manager` usable
    /// even when the network behaviour is not available (e.g. during tests).
    pub async fn reconnect_peers(&self) {
        let disconnected: Vec<_> = {
            let peers = self.peers.read();
            peers.values()
                .filter(|p| !p.is_connected)
                .map(|p| p.peer_id.clone())
                .collect()
        };

        for peer_id in disconnected {
            tracing::info!("Attempting to reconnect to peer: {}", peer_id);
            // mark as connected to keep state moving; actual connection occurs
            // through the P2P network layer elsewhere
            self.update_peer_status(&peer_id, true);
        }
    }

    /// Penalize a peer for misbehavior (bad block or invalid tx).  The
    /// supplied `points` are subtracted from the peer's score; if the score
    /// reaches zero the peer is disconnected and banned for 24 hours.
    pub fn penalize_peer(&self, peer_id: &PeerId, points: i32) {
        let mut peers = self.peers.write();
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.score -= points;
            tracing::warn!("Peer {} penalized {} points, score now {}", peer_id, points, peer.score);
            if peer.score <= 0 {
                peer.is_connected = false;
                let ban_until = chrono::Utc::now().timestamp() as u64 + 24 * 3600;
                self.bans.write().insert(peer_id.clone(), ban_until);
                tracing::warn!("Peer {} has been auto-banned until {}", peer_id, ban_until);
            }
        }
    }

    /// Reward a peer for good behavior, incrementing their score up to a
    /// sane maximum to avoid unbounded growth.  This can be called when a
    /// peer reliably supplies valid blocks or transactions.
    pub fn reward_peer(&self, peer_id: &PeerId, points: i32) {
        let mut peers = self.peers.write();
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.score = (peer.score + points).min(100);
            tracing::info!("Peer {} rewarded {} points, score now {}", peer_id, points, peer.score);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::PeerId;

    fn random_peer() -> PeerId {
        PeerId::random()
    }

    #[tokio::test]
    async fn test_reputation_and_ban() {
        let pm = PeerManager::new(PeerId::random(), 10);
        let peer = random_peer();
        pm.add_peer(peer.clone(), "addr".to_string()).unwrap();
        let info = pm.get_peer_info(&peer).unwrap();
        assert_eq!(info.score, 100);
        
        pm.penalize_peer(&peer, 50);
        let info2 = pm.get_peer_info(&peer).unwrap();
        assert_eq!(info2.score, 50);
        
        pm.penalize_peer(&peer, 60);
        // should now be banned and disconnected
        let info3 = pm.get_peer_info(&peer).unwrap();
        assert!(info3.score <= 0);
        assert!(!info3.is_connected);
        assert!(pm.bans.read().contains_key(&peer));
        
        // attempt to re-add while banned should fail
        assert!(pm.add_peer(peer.clone(), "addr".to_string()).is_err());
        
        // expire ban manually
        pm.bans.write().insert(peer.clone(), 0);
        let r = pm.add_peer(peer.clone(), "addr".to_string());
        assert!(r.is_ok());
    }
}