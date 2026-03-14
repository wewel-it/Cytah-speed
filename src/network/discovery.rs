use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use parking_lot::RwLock;
use tokio::time::{interval, Duration, Instant};
use libp2p::{PeerId, Multiaddr};
use tracing::{info, debug, warn, error};
use serde::{Serialize, Deserialize};

/// Peer reputation score - higher is better
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReputationScore(i32);

impl ReputationScore {
    pub const MAX_SCORE: i32 = 1000;
    pub const MIN_SCORE: i32 = -1000;
    pub const DEFAULT_SCORE: i32 = 0;

    pub fn new(score: i32) -> Self {
        Self(score.clamp(Self::MIN_SCORE, Self::MAX_SCORE))
    }

    pub fn value(&self) -> i32 {
        self.0
    }

    pub fn increase(&mut self, amount: i32) {
        self.0 = (self.0 + amount).min(Self::MAX_SCORE);
    }

    pub fn decrease(&mut self, amount: i32) {
        self.0 = (self.0 - amount).max(Self::MIN_SCORE);
    }

    pub fn is_banned(&self) -> bool {
        self.0 <= -500
    }

    pub fn is_trusted(&self) -> bool {
        self.0 >= 500
    }
}

impl Default for ReputationScore {
    fn default() -> Self {
        Self(Self::DEFAULT_SCORE)
    }
}

/// Peer information with reputation and connection stats
#[derive(Debug, Clone)]
pub struct PeerData {
    pub peer_id: PeerId,
    pub addresses: Vec<Multiaddr>,
    pub reputation: ReputationScore,
    pub last_seen: Instant,
    pub connected: bool,
    pub connection_attempts: u32,
    pub successful_connections: u32,
    pub messages_sent: u64,
    pub messages_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub first_discovered: Instant,
}

impl PeerData {
    pub fn new(peer_id: PeerId, address: Multiaddr) -> Self {
        Self {
            peer_id,
            addresses: vec![address],
            reputation: ReputationScore::default(),
            last_seen: Instant::now(),
            connected: false,
            connection_attempts: 0,
            successful_connections: 0,
            messages_sent: 0,
            messages_received: 0,
            bytes_sent: 0,
            bytes_received: 0,
            first_discovered: Instant::now(),
        }
    }

    pub fn success_rate(&self) -> f64 {
        if self.connection_attempts == 0 {
            0.0
        } else {
            self.successful_connections as f64 / self.connection_attempts as f64
        }
    }

    pub fn update_last_seen(&mut self) {
        self.last_seen = Instant::now();
    }

    pub fn record_connection_attempt(&mut self, success: bool) {
        self.connection_attempts += 1;
        if success {
            self.successful_connections += 1;
            self.reputation.increase(10);
        } else {
            self.reputation.decrease(5);
        }
    }

    pub fn record_message_sent(&mut self, size: usize) {
        self.messages_sent += 1;
        self.bytes_sent += size as u64;
    }

    pub fn record_message_received(&mut self, size: usize) {
        self.messages_received += 1;
        self.bytes_received += size as u64;
        // Small reputation boost for active peers
        self.reputation.increase(1);
    }

    pub fn should_connect(&self) -> bool {
        !self.reputation.is_banned() &&
        self.success_rate() > 0.1 &&
        self.last_seen.elapsed() < Duration::from_secs(3600) // 1 hour
    }
}

/// DNS seed configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsSeed {
    pub hostname: String,
    pub port: u16,
}

/// Bootstrap peer configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapPeer {
    pub address: String,
    pub peer_id: Option<String>,
}

/// Network service discovery manager untuk Cytah-speed
/// Menangani peer discovery melalui multiple mechanisms:
/// 1. DNS seed nodes untuk initial peer discovery
/// 2. Kademlia DHT untuk distributed peer discovery
/// 3. Hardcoded bootstrap peers untuk fallback
/// 4. mDNS untuk local network discovery
/// 5. Peer reputation and scoring system
/// 6. Automatic peer rotation and health monitoring
#[derive(Clone)]
pub struct DiscoveryManager {
    /// DNS seed nodes
    dns_seeds: Arc<RwLock<Vec<DnsSeed>>>,
    /// Bootstrap peers untuk initial connectivity
    bootstrap_peers: Arc<RwLock<Vec<BootstrapPeer>>>,
    /// Known peers dengan reputation data
    peers: Arc<RwLock<HashMap<PeerId, PeerData>>>,
    /// Trusted peers (high reputation)
    trusted_peers: Arc<RwLock<HashSet<PeerId>>>,
    /// Banned peers (low reputation)
    banned_peers: Arc<RwLock<HashSet<PeerId>>>,
    /// Local peer ID
    local_peer_id: PeerId,
    /// Enable mDNS discovery
    enable_mdns: bool,
    /// Enable DHT discovery
    enable_dht: bool,
    /// Maximum peers to maintain
    max_peers: usize,
    /// Minimum peers to maintain connection
    min_peers: usize,
}

