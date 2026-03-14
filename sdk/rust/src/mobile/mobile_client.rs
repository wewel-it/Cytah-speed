use crate::client::Client;
use crate::errors::SdkError;
use cytah_core::{Address, BlockHash, Transaction as CoreTransaction};
use serde::{Deserialize, Serialize};

/// Lightweight mobile client optimized for mobile devices
/// Uses efficient networking and minimal memory footprint
pub struct MobileClient {
    client: Client,
    cache: std::sync::Mutex<std::collections::HashMap<String, serde_json::Value>>,
}

impl MobileClient {
    /// Create a new mobile client
    pub fn new(endpoint: String) -> Self {
        MobileClient {
            client: Client::new(endpoint),
            cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Get account balance (with caching)
    pub async fn get_balance(&self, address: &Address) -> Result<crate::client::Balance, SdkError> {
        let cache_key = format!("balance_{}", hex::encode(address));

        // Check cache first
        if let Some(cached) = self.cache.lock().unwrap().get(&cache_key) {
            if let Ok(balance) = serde_json::from_value::<crate::client::Balance>(cached.clone()) {
                return Ok(balance);
            }
        }

        // Fetch from network
        let balance = self.client.get_balance(*address).await?;

        // Cache result (simple time-based expiration could be added)
        let value = serde_json::to_value(&balance).map_err(|e| SdkError::SerializationError(e.to_string()))?;
        self.cache.lock().unwrap().insert(cache_key, value);

        Ok(balance.balance.to_string())
    }

    /// Send transaction (optimized for mobile)
    pub async fn send_transaction(&self, tx: &CoreTransaction) -> Result<String, SdkError> {
        self.client.send_transaction(tx).await?;
        Ok(hex::encode(tx.hash())) // Return transaction hash
    }

    /// Get transaction status (lightweight)
    pub async fn get_transaction_status(&self, tx_hash: &str) -> Result<TransactionStatus, SdkError> {
        // In a real implementation, this would check transaction status
        // For now, return a mock status
        Ok(TransactionStatus::Confirmed)
    }

    /// Get basic node info (optimized response)
    pub async fn get_node_info(&self) -> Result<MobileNodeInfo, SdkError> {
        let info = self.client.get_node_info().await?;
        Ok(MobileNodeInfo {
            height: info.dag_height,
            peers: info.connected_peers.len(),
            status: "running".to_string(),
        })
    }

    /// Clear cache (useful for memory management on mobile)
    pub fn clear_cache(&self) {
        self.cache.lock().unwrap().clear();
    }

    /// Get cache size
    pub fn cache_size(&self) -> usize {
        self.cache.lock().unwrap().len()
    }
}

/// Mobile-optimized node information
#[derive(Debug, Serialize, Deserialize)]
pub struct MobileNodeInfo {
    pub height: usize,
    pub peers: usize,
    pub status: String,
}

/// Transaction status for mobile queries
#[derive(Debug, Serialize, Deserialize)]
pub enum TransactionStatus {
    Pending,
    Confirmed,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mobile_client_creation() {
        let client = MobileClient::new("http://localhost:8080".to_string());
        assert_eq!(client.cache_size(), 0);
    }

    #[test]
    fn test_cache_operations() {
        let client = MobileClient::new("http://localhost:8080".to_string());

        // Initially empty
        assert_eq!(client.cache_size(), 0);

        // Add to cache manually for testing
        {
            let mut cache = client.cache.lock().unwrap();
            cache.insert("test".to_string(), serde_json::Value::String("value".to_string()));
        }

        assert_eq!(client.cache_size(), 1);

        // Clear cache
        client.clear_cache();
        assert_eq!(client.cache_size(), 0);
    }
}