use std::fmt::Write;

use futures::{SinkExt, StreamExt};
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use tokio_tungstenite::tungstenite::Message as WsMessageType;
use tokio_tungstenite::connect_async;
use url::Url;

use cytah_core::{Block, Transaction, Address, events::Event};
use crate::errors::SdkError;

/// WebSocket traffic message format from the node
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
enum WsMessage {
    Ping,
    Pong,
    Event(Event),
    Error { message: String },
}

/// Simple wrapper around the node RPC API.
///
/// Example:
/// ```no_run
/// use cytah_core::sdk::Client;
/// use cytah_core::core::Address;
/// 
/// #[tokio::main]
/// async fn main() {
///     let client = Client::new("http://127.0.0.1:8080");
///     let addr: Address = [0u8; 20];
///     let balance = client.get_balance(addr).await.unwrap();
///     println!("balance = {}", balance.balance);
/// }
/// ```
#[derive(Clone, Debug)]
pub struct Client {
    base_url: String,
    http: HttpClient,
}

impl Client {
    /// Create a new client targeting a node RPC base URL.
    pub fn new(base_url: impl Into<String>) -> Self {
        let mut base_url = base_url.into();
        // normalize base URL (remove trailing slash)
        if base_url.ends_with('/') {
            base_url.pop();
        }
        Self {
            base_url,
            http: HttpClient::new(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// Formats an address to the JSON RPC expected value (cyt + hex).
    pub fn format_address(address: Address) -> String {
        let mut s = String::with_capacity(3 + 40);
        s.push_str("cyt");
        write!(&mut s, "{}", hex::encode(address)).expect("writing to String cannot fail");
        s
    }

    /// Submit a signed transaction to the node.
    pub async fn send_transaction(&self, transaction: &Transaction) -> Result<(), SdkError> {
        let url = self.url("/send_tx");
        let request = SendTxRequest {
            transaction: transaction.clone(),
        };

        let resp = self
            .http
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| SdkError::NetworkError(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(SdkError::RpcError(format!("{}: {}", status, body)));
        }

        Ok(())
    }

    /// Query account balance and nonce from the node.
    pub async fn get_balance(&self, address: Address) -> Result<Balance, SdkError> {
        let url = self.url(&format!("/balance/{}", Self::format_address(address)));
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| SdkError::NetworkError(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(SdkError::RpcError(format!("{}", resp.status())));
        }

        let result = resp
            .json::<BalanceResponse>()
            .await
            .map_err(|e| SdkError::SerializationError(e.to_string()))?;
        Ok(Balance {
            address: result.address,
            balance: result.balance,
            nonce: result.nonce,
        })
    }

    /// Query a block by hash (32-byte array).
    pub async fn get_block(&self, hash: [u8; 32]) -> Result<Option<Block>, SdkError> {
        let hash_hex = hex::encode(hash);
        let url = self.url(&format!("/block/{}", hash_hex));
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| SdkError::NetworkError(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(SdkError::RpcError(format!("{}", resp.status())));
        }

        let result = resp
            .json::<BlockResponse>()
            .await
            .map_err(|e| SdkError::SerializationError(e.to_string()))?;
        Ok(result.block)
    }

    /// Fetch a transaction by hash.
    pub async fn get_transaction(&self, hash: [u8; 32]) -> Result<Option<Transaction>, SdkError> {
        let hash_hex = hex::encode(hash);
        let url = self.url(&format!("/tx/{}", hash_hex));
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| SdkError::NetworkError(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(SdkError::RpcError(format!("{}", resp.status())));
        }

        let result = resp
            .json::<TransactionResponse>()
            .await
            .map_err(|e| SdkError::SerializationError(e.to_string()))?;
        Ok(result.transaction)
    }

    /// Query the node's DAG info.
    pub async fn get_dag_info(&self) -> Result<DagInfo, SdkError> {
        let url = self.url("/dag");
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| SdkError::NetworkError(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(SdkError::RpcError(format!("{}", resp.status())));
        }

        let result = resp
            .json::<DagResponse>()
            .await
            .map_err(|e| SdkError::SerializationError(e.to_string()))?;
        Ok(DagInfo {
            tips: result.tips,
            total_blocks: result.total_blocks,
            stats: result.stats,
        })
    }

    /// Query the node's runtime information (peers, mempool, height).
    pub async fn get_node_info(&self) -> Result<NodeInfo, SdkError> {
        let url = self.url("/node/info");
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| SdkError::NetworkError(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(SdkError::RpcError(format!("{}", resp.status())));
        }

        let result = resp
            .json::<NodeInfo>()
            .await
            .map_err(|e| SdkError::SerializationError(e.to_string()))?;
        Ok(result)
    }

    /// Deploy a WebAssembly contract.
    pub async fn deploy_contract(
        &self,
        from: Address,
        nonce: u64,
        wasm_code: Vec<u8>,
        init_args: Option<Vec<u8>>,
    ) -> Result<DeployContractResult, SdkError> {
        let url = self.url("/contract/deploy");
        let request = DeployContractRequest {
            from: Self::format_address(from),
            nonce,
            wasm_code: hex::encode(wasm_code),
            init_args: init_args.map(|b| hex::encode(b)),
        };

        let resp = self
            .http
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| SdkError::NetworkError(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(SdkError::RpcError(format!("{}: {}", status, body)));
        }

        let result = resp
            .json::<DeployContractResponse>()
            .await
            .map_err(|e| SdkError::SerializationError(e.to_string()))?;

        Ok(DeployContractResult {
            contract_address: result.contract_address,
            tx_hash: result.tx_hash,
        })
    }

    /// Call an existing contract method.
    pub async fn call_contract(
        &self,
        from: Address,
        nonce: u64,
        contract_address: String,
        method: String,
        args: Option<Vec<u8>>,
    ) -> Result<CallContractResult, SdkError> {
        let url = self.url("/contract/call");
        let request = CallContractRequest {
            from: Self::format_address(from),
            nonce,
            contract_address,
            method,
            args: args.map(|b| hex::encode(b)),
        };

        let resp = self
            .http
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| SdkError::NetworkError(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(SdkError::RpcError(format!("{}: {}", status, body)));
        }

        let result = resp
            .json::<CallContractResponse>()
            .await
            .map_err(|e| SdkError::SerializationError(e.to_string()))?;

        Ok(CallContractResult {
            status: result.status,
            result: result.result,
        })
    }

    /// Subscribe to new block events via WebSocket
    pub async fn subscribe_new_blocks<F, Fut>(&self, callback: F) -> Result<(), SdkError>
    where
        F: Fn(Event) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        self.subscribe_ws("/blocks", callback).await
    }

    /// Subscribe to new transaction events
    pub async fn subscribe_transactions<F, Fut>(&self, callback: F) -> Result<(), SdkError>
    where
        F: Fn(Event) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        self.subscribe_ws("/transactions", callback).await
    }

    /// Subscribe to contract events
    pub async fn subscribe_contract_events<F, Fut>(&self, callback: F) -> Result<(), SdkError>
    where
        F: Fn(Event) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        self.subscribe_ws("/events", callback).await
    }