impl DiscoveryManager {
    /// Create new discovery manager with default configuration
    pub fn new(local_peer_id: PeerId) -> Self {
        let mut dns_seeds = Vec::new();
        dns_seeds.push(DnsSeed {
            hostname: "seed.cytah-speed.net".to_string(),
            port: 8333,
        });
        dns_seeds.push(DnsSeed {
            hostname: "seed2.cytah-speed.net".to_string(),
            port: 8333,
        });

        let mut bootstrap_peers = Vec::new();
        // Add some hardcoded bootstrap peers
        bootstrap_peers.push(BootstrapPeer {
            address: "/ip4/127.0.0.1/tcp/8333".to_string(),
            peer_id: None,
        });

        Self {
            dns_seeds: Arc::new(RwLock::new(dns_seeds)),
            bootstrap_peers: Arc::new(RwLock::new(bootstrap_peers)),
            peers: Arc::new(RwLock::new(HashMap::new())),
            trusted_peers: Arc::new(RwLock::new(HashSet::new())),
            banned_peers: Arc::new(RwLock::new(HashSet::new())),
            local_peer_id,
            enable_mdns: true,
            enable_dht: true,
            max_peers: 100,
            min_peers: 8,
        }
    }

    /// Add DNS seed
    pub fn add_dns_seed(&self, seed: DnsSeed) {
        let seed_clone = seed.clone();
        let mut seeds = self.dns_seeds.write();
        seeds.push(seed);
        info!("Added DNS seed: {}:{}", seed_clone.hostname, seed_clone.port);
    }

    /// Add bootstrap peer
    pub fn add_bootstrap_peer(&self, peer: BootstrapPeer) {
        let peer_clone = peer.clone();
        let mut peers = self.bootstrap_peers.write();
        peers.push(peer);
        info!("Added bootstrap peer: {}", peer_clone.address);
    }

