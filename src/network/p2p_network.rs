use std::sync::Arc;
use parking_lot::RwLock;
use std::net::SocketAddr;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use crate::core::{Transaction, Block, BlockHash};

/// Message types untuk P2P network
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NetworkMessage {
    /// Broadcast transaksi
    Transaction(Transaction),
    /// Broadcast blok
    Block(Block),
    /// Request blok tertentu
    RequestBlock(BlockHash),
    /// Response untuk block request
    ResponseBlock(Option<Block>),
    /// Ping untuk keep-alive
    Ping,
    /// Pong response
    Pong,
    /// Sync request untuk semua blocks
    SyncRequest { from_height: u64 },
    /// Response batch blocks
    SyncResponse { blocks: Vec<Block> },
    /// Get tips
    GetTips,
    /// Tips response
    TipsResponse { tips: Vec<BlockHash> },
}

/// Informasi tentang peer yang terhubung
#[derive(Clone, Debug)]
pub struct PeerInfo {
    pub address: String,
    pub last_seen: u64,
    pub is_connected: bool,
}

impl PeerInfo {
    pub fn new(address: String) -> Self {
        Self {
            address,
            last_seen: chrono::Utc::now().timestamp() as u64,
            is_connected: false,
        }
    }
}

/// P2P Network untuk komunikasi antar node
/// 
/// Fitur:
/// - Connect ke peers
/// - Broadcast messages
/// - Receive messages
/// - Track connected peers
#[derive(Clone)]
pub struct P2PNetwork {
    /// Map dari peer address ke PeerInfo
    peers: Arc<RwLock<HashMap<String, PeerInfo>>>,
    /// Queue untuk received messages
    pub received_messages: Arc<RwLock<Vec<NetworkMessage>>>,
    /// Local address untuk listen
    pub local_addr: String,
    /// Max peers
    max_peers: usize,
}

