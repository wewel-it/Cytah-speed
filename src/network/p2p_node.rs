use std::sync::Arc;
use parking_lot::RwLock;
use tokio::sync::mpsc;
use libp2p::{
    identity,
    PeerId,
    swarm::{NetworkBehaviour, Swarm, SwarmEvent},
    mdns,
    noise,
    yamux,
    tcp,
    Transport,
    core::upgrade,
};
use futures::StreamExt;
use crate::core::{Block, Transaction};
use crate::dag::blockdag::BlockDAG;
use crate::state::state_manager::StateManager;
use crate::network::{
    PeerManager, GossipProtocol, SyncManager, NetworkMessage
};

/// Network behaviour combining all protocols
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent")]
pub struct Behaviour {
    floodsub: libp2p::floodsub::Floodsub,
    mdns: mdns::tokio::Behaviour,
}

/// Events from the network behaviour
#[derive(Debug)]
pub enum OutEvent {
    Floodsub(libp2p::floodsub::FloodsubEvent),
    Mdns(mdns::Event),
}

impl From<libp2p::floodsub::FloodsubEvent> for OutEvent {
    fn from(event: libp2p::floodsub::FloodsubEvent) -> Self {
        OutEvent::Floodsub(event)
    }
}

impl From<mdns::Event> for OutEvent {
    fn from(event: mdns::Event) -> Self {
        OutEvent::Mdns(event)
    }
}

/// Main P2P node
pub struct P2PNode {
    /// Swarm for network communication
    swarm: Swarm<Behaviour>,
    /// Peer manager
    peer_manager: Arc<PeerManager>,
    /// Gossip protocol
    gossip: Arc<GossipProtocol>,
    /// Sync manager
    sync_manager: Arc<SyncManager>,
    /// Channel receiver for outgoing messages
    message_receiver: mpsc::UnboundedReceiver<(PeerId, NetworkMessage)>,
}
// The underlying libp2p `Swarm` type is not Send because it contains
// non-Send handles internally. In our design all access to the swarm is
// serialized via `&mut self` methods, and when used across threads we wrap
// the entire `P2PNode` instance in a `tokio::sync::RwLock`. Thus it is safe
// to mark the struct as Send/Sync manually.

// SAFETY: callers must ensure that all mutable access to the internal swarm
// occurs while holding the async lock. This pattern is followed throughout
// the codebase (e.g. `RpcState` wraps the node in an `Arc<RwLock<...>>`).
unsafe impl Send for P2PNode {}
unsafe impl Sync for P2PNode {}impl P2PNode {
    /// Create a new P2P node
    pub async fn new(
        dag: Arc<RwLock<BlockDAG>>,
        state: Arc<parking_lot::Mutex<StateManager>>,
        listen_addr: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Generate a random keypair
        let local_key = identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(local_key.public());

        tracing::info!("Local peer id: {}", local_peer_id);

        // Create transport
        let transport = tcp::tokio::Transport::new(tcp::Config::default())
            .upgrade(upgrade::Version::V1)
            .authenticate(noise::Config::new(&local_key).unwrap())
            .multiplex(yamux::Config::default())
            .boxed();

        // Create message channels
        let (message_sender, message_receiver) = mpsc::unbounded_channel();

        // Create peer manager
        let peer_manager = Arc::new(PeerManager::new(local_peer_id.clone(), 1000));

        // Create gossip protocol
        let gossip = Arc::new(GossipProtocol::new(
            peer_manager.clone(),
            message_sender.clone(),
        ));

        // Create sync manager
        let sync_manager = Arc::new(SyncManager::new(
            dag,
            peer_manager.clone(),
            message_sender.clone(),
            state.clone(),
        ));

        // Create floodsub for gossip
        let floodsub_topic = libp2p::floodsub::Topic::new("cytah-blocks");
        let mut floodsub = libp2p::floodsub::Floodsub::new(local_peer_id.clone());
        floodsub.subscribe(floodsub_topic.clone());

        // Create mDNS for peer discovery
        let mdns = mdns::tokio::Behaviour::new(mdns::Config::default(), local_peer_id.clone())?;

        // Create behaviour
        let behaviour = Behaviour {
            floodsub,
            mdns,
        };

        // Create swarm
        let mut swarm = Swarm::new(
            transport,
            behaviour,
            local_peer_id.clone(),
            libp2p::swarm::Config::with_tokio_executor(),
        );

        // Listen on the specified address
        swarm.listen_on(listen_addr.parse()?)?;

        Ok(Self {
            swarm,
            peer_manager,
            gossip,
            sync_manager,
            message_receiver,
        })
    }

    /// Start the P2P node
    pub async fn run(mut self) -> Result<(), Box<dyn std::error::Error>> {
        tracing::info!("P2P node starting...");

        // Start background tasks
        let peer_manager = self.peer_manager.clone();
        tokio::spawn(async move {
            peer_manager.cleanup_stale_peers().await;
        });

        let gossip = self.gossip.clone();
        tokio::spawn(async move {
            gossip.cleanup_seen_messages().await;
        });

        let sync_manager = self.sync_manager.clone();
        tokio::spawn(async move {
            sync_manager.periodic_sync_check().await;
        });

        // Start initial sync
        self.sync_manager.start_sync().await?;

        // Main event loop
        loop {
            tokio::select! {
                event = self.swarm.select_next_some() => {
                    self.handle_swarm_event(event).await;
                }
                message = self.message_receiver.recv() => {
                    if let Some((peer, msg)) = message {
                        self.send_message_to_peer(peer, msg).await;
                    }
                }
            }
        }
    }