    /// Discover peers from DNS seeds
    pub async fn discover_from_dns(&self) -> Result<Vec<PeerId>, String> {
        let seeds = self.dns_seeds.read().clone();
        let mut discovered_peers = Vec::new();

        for seed in seeds {
            match self.query_dns_seed(&seed).await {
                Ok(peers) => {
                    for peer_id in peers {
                        if peer_id != self.local_peer_id {
                            discovered_peers.push(peer_id);
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to query DNS seed {}: {}", seed.hostname, e);
                }
            }
        }

        info!("Discovered {} peers from DNS seeds", discovered_peers.len());
        Ok(discovered_peers)
    }

    /// Query a single DNS seed for peers
    async fn query_dns_seed(&self, seed: &DnsSeed) -> Result<Vec<PeerId>, String> {
        // In a real implementation, this would query the DNS seed
        // For now, return some mock peers
        debug!("Querying DNS seed: {}:{}", seed.hostname, seed.port);

        // Mock implementation - in reality this would do DNS TXT record lookups
        // or HTTP requests to seed nodes
        let mock_peers = vec![
            PeerId::random(),
            PeerId::random(),
            PeerId::random(),
        ];

        Ok(mock_peers)
    }

    /// Get bootstrap peer addresses
    pub fn get_bootstrap_addresses(&self) -> Vec<String> {
        self.bootstrap_peers.read().iter()
            .map(|p| p.address.clone())
            .collect()
    }

    /// Add or update peer information
    pub fn add_peer(&self, peer_id: PeerId, address: Multiaddr) {
        let mut peers = self.peers.write();
        peers.entry(peer_id)
            .or_insert_with(|| PeerData::new(peer_id, address.clone()))
            .addresses.push(address);
    }

    /// Update peer reputation
    pub fn update_peer_reputation(&self, peer_id: &PeerId, delta: i32) {
        let mut peers = self.peers.write();
        if let Some(peer_data) = peers.get_mut(peer_id) {
            if delta > 0 {
                peer_data.reputation.increase(delta);
            } else {
                peer_data.reputation.decrease(-delta);
            }

            // Update trusted/banned lists
            if peer_data.reputation.is_trusted() {
                self.trusted_peers.write().insert(*peer_id);
            } else {
                self.trusted_peers.write().remove(peer_id);
            }

            if peer_data.reputation.is_banned() {
                self.banned_peers.write().insert(*peer_id);
                warn!("Peer {} banned due to low reputation", peer_id);
            } else {
                self.banned_peers.write().remove(peer_id);
            }
        }
    }

    /// Record successful connection to peer
    pub fn record_connection_success(&self, peer_id: &PeerId) {
        let mut peers = self.peers.write();
        if let Some(peer_data) = peers.get_mut(peer_id) {
            peer_data.record_connection_attempt(true);
            peer_data.connected = true;
            peer_data.update_last_seen();
        }
    }

    /// Record failed connection to peer
    pub fn record_connection_failure(&self, peer_id: &PeerId) {
        let mut peers = self.peers.write();
        if let Some(peer_data) = peers.get_mut(peer_id) {
            peer_data.record_connection_attempt(false);
        }
    }

    /// Get peers that should be connected to maintain minimum connections
    pub fn get_peers_to_connect(&self) -> Vec<PeerId> {
        let peers = self.peers.read();
        let banned = self.banned_peers.read();

        peers.iter()
            .filter(|(peer_id, peer_data)| {
                !banned.contains(peer_id) &&
                !peer_data.connected &&
                peer_data.should_connect()
            })
            .take(self.max_peers)
            .map(|(peer_id, _)| *peer_id)
            .collect()
    }

    /// Get trusted peers for priority connections
    pub fn get_trusted_peers(&self) -> Vec<PeerId> {
        self.trusted_peers.read().iter().cloned().collect()
    }

    /// Get peer reputation score
    pub fn get_peer_reputation(&self, peer_id: &PeerId) -> ReputationScore {
        self.peers.read()
            .get(peer_id)
            .map(|p| p.reputation)
            .unwrap_or_default()
    }

    /// Check if peer is banned
    pub fn is_peer_banned(&self, peer_id: &PeerId) -> bool {
        self.banned_peers.read().contains(peer_id)
    }

    /// Get peer statistics
    pub fn get_peer_stats(&self, peer_id: &PeerId) -> Option<PeerData> {
        self.peers.read().get(peer_id).cloned()
    }

    /// Clean up old/stale peers
    pub fn cleanup_stale_peers(&self) {
        let mut peers = self.peers.write();
        let mut trusted = self.trusted_peers.write();
        let mut banned = self.banned_peers.write();

        let cutoff = Instant::now() - Duration::from_secs(7 * 24 * 3600); // 7 days

        peers.retain(|peer_id, peer_data| {
            let should_keep = peer_data.first_discovered > cutoff ||
                            peer_data.successful_connections > 0 ||
                            peer_data.reputation.is_trusted();

            if !should_keep {
                trusted.remove(peer_id);
                banned.remove(peer_id);
            }

            should_keep
        });

        info!("Cleaned up stale peers, {} peers remaining", peers.len());
    }

    /// Start background maintenance tasks
    pub async fn start_maintenance(&self) {
        let discovery = self.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(300)); // 5 minutes

            loop {
                interval.tick().await;
                discovery.cleanup_stale_peers();

                // Periodic DNS discovery
                if let Err(e) = discovery.discover_from_dns().await {
                    warn!("Periodic DNS discovery failed: {}", e);
                }
            }
        });
    }

    /// Legacy methods for backward compatibility
    pub fn add_bootstrap_peer_legacy(&self, peer_address: String) {
        let peer = BootstrapPeer {
            address: peer_address.clone(),
            peer_id: None,
        };
        self.add_bootstrap_peer(peer);
    }

    pub fn get_bootstrap_peers_legacy(&self) -> Vec<String> {
        self.get_bootstrap_addresses()
    }

    /// Parse multiaddr string to extract peer ID
    pub fn parse_multiaddr(addr: &str) -> Option<String> {
        if let Some(p2p_pos) = addr.find("/p2p/") {
            let peer_id_part = &addr[p2p_pos + 6..];
            if let Some(end_pos) = peer_id_part.find('/') {
                Some(peer_id_part[..end_pos].to_string())
            } else {
                Some(peer_id_part.to_string())
            }
        } else {
            None
        }
    }

    /// Check if mDNS discovery is enabled
    pub fn is_mdns_enabled(&self) -> bool {
        self.enable_mdns
    }

    /// Enable or disable mDNS discovery
    pub fn set_mdns_enabled(&mut self, enabled: bool) {
        self.enable_mdns = enabled;
    }
}

impl Default for DiscoveryManager {
    fn default() -> Self {
        Self::new(PeerId::random())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bootstrap_peers() {
        let dm = DiscoveryManager::new(PeerId::random());
        
        dm.add_bootstrap_peer(BootstrapPeer {
            address: "/ip4/127.0.0.1/tcp/30333".to_string(),
            peer_id: None,
        });
        assert_eq!(dm.get_bootstrap_addresses().len(), 1);
    }

    #[test]
    fn test_known_peers() {
        let dm = DiscoveryManager::new(PeerId::random());
        
        // TODO: Implement add_known_peer and related methods
        // dm.add_known_peer("QmPeer1".to_string());
        // dm.add_known_peer("QmPeer2".to_string());
        // assert_eq!(dm.get_known_peers().len(), 2);
        
        // dm.clear_known_peers();
        // assert_eq!(dm.get_known_peers().len(), 0);
    }

    #[test]
    fn test_multiaddr_parsing() {
        let addr = "/ip4/127.0.0.1/tcp/30333/p2p/QmAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let peer_id = DiscoveryManager::parse_multiaddr(addr);
        assert_eq!(peer_id, Some("QmAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_string()));
    }

    #[test]
    fn test_mdns_control() {
        let mut dm = DiscoveryManager::new(PeerId::random());
        assert!(dm.is_mdns_enabled());
        
        dm.set_mdns_enabled(false);
        assert!(!dm.is_mdns_enabled());
    }
}
