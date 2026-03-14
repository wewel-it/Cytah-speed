use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use crate::events::event_types::Event;

/// Central event bus for publishing and subscribing to blockchain events
pub struct EventBus {
    /// Broadcast channels for different event types
    channels: Arc<RwLock<HashMap<String, broadcast::Sender<Event>>>>,
    /// Maximum capacity for broadcast channels
    capacity: usize,
}

impl EventBus {
    /// Create a new event bus
    pub fn new() -> Self {
        EventBus {
            channels: Arc::new(RwLock::new(HashMap::new())),
            capacity: 1000, // Default capacity
        }
    }

    /// Create a new event bus with custom capacity
    pub fn with_capacity(capacity: usize) -> Self {
        EventBus {
            channels: Arc::new(RwLock::new(HashMap::new())),
            capacity,
        }
    }

    /// Get or create a broadcast channel for an event type
    async fn get_or_create_channel(&self, event_type: &str) -> broadcast::Sender<Event> {
        let mut channels = self.channels.write().await;

        if let Some(sender) = channels.get(event_type) {
            sender.clone()
        } else {
            let (sender, _) = broadcast::channel(self.capacity);
            channels.insert(event_type.to_string(), sender.clone());
            sender
        }
    }

    /// Publish an event to all subscribers
    pub async fn publish(&self, event: Event) {
        let event_type = match &event.event_type {
            crate::events::event_types::EventType::NewBlock { .. } => "new_block",
            crate::events::event_types::EventType::NewTransaction { .. } => "new_transaction",
            crate::events::event_types::EventType::ContractEvent { .. } => "contract_event",
            crate::events::event_types::EventType::PeerConnected { .. } => "peer_connected",
            crate::events::event_types::EventType::PeerDisconnected { .. } => "peer_disconnected",
            crate::events::event_types::EventType::NodeStatusChanged { .. } => "node_status",
        };

        if let Ok(_) = self.get_or_create_channel(event_type).await.send(event) {
            // Event sent successfully
            tracing::debug!("Event published: {}", event_type);
        } else {
            tracing::warn!("Failed to publish event: {} (no subscribers or channel full)", event_type);
        }
    }

    /// Subscribe to events of a specific type
    pub async fn subscribe(&self, event_type: &str) -> broadcast::Receiver<Event> {
        let sender = self.get_or_create_channel(event_type).await;
        sender.subscribe()
    }

    /// Subscribe to all events
    pub async fn subscribe_all(&self) -> broadcast::Receiver<Event> {
        // Create a special "all" channel that receives all events
        let (sender, receiver) = broadcast::channel(self.capacity);

        // Spawn a task to forward all events to the "all" channel
        let channels = self.channels.clone();
        let sender_clone = sender.clone();

        tokio::spawn(async move {
            loop {
                let mut all_receivers = Vec::new();

                {
                    let channels_read = channels.read().await;
                    for (_, sender) in channels_read.iter() {
                        all_receivers.push(sender.subscribe());
                    }
                }

                // Wait for any event from any channel
                let select_all = futures::future::select_all(
                    all_receivers.into_iter().map(|mut rx| Box::pin(async move { rx.recv().await }))
                );

                if let (Ok(event), _, _) = select_all.await {
                    let _ = sender_clone.send(event);
                } else {
                    // If all channels are closed, break
                    break;
                }
            }
        });

        receiver
    }

    /// Get the number of subscribers for an event type
    pub async fn subscriber_count(&self, event_type: &str) -> usize {
        let channels = self.channels.read().await;
        channels
            .get(event_type)
            .map(|sender| sender.receiver_count())
            .unwrap_or(0)
    }

    /// Get all active event types
    pub async fn active_event_types(&self) -> Vec<String> {
        let channels = self.channels.read().await;
        channels.keys().cloned().collect()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::event_types::{Event, EventType, TransactionStatus};

    #[tokio::test]
    async fn test_event_bus_publish_subscribe() {
        let bus = EventBus::new();

        // Subscribe to new_block events
        let mut receiver = bus.subscribe("new_block").await;

        // Create and publish an event
        let event = Event::new_block(
            crate::core::Block::default(),
            1,
            "test_node".to_string(),
        );

        bus.publish(event.clone()).await;

        // Receive the event
        if let Ok(received_event) = receiver.recv().await {
            assert_eq!(received_event.id, event.id);
        } else {
            panic!("Failed to receive event");
        }
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let bus = EventBus::new();

        let mut receiver1 = bus.subscribe("new_transaction").await;
        let mut receiver2 = bus.subscribe("new_transaction").await;

        let event = Event::new_transaction(
            crate::core::Transaction::default(),
            TransactionStatus::Pending,
            "test_node".to_string(),
        );

        bus.publish(event.clone()).await;

        // Both receivers should get the event
        let event1 = receiver1.recv().await.unwrap();
        let event2 = receiver2.recv().await.unwrap();

        assert_eq!(event1.id, event.id);
        assert_eq!(event2.id, event.id);
    }
}