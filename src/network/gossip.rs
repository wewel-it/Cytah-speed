use std::sync::Arc;
use parking_lot::RwLock;
use tokio::sync::mpsc;
use libp2p::PeerId;
use crate::core::{Block, Transaction};
use crate::network::message::NetworkMessage;
use crate::network::PeerManager;

/// Gossip protocol for broadcasting blocks and transactions
pub struct GossipProtocol {
    /// Peer manager reference
    peer_manager: Arc<PeerManager>,
    /// Channel for sending network messages
    message_sender: mpsc::UnboundedSender<(PeerId, NetworkMessage)>,
    /// Set of recently seen block hashes to prevent duplicate broadcasts
    seen_blocks: Arc<RwLock<std::collections::HashSet<[u8; 32]>>>,
    /// Set of recently seen transaction hashes
    seen_transactions: Arc<RwLock<std::collections::HashSet<[u8; 32]>>>,
}

impl GossipProtocol {
    /// Create a new gossip protocol
    pub fn new(
        peer_manager: Arc<PeerManager>,
        message_sender: mpsc::UnboundedSender<(PeerId, NetworkMessage)>,
    ) -> Self {
        Self {
            peer_manager,
            message_sender,
            seen_blocks: Arc::new(RwLock::new(std::collections::HashSet::new())),
            seen_transactions: Arc::new(RwLock::new(std::collections::HashSet::new())),
        }
    }

    /// Broadcast a new block to all connected peers
    pub async fn broadcast_block(&self, block: Block) -> Result<(), String> {
        let block_hash = block.hash;

        // Check if we've already seen this block
        {
            let mut seen = self.seen_blocks.write();
            if seen.contains(&block_hash) {
                return Ok(()); // Already broadcasted
            }
            seen.insert(block_hash);
        }

        let message = NetworkMessage::NewBlock(block);
        self.broadcast_message(message).await
    }

    /// Broadcast a new transaction to all connected peers
    pub async fn broadcast_transaction(&self, transaction: Transaction) -> Result<(), String> {
        let tx_hash = transaction.hash();

        // Check if we've already seen this transaction
        {
            let mut seen = self.seen_transactions.write();
            if seen.contains(&tx_hash) {
                return Ok(()); // Already broadcasted
            }
            seen.insert(tx_hash);
        }

        let message = NetworkMessage::NewTransaction(transaction);
        self.broadcast_message(message).await
    }

    /// Broadcast a message to all connected peers
    async fn broadcast_message(&self, message: NetworkMessage) -> Result<(), String> {
        let peers = self.peer_manager.get_connected_peers();

        if peers.is_empty() {
            tracing::debug!("No connected peers to broadcast to");
            return Ok(());
        }

        let mut sent_count = 0;
        for peer in peers {
            if let Err(e) = self.message_sender.send((peer, message.clone())) {
                tracing::error!("Failed to send message to peer: {}", e);
            } else {
                sent_count += 1;
            }
        }

        tracing::debug!("Broadcasted message to {} peers", sent_count);
        Ok(())
    }

    /// Handle incoming gossip message
    pub async fn handle_gossip_message(&self, message: NetworkMessage) -> Result<(), String> {
        match message {
            NetworkMessage::NewBlock(block) => {
                let block_hash = block.hash;
                {
                    let mut seen = self.seen_blocks.write();
                    if seen.contains(&block_hash) {
                        return Ok(()); // Already seen
                    }
                    seen.insert(block_hash);
                }
                // Re-broadcast to other peers
                self.broadcast_message(NetworkMessage::NewBlock(block)).await?;
            }
            NetworkMessage::NewTransaction(tx) => {
                let tx_hash = tx.hash();
                {
                    let mut seen = self.seen_transactions.write();
                    if seen.contains(&tx_hash) {
                        return Ok(()); // Already seen
                    }
                    seen.insert(tx_hash);
                }
                // Re-broadcast to other peers
                self.broadcast_message(NetworkMessage::NewTransaction(tx)).await?;
            }
            _ => {
                // Other message types are handled elsewhere
            }
        }

        Ok(())
    }

    /// Clean up old seen messages periodically
    pub async fn cleanup_seen_messages(&self) {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(300)); // 5 minutes

        loop {
            interval.tick().await;

            let mut seen_blocks = self.seen_blocks.write();
            let mut seen_txs = self.seen_transactions.write();

            // Keep only recent messages (simple implementation - clear all periodically)
            // In a real implementation, you'd use timestamps and keep recent ones
            seen_blocks.clear();
            seen_txs.clear();

            tracing::debug!("Cleaned up seen message caches");
        }
    }
}