impl P2PNetwork {
    /// Buat network baru
    pub fn new(local_addr: String, max_peers: usize) -> Self {
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
            received_messages: Arc::new(RwLock::new(Vec::new())),
            local_addr,
            max_peers,
        }
    }

    /// Start listening untuk incoming connections (simplified - tidak actual listening)
    pub async fn start_node(&self) -> Result<(), String> {
        tracing::info!(
            "P2P Network started on {}",
            self.local_addr
        );
        Ok(())
    }

    /// Connect ke peer lain
    pub async fn connect_peer(&self, peer_addr: String) -> Result<(), String> {
        let mut peers = self.peers.write();

        if peers.len() >= self.max_peers {
            return Err("Max peers reached".to_string());
        }

        if peers.contains_key(&peer_addr) {
            return Err("Already connected to this peer".to_string());
        }

        let mut peer_info = PeerInfo::new(peer_addr.clone());
        peer_info.is_connected = true;
        peers.insert(peer_addr.clone(), peer_info);

        tracing::info!("Connected to peer: {}", peer_addr);
        Ok(())
    }

    /// Disconnect dari peer
    pub fn disconnect_peer(&self, peer_addr: &str) -> Result<(), String> {
        let mut peers = self.peers.write();

        if let Some(peer) = peers.get_mut(peer_addr) {
            peer.is_connected = false;
            tracing::info!("Disconnected from peer: {}", peer_addr);
            Ok(())
        } else {
            Err("Peer not found".to_string())
        }
    }

    /// Broadcast message ke semua connected peers
    pub async fn broadcast_message(&self, message: NetworkMessage) -> Result<(), String> {
        let peers = self.peers.read();
        let connected_peers: Vec<_> = peers
            .values()
            .filter(|p| p.is_connected)
            .cloned()
            .collect();
        drop(peers);

        if connected_peers.is_empty() {
            return Ok(());
        }

        // Simulate sending message oleh menambahkan ke queue
        // Dalam implementasi real, ini akan send melalui QUIC streams
        let mut messages = self.received_messages.write();
        messages.push(message.clone());

        tracing::debug!(
            "Broadcast message to {} peers",
            connected_peers.len()
        );

        Ok(())
    }

    /// Broadcast transaksi
    pub async fn broadcast_transaction(&self, tx: Transaction) -> Result<(), String> {
        self.broadcast_message(NetworkMessage::Transaction(tx)).await
    }

    /// Broadcast blok
    pub async fn broadcast_block(&self, block: Block) -> Result<(), String> {
        self.broadcast_message(NetworkMessage::Block(block)).await
    }

    /// Receive dan process message
    pub fn receive_message(&self) -> Option<NetworkMessage> {
        let mut messages = self.received_messages.write();
        messages.pop()
    }

    /// Get semua received messages dan clear
    pub fn get_all_messages(&self) -> Vec<NetworkMessage> {
        self.received_messages.write().drain(..).collect()
    }

    /// Dapatkan list connected peers
    pub fn get_connected_peers(&self) -> Vec<String> {
        self.peers
            .read()
            .values()
            .filter(|p| p.is_connected)
            .map(|p| p.address.clone())
            .collect()
    }

    /// Dapatkan semua peers (connected dan disconnected)
    pub fn get_all_peers(&self) -> Vec<String> {
        self.peers.read().keys().cloned().collect()
    }

    /// Check apakah terhubung ke peer
    pub fn is_connected_to(&self, peer_addr: &str) -> bool {
        self.peers
            .read()
            .get(peer_addr)
            .map(|p| p.is_connected)
            .unwrap_or(false)
    }

    /// Update last seen peer
    pub fn update_peer_activity(&self, peer_addr: &str) {
        let mut peers = self.peers.write();
        if let Some(peer) = peers.get_mut(peer_addr) {
            peer.last_seen = chrono::Utc::now().timestamp() as u64;
        }
    }

    /// Check dan remove inactive peers (tidak dilihat selama > timeout secs)
    pub fn prune_inactive_peers(&self, timeout_secs: u64) {
        let now = chrono::Utc::now().timestamp() as u64;
        let mut peers = self.peers.write();

        peers.retain(|_, peer| {
            if !peer.is_connected && (now - peer.last_seen) > timeout_secs {
                tracing::debug!("Removing inactive peer: {}", peer.address);
                false
            } else {
                true
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_p2p_network_creation() {
        let network = P2PNetwork::new("127.0.0.1:8000".to_string(), 10);
        assert_eq!(network.local_addr, "127.0.0.1:8000");
    }

    #[tokio::test]
    async fn test_start_node() {
        let network = P2PNetwork::new("127.0.0.1:8001".to_string(), 10);
        let result = network.start_node().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_connect_peer() {
        let network = P2PNetwork::new("127.0.0.1:8002".to_string(), 10);
        let result = network.connect_peer("127.0.0.1:8003".to_string()).await;
        assert!(result.is_ok());
        assert!(network.is_connected_to("127.0.0.1:8003"));
    }

    #[tokio::test]
    async fn test_disconnect_peer() {
        let network = P2PNetwork::new("127.0.0.1:8004".to_string(), 10);
        network.connect_peer("127.0.0.1:8005".to_string()).await.ok();
        assert!(network.is_connected_to("127.0.0.1:8005"));

        let result = network.disconnect_peer("127.0.0.1:8005");
        assert!(result.is_ok());
        assert!(!network.is_connected_to("127.0.0.1:8005"));
    }

    #[tokio::test]
    async fn test_max_peers() {
        let network = P2PNetwork::new("127.0.0.1:8006".to_string(), 2);
        network.connect_peer("127.0.0.1:8007".to_string()).await.ok();
        network.connect_peer("127.0.0.1:8008".to_string()).await.ok();

        let result = network.connect_peer("127.0.0.1:8009".to_string()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_broadcast_message() {
        let network = P2PNetwork::new("127.0.0.1:8010".to_string(), 10);
        network.connect_peer("127.0.0.1:8011".to_string()).await.ok();

        let message = NetworkMessage::Ping;
        let result = network.broadcast_message(message).await;
        assert!(result.is_ok());

        let received = network.receive_message();
        assert!(received.is_some());
    }

    #[tokio::test]
    async fn test_get_connected_peers() {
        let network = P2PNetwork::new("127.0.0.1:8012".to_string(), 10);
        network.connect_peer("127.0.0.1:8013".to_string()).await.ok();
        network.connect_peer("127.0.0.1:8014".to_string()).await.ok();

        let connected = network.get_connected_peers();
        assert_eq!(connected.len(), 2);
    }

    #[tokio::test]
    async fn test_prune_inactive_peers() {
        let network = P2PNetwork::new("127.0.0.1:8015".to_string(), 10);
        network.connect_peer("127.0.0.1:8016".to_string()).await.ok();

        network.disconnect_peer("127.0.0.1:8016").ok();

        // Artificially set last_seen to old value untuk testing
        let mut peers = network.peers.write();
        if let Some(peer) = peers.get_mut("127.0.0.1:8016") {
            peer.last_seen = 0;
        }
        drop(peers);

        assert_eq!(network.get_all_peers().len(), 1);
        network.prune_inactive_peers(100);
        assert_eq!(network.get_all_peers().len(), 0);
    }
}
