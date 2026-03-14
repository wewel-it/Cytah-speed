use std::sync::Arc;
use crate::events::event_types::Event;
use crate::events::event_bus::EventBus;

/// Event listener for subscribing to blockchain events
pub struct EventListener {
    event_bus: Arc<EventBus>,
    node_id: String,
}

impl EventListener {
    /// Create a new event listener
    pub fn new(event_bus: Arc<EventBus>, node_id: String) -> Self {
        EventListener { event_bus, node_id }
    }

    /// Subscribe to new block events
    pub async fn subscribe_new_blocks<F, Fut>(&self, callback: F) -> EventSubscription
    where
        F: Fn(Event) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let mut receiver = self.event_bus.subscribe("new_block").await;
        let node_id = self.node_id.clone();

        tokio::spawn(async move {
            while let Ok(event) = receiver.recv().await {
                callback(event).await;
            }
        });

        EventSubscription {
            event_type: "new_block".to_string(),
            node_id,
        }
    }

    /// Subscribe to new transaction events
    pub async fn subscribe_transactions<F, Fut>(&self, callback: F) -> EventSubscription
    where
        F: Fn(Event) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let mut receiver = self.event_bus.subscribe("new_transaction").await;
        let node_id = self.node_id.clone();

        tokio::spawn(async move {
            while let Ok(event) = receiver.recv().await {
                callback(event).await;
            }
        });

        EventSubscription {
            event_type: "new_transaction".to_string(),
            node_id,
        }
    }

    /// Subscribe to contract events
    pub async fn subscribe_contract_events<F, Fut>(&self, callback: F) -> EventSubscription
    where
        F: Fn(Event) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let mut receiver = self.event_bus.subscribe("contract_event").await;
        let node_id = self.node_id.clone();

        tokio::spawn(async move {
            while let Ok(event) = receiver.recv().await {
                callback(event).await;
            }
        });

        EventSubscription {
            event_type: "contract_event".to_string(),
            node_id,
        }
    }

    /// Subscribe to peer connection events
    pub async fn subscribe_peer_events<F, Fut>(&self, callback: F) -> EventSubscription
    where
        F: Fn(Event) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let mut receiver_connected = self.event_bus.subscribe("peer_connected").await;
        let mut receiver_disconnected = self.event_bus.subscribe("peer_disconnected").await;
        let node_id = self.node_id.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    event = receiver_connected.recv() => {
                        if let Ok(event) = event {
                            callback(event).await;
                        }
                    }
                    event = receiver_disconnected.recv() => {
                        if let Ok(event) = event {
                            callback(event).await;
                        }
                    }
                }
            }
        });

        EventSubscription {
            event_type: "peer_events".to_string(),
            node_id,
        }
    }

    /// Subscribe to all events
    pub async fn subscribe_all<F, Fut>(&self, callback: F) -> EventSubscription
    where
        F: Fn(Event) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let mut receiver = self.event_bus.subscribe_all().await;
        let node_id = self.node_id.clone();

        tokio::spawn(async move {
            while let Ok(event) = receiver.recv().await {
                callback(event).await;
            }
        });

        EventSubscription {
            event_type: "all".to_string(),
            node_id,
        }
    }

    /// Get subscriber count for an event type
    pub async fn subscriber_count(&self, event_type: &str) -> usize {
        self.event_bus.subscriber_count(event_type).await
    }
}

/// Represents an active event subscription
pub struct EventSubscription {
    pub event_type: String,
    pub node_id: String,
}

impl EventSubscription {
    /// Get the event type
    pub fn event_type(&self) -> &str {
        &self.event_type
    }

    /// Get the node ID
    pub fn node_id(&self) -> &str {
        &self.node_id
    }
}

/// High-level event handler trait for easier event processing
#[async_trait::async_trait]
pub trait EventHandler: Send + Sync {
    async fn on_new_block(&self, event: Event);
    async fn on_new_transaction(&self, event: Event);
    async fn on_contract_event(&self, event: Event);
    async fn on_peer_event(&self, event: Event);
}

/// Convenience function to create an event listener from an event handler
pub async fn create_listener_from_handler<H>(
    event_bus: Arc<EventBus>,
    node_id: String,
    handler: H,
) -> Vec<EventSubscription>
where
    H: EventHandler + Clone + 'static,
{
    let listener = EventListener::new(event_bus, node_id);

    let handler_clone1 = handler.clone();
    let sub1 = listener
        .subscribe_new_blocks(move |event| {
            let handler = handler_clone1.clone();
            async move { handler.on_new_block(event).await }
        })
        .await;

    let handler_clone2 = handler.clone();
    let sub2 = listener
        .subscribe_transactions(move |event| {
            let handler = handler_clone2.clone();
            async move { handler.on_new_transaction(event).await }
        })
        .await;

    let handler_clone3 = handler.clone();
    let sub3 = listener
        .subscribe_contract_events(move |event| {
            let handler = handler_clone3.clone();
            async move { handler.on_contract_event(event).await }
        })
        .await;

    let sub4 = listener
        .subscribe_peer_events(move |event| {
            let handler = handler.clone();
            async move { handler.on_peer_event(event).await }
        })
        .await;

    vec![sub1, sub2, sub3, sub4]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[derive(Clone)]
    struct TestHandler {
        block_count: Arc<AtomicUsize>,
        tx_count: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl EventHandler for TestHandler {
        async fn on_new_block(&self, _event: Event) {
            self.block_count.fetch_add(1, Ordering::SeqCst);
        }

        async fn on_new_transaction(&self, _event: Event) {
            self.tx_count.fetch_add(1, Ordering::SeqCst);
        }

        async fn on_contract_event(&self, _event: Event) {}

        async fn on_peer_event(&self, _event: Event) {}
    }

    #[tokio::test]
    async fn test_event_listener_with_handler() {
        let event_bus = Arc::new(EventBus::new());
        let handler = TestHandler {
            block_count: Arc::new(AtomicUsize::new(0)),
            tx_count: Arc::new(AtomicUsize::new(0)),
        };

        let _subscriptions = create_listener_from_handler(
            event_bus.clone(),
            "test_node".to_string(),
            handler.clone(),
        )
        .await;

        // Give some time for subscriptions to be set up
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // Publish events
        let block_event = Event::new_block(
            crate::core::Block::default(),
            1,
            "test_node".to_string(),
        );
        let tx_event = Event::new_transaction(
            crate::core::Transaction::default(),
            crate::events::event_types::TransactionStatus::Pending,
            "test_node".to_string(),
        );

        event_bus.publish(block_event).await;
        event_bus.publish(tx_event).await;

        // Give time for events to be processed
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Check that events were handled
        assert!(handler.block_count.load(Ordering::SeqCst) > 0);
        assert!(handler.tx_count.load(Ordering::SeqCst) > 0);
    }
}