    /// Subscribe to all events
    pub async fn subscribe_all_events<F, Fut>(&self, callback: F) -> Result<(), SdkError>
    where
        F: Fn(Event) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        self.subscribe_ws("/events", callback).await
    }

    async fn subscribe_ws<F, Fut>(&self, path: &str, callback: F) -> Result<(), SdkError>
    where
        F: Fn(Event) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let mut url = Url::parse(&self.base_url)
            .map_err(|e| SdkError::NetworkError(format!("Invalid base URL: {}", e)))?;

        // Ensure we use ws(s) scheme for websocket
        let scheme = match url.scheme() {
            "http" => "ws",
            "https" => "wss",
            other => other,
        };
        url.set_scheme(scheme).map_err(|_| SdkError::NetworkError("Invalid scheme".to_string()))?;
        url.set_path(path);

        let (ws_stream, _) = connect_async(url)
            .await
            .map_err(|e| SdkError::NetworkError(format!("WebSocket connect failed: {}", e)))?;

        let (mut write, mut read) = ws_stream.split();

        // Fire off ping sequence to keep connection alive
        let _ = write.send(WsMessageType::Text("{\"type\":\"Ping\"}".to_string())).await;

        tokio::spawn(async move {
            while let Some(msg) = read.next().await {
                if let Ok(msg) = msg {
                    if let WsMessageType::Text(txt) = msg {
                        if let Ok(ws_msg) = serde_json::from_str::<WsMessage>(&txt) {
                            if let WsMessage::Event(ev) = ws_msg {
                                callback(ev).await;
                            }
                        }
                    }
                } else {
                    break;
                }
            }
        });

        Ok(())
    }

}

/// Response types used by the SDK client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Balance {
    pub address: String,
    pub balance: u64,
    pub nonce: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BalanceResponse {
    pub address: String,
    pub balance: u64,
    pub nonce: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BlockResponse {
    pub hash: String,
    pub block: Option<Block>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TransactionResponse {
    pub hash: String,
    pub transaction: Option<Transaction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagInfo {
    pub tips: Vec<String>,
    pub total_blocks: usize,
    pub stats: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DagResponse {
    pub tips: Vec<String>,
    pub total_blocks: usize,
    pub stats: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub peer_id: [u8; 32],
    pub connected_peers: Vec<[u8; 32]>,
    pub mempool_size: usize,
    pub dag_height: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SendTxRequest {
    pub transaction: Transaction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeployContractRequest {
    pub from: String,
    pub nonce: u64,
    pub wasm_code: String,
    pub init_args: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeployContractResponse {
    pub contract_address: String,
    pub tx_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployContractResult {
    pub contract_address: String,
    pub tx_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CallContractRequest {
    pub from: String,
    pub nonce: u64,
    pub contract_address: String,
    pub method: String,
    pub args: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CallContractResponse {
    pub status: String,
    pub result: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallContractResult {
    pub status: String,
    pub result: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_address_format() {
        let addr: Address = [1u8; 20];
        let s = Client::format_address(addr);
        assert_eq!(s.len(), 3 + 40);
        assert!(s.starts_with("cyt"));
    }
}
