use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    response::IntoResponse,
    routing::get,
    Router,
};
use futures::{sink::SinkExt, stream::StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::cors::CorsLayer;

use crate::events::{EventBus, Event};

/// WebSocket RPC server for real-time blockchain events
pub struct WebSocketServer {
    event_bus: Arc<EventBus>,
}

impl WebSocketServer {
    /// Create a new WebSocket server
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        WebSocketServer { event_bus }
    }

    /// Create the Axum router for WebSocket endpoints
    pub fn router(self) -> Router {
        let state = Arc::new(self);

        Router::new()
            .route("/events", get(ws_events_handler))
            .route("/blocks", get(ws_blocks_handler))
            .route("/transactions", get(ws_transactions_handler))
            .layer(CorsLayer::permissive())
            .with_state(state)
    }

    /// Start the WebSocket server
    pub async fn serve(self, addr: std::net::SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
        let app = self.router();
        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!("WebSocket server listening on {}", addr);

        axum::serve(listener, app).await?;
        Ok(())
    }
}

/// WebSocket message types
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum WsMessage {
    /// Subscribe to events
    Subscribe { event_types: Vec<String> },
    /// Unsubscribe from events
    Unsubscribe { event_types: Vec<String> },
    /// Ping message
    Ping,
    /// Pong response
    Pong,
    /// Event data
    Event(Event),
    /// Error message
    Error { message: String },
}

/// Handle WebSocket connections for general events
async fn ws_events_handler(
    ws: WebSocketUpgrade,
    State(server): State<Arc<WebSocketServer>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_events_socket(socket, server))
}

/// Handle WebSocket connections for block events
async fn ws_blocks_handler(
    ws: WebSocketUpgrade,
    State(server): State<Arc<WebSocketServer>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_blocks_socket(socket, server))
}

/// Handle WebSocket connections for transaction events
async fn ws_transactions_handler(
    ws: WebSocketUpgrade,
    State(server): State<Arc<WebSocketServer>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_transactions_socket(socket, server))
}

/// Handle general events WebSocket connection
async fn handle_events_socket(socket: WebSocket, server: Arc<WebSocketServer>) {
    let (mut sender, mut receiver) = socket.split();

    // Subscribe to all events by default
    let mut event_receiver = server.event_bus.subscribe_all().await;

    tokio::spawn(async move {
        loop {
            tokio::select! {
                // Handle incoming WebSocket messages
                message = receiver.next() => {
                    match message {
                        Some(Ok(Message::Text(text))) => {
                            if let Ok(ws_msg) = serde_json::from_str::<WsMessage>(&text) {
                                match ws_msg {
                                    WsMessage::Ping => {
                                        let pong = WsMessage::Pong;
                                        if let Ok(json) = serde_json::to_string(&pong) {
                                            let _ = sender.send(Message::Text(json)).await;
                                        }
                                    }
                                    WsMessage::Subscribe { event_types } => {
                                        // For general events endpoint, we already subscribe to all
                                        // In a more sophisticated implementation, we could manage
                                        // multiple subscriptions per connection
                                        tracing::debug!("Client subscribed to: {:?}", event_types);
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Some(Ok(Message::Close(_))) => break,
                        Some(Err(_)) => break,
                        _ => {}
                    }
                }

                // Handle blockchain events
                event = event_receiver.recv() => {
                    match event {
                        Ok(event) => {
                            let ws_msg = WsMessage::Event(event);
                            if let Ok(json) = serde_json::to_string(&ws_msg) {
                                if sender.send(Message::Text(json)).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
        }
    });
}

/// Handle blocks WebSocket connection
/// Handle blocks WebSocket connection
async fn handle_blocks_socket(socket: WebSocket, server: Arc<WebSocketServer>) {
    let (mut sender, mut receiver) = socket.split();

    let mut event_receiver = server.event_bus.subscribe("new_block").await;

    tokio::spawn(async move {
        loop {
            tokio::select! {
                message = receiver.next() => {
                    match message {
                        Some(Ok(Message::Text(text))) => {
                            if let Ok(ws_msg) = serde_json::from_str::<WsMessage>(&text) {
                                match ws_msg {
                                    WsMessage::Ping => {
                                        let pong = WsMessage::Pong;
                                        if let Ok(json) = serde_json::to_string(&pong) {
                                            let _ = sender.send(Message::Text(json)).await;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Some(Ok(Message::Close(_))) => break,
                        Some(Err(_)) => break,
                        _ => {}
                    }
                }

                event = event_receiver.recv() => {
                    match event {
                        Ok(event) => {
                            let ws_msg = WsMessage::Event(event);
                            if let Ok(json) = serde_json::to_string(&ws_msg) {
                                if sender.send(Message::Text(json)).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
        }
    });
}

/// Handle transactions WebSocket connection
async fn handle_transactions_socket(socket: WebSocket, server: Arc<WebSocketServer>) {
    let (mut sender, mut receiver) = socket.split();

    let mut event_receiver = server.event_bus.subscribe("new_transaction").await;

    tokio::spawn(async move {
        loop {
            tokio::select! {
                message = receiver.next() => {
                    match message {
                        Some(Ok(Message::Text(text))) => {
                            if let Ok(ws_msg) = serde_json::from_str::<WsMessage>(&text) {
                                match ws_msg {
                                    WsMessage::Ping => {
                                        let pong = WsMessage::Pong;
                                        if let Ok(json) = serde_json::to_string(&pong) {
                                            let _ = sender.send(Message::Text(json)).await;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Some(Ok(Message::Close(_))) => break,
                        Some(Err(_)) => break,
                        _ => {}
                    }
                }

                event = event_receiver.recv() => {
                    match event {
                        Ok(event) => {
                            let ws_msg = WsMessage::Event(event);
                            if let Ok(json) = serde_json::to_string(&ws_msg) {
                                if sender.send(Message::Text(json)).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    #[tokio::test]
    async fn test_websocket_server_creation() {
        let event_bus = Arc::new(EventBus::new());
        let server = WebSocketServer::new(event_bus);

        // Just test that we can create the router
        let _router = server.router();
        assert!(true);
    }

    #[tokio::test]
    async fn test_ws_message_serialization() {
        let event = Event::new_block(
            crate::core::Block::default(),
            1,
            "test_node".to_string(),
        );

        let ws_msg = WsMessage::Event(event);
        let json = serde_json::to_string(&ws_msg).unwrap();
        let deserialized: WsMessage = serde_json::from_str(&json).unwrap();

        match deserialized {
            WsMessage::Event(_) => assert!(true),
            _ => panic!("Wrong message type"),
        }
    }
}