    /// Handle swarm events
    async fn handle_swarm_event(&mut self, event: SwarmEvent<OutEvent>) {
        match event {
            SwarmEvent::NewListenAddr { address, .. } => {
                tracing::info!("Listening on {}", address);
            }
            SwarmEvent::Behaviour(event) => {
                self.handle_behaviour_event(event).await;
            }
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                tracing::info!("Connected to peer: {}", peer_id);
                self.peer_manager.update_peer_status(&peer_id, true);
            }
            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                tracing::info!("Disconnected from peer: {}", peer_id);
                self.peer_manager.update_peer_status(&peer_id, false);
            }
            _ => {}
        }
    }

    /// Handle behaviour events
    async fn handle_behaviour_event(&mut self, event: OutEvent) {
        match event {
            OutEvent::Floodsub(libp2p::floodsub::FloodsubEvent::Message(msg)) => {
                if let Ok(network_msg) = serde_json::from_slice::<NetworkMessage>(&msg.data) {
                    // `msg.source` is a PeerId identifying the sender
                    let peer = msg.source;
                    self.handle_network_message(peer, network_msg).await;
                }
            }
            OutEvent::Floodsub(_) => {} // Ignore other floodsub events
            OutEvent::Mdns(mdns::Event::Discovered(list)) => {
                for (peer_id, _addr) in list {
                    tracing::info!("Discovered peer: {}", peer_id);
                    if let Err(e) = self.peer_manager.add_peer(peer_id, "".to_string()) {
                        tracing::debug!("Failed to add discovered peer: {}", e);
                    }
                }
            }
            OutEvent::Mdns(mdns::Event::Expired(list)) => {
                for (peer_id, _addr) in list {
                    tracing::info!("Peer expired: {}", peer_id);
                    self.peer_manager.remove_peer(&peer_id);
                }
            }
        }
    }

    /// Handle incoming network messages
    async fn handle_network_message(&self, peer: PeerId, message: NetworkMessage) {
        match message {
            NetworkMessage::NewBlock(block) => {
                // Handle new block via gossip
                if let Err(e) = self.gossip.handle_gossip_message(NetworkMessage::NewBlock(block)).await {
                    tracing::error!("Failed to handle new block: {}", e);
                }
            }
            NetworkMessage::NewTransaction(tx) => {
                // Handle new transaction via gossip
                if let Err(e) = self.gossip.handle_gossip_message(NetworkMessage::NewTransaction(tx)).await {
                    tracing::error!("Failed to handle new transaction: {}", e);
                }
            }
            NetworkMessage::RequestBlock(hash) => {
                // Handle block request (would need to implement block lookup)
                tracing::debug!("Received block request for {:?}", hash);
            }
            NetworkMessage::RequestDag => {
                // Handle DAG request
                if let Err(e) = self.sync_manager.handle_dag_response(vec![]).await {
                    tracing::error!("Failed to handle DAG request: {}", e);
                }
            }
            NetworkMessage::DagResponse(blocks) => {
                // Handle DAG response
                if let Err(e) = self.sync_manager.handle_dag_response(blocks).await {
                    tracing::error!("Failed to handle DAG response: {}", e);
                }
            }
            NetworkMessage::GetHeaders { from, max } => {
                let _ = self.sync_manager.handle_message(peer.clone(), NetworkMessage::GetHeaders { from, max }).await;
            }
            NetworkMessage::Headers(headers) => {
                // hand off to sync manager for processing
                let _ = self.sync_manager.handle_headers(headers).await;
            }
            NetworkMessage::GetBlocks(hashes) => {
                let _ = self.sync_manager.handle_message(peer.clone(), NetworkMessage::GetBlocks(hashes)).await;
            }
            NetworkMessage::Blocks(blocks) => {
                for blk in blocks {
                    let _ = self.sync_manager.handle_block_response(Some(blk)).await;
                }
            }
            NetworkMessage::RequestState => {
                let _ = self.sync_manager.handle_message(peer.clone(), NetworkMessage::RequestState).await;
            }
            NetworkMessage::StateSnapshot(data) => {
                // deserialize and verify
                if let Ok(state) = bincode::deserialize::<crate::state::state_manager::StateManager>(&data) {
                    let root = state.get_state_root();
                    let local_root = self.sync_manager.state.lock().get_state_root();
                    if root == local_root {
                        tracing::info!("Received matching state snapshot from peer");
                    } else {
                        tracing::warn!("State snapshot root mismatch {} vs {}", hex::encode(root), hex::encode(local_root));
                    }
                }
            }
        }
    }

    /// Send a message to a specific peer
    async fn send_message_to_peer(&mut self, _peer: PeerId, message: NetworkMessage) {
        let data = serde_json::to_vec(&message).unwrap_or_default();
        let topic = libp2p::floodsub::Topic::new("cytah-blocks");

        self.swarm.behaviour_mut().floodsub.publish(topic, data);
    }

    /// Broadcast a new block
    pub async fn broadcast_block(&self, block: Block) -> Result<(), String> {
        self.gossip.broadcast_block(block).await
    }

    /// Broadcast a new transaction
    pub async fn broadcast_transaction(&self, transaction: Transaction) -> Result<(), String> {
        self.gossip.broadcast_transaction(transaction).await
    }

    /// Get connected peers
    pub fn get_connected_peers(&self) -> Vec<String> {
        self.peer_manager.get_connected_peers()
            .into_iter()
            .map(|p| p.to_string())
            .collect()
    }
}

