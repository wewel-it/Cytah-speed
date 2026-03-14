use rocksdb::{DB, IteratorMode, Options};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::collections::HashMap;
use crate::core::{Address, Transaction, BlockHash};
use crate::sdk::errors::SdkError;

/// Address indexer for efficient address-based queries
pub struct AddressIndexer {
    db: Arc<DB>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AddressInfo {
    pub address: Address,
    pub balance: String, // String to handle large numbers
    pub transaction_count: u64,
    pub first_seen: u64, // Timestamp
    pub last_seen: u64,  // Timestamp
}

impl AddressIndexer {
    /// Create a new address indexer
    pub fn new(db_path: &str) -> Result<Self, SdkError> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.set_max_open_files(10000);

        let db = DB::open(&opts, db_path)
            .map_err(|e| SdkError::TransactionError(format!("Failed to open address index DB: {}", e)))?;

        Ok(AddressIndexer { db: Arc::new(db) })
    }

    /// Index an address from a transaction
    pub fn index_address_from_transaction(&self, tx: &Transaction, timestamp: u64) -> Result<(), SdkError> {
        // Index sender
        self.update_address_info(tx.from, timestamp)?;

        // Index recipient based on transaction type
        match &tx.payload {
            crate::core::TxPayload::Transfer { to, .. } => {
                self.update_address_info(*to, timestamp)?;
            }
            crate::core::TxPayload::ContractDeploy { .. } => {
                // Contract deployment - sender is the deployer
            }
            crate::core::TxPayload::ContractCall { contract_address, .. } => {
                self.update_address_info(*contract_address, timestamp)?;
            }
        }

        Ok(())
    }

    /// Update or create address information
    fn update_address_info(&self, address: Address, timestamp: u64) -> Result<(), SdkError> {
        let addr_key = format!("addr:{}", hex::encode(address));
        let tx_count_key = format!("addr_tx_count:{}", hex::encode(address));

        // Get existing info or create new
        let mut info = match self.get_address_info(&address)? {
            Some(existing) => existing,
            None => AddressInfo {
                address,
                balance: "0".to_string(),
                transaction_count: 0,
                first_seen: timestamp,
                last_seen: timestamp,
            },
        };

        // Update transaction count
        info.transaction_count += 1;
        info.last_seen = timestamp;

        // Save updated info
        let info_data = serde_json::to_vec(&info)
            .map_err(|e| SdkError::SerializationError(e.to_string()))?;

        self.db.put(addr_key.as_bytes(), info_data)
            .map_err(|e| SdkError::TransactionError(format!("Failed to index address: {}", e)))?;

        // Update transaction count
        self.db.put(tx_count_key.as_bytes(), &info.transaction_count.to_le_bytes())
            .map_err(|e| SdkError::TransactionError(format!("Failed to update tx count: {}", e)))?;

        Ok(())
    }

    /// Get address information
    pub fn get_address_info(&self, address: &Address) -> Result<Option<AddressInfo>, SdkError> {
        let key = format!("addr:{}", hex::encode(address));
        match self.db.get(key.as_bytes())
            .map_err(|e| SdkError::TransactionError(format!("Failed to get address info: {}", e)))? {
            Some(data) => {
                let info: AddressInfo = serde_json::from_slice(&data)
                    .map_err(|e| SdkError::SerializationError(e.to_string()))?;
                Ok(Some(info))
            }
            None => Ok(None),
        }
    }

    /// Update address balance
    pub fn update_balance(&self, address: &Address, balance: &str) -> Result<(), SdkError> {
        if let Some(mut info) = self.get_address_info(address)? {
            info.balance = balance.to_string();

            let info_data = serde_json::to_vec(&info)
                .map_err(|e| SdkError::SerializationError(e.to_string()))?;

            let key = format!("addr:{}", hex::encode(address));
            self.db.put(key.as_bytes(), info_data)
                .map_err(|e| SdkError::TransactionError(format!("Failed to update balance: {}", e)))?;
        }

        Ok(())
    }

    /// Get addresses with highest transaction counts
    pub fn get_top_addresses_by_transactions(&self, limit: usize) -> Result<Vec<AddressInfo>, SdkError> {
        let mut addresses = Vec::new();

        // This is a simplified implementation - in practice, you'd want a more efficient way
        // to query top addresses, perhaps using a separate sorted index
        let iter = self.db.iterator(IteratorMode::Start);
        for item in iter {
            let (key, value) = item
                .map_err(|e| SdkError::TransactionError(format!("Iterator error: {}", e)))?;

            let key_str = String::from_utf8_lossy(&key);
            if key_str.starts_with("addr:") && !key_str.contains("_tx_count") {
                let info: AddressInfo = serde_json::from_slice(&value)
                    .map_err(|e| SdkError::SerializationError(e.to_string()))?;
                addresses.push(info);

                if addresses.len() >= limit {
                    break;
                }
            }
        }

        // Sort by transaction count (descending)
        addresses.sort_by(|a, b| b.transaction_count.cmp(&a.transaction_count));

        Ok(addresses.into_iter().take(limit).collect())
    }

    /// Get recently active addresses
    pub fn get_recent_addresses(&self, limit: usize) -> Result<Vec<AddressInfo>, SdkError> {
        let mut addresses = Vec::new();

        let iter = self.db.iterator(IteratorMode::Start);
        for item in iter {
            let (key, value) = item
                .map_err(|e| SdkError::TransactionError(format!("Iterator error: {}", e)))?;

            let key_str = String::from_utf8_lossy(&key);
            if key_str.starts_with("addr:") && !key_str.contains("_tx_count") {
                let info: AddressInfo = serde_json::from_slice(&value)
                    .map_err(|e| SdkError::SerializationError(e.to_string()))?;
                addresses.push(info);

                if addresses.len() >= limit {
                    break;
                }
            }
        }

        // Sort by last seen (descending)
        addresses.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));

        Ok(addresses.into_iter().take(limit).collect())
    }

    /// Get total number of indexed addresses
    pub fn get_address_count(&self) -> Result<u64, SdkError> {
        let mut count = 0u64;
        let prefix = b"addr:";

        let iter = self.db.iterator(IteratorMode::From(prefix, rocksdb::Direction::Forward));
        for item in iter {
            let (key, _) = item
                .map_err(|e| SdkError::TransactionError(format!("Iterator error: {}", e)))?;

            let key_str = String::from_utf8_lossy(&key);
            if key_str.starts_with("addr:") && !key_str.contains("_tx_count") {
                count += 1;
            } else if !key_str.starts_with("addr:") {
                break;
            }
        }

        Ok(count)
    }

    /// Search addresses by partial match
    pub fn search_addresses(&self, query: &str, limit: usize) -> Result<Vec<AddressInfo>, SdkError> {
        let mut results = Vec::new();
        let query_lower = query.to_lowercase();

        let iter = self.db.iterator(IteratorMode::Start);
        for item in iter {
            let (key, value) = item
                .map_err(|e| SdkError::TransactionError(format!("Iterator error: {}", e)))?;

            let key_str = String::from_utf8_lossy(&key);
            if key_str.starts_with("addr:") && !key_str.contains("_tx_count") {
                let addr_hex = &key_str[5..]; // Remove "addr:" prefix
                if addr_hex.to_lowercase().contains(&query_lower) {
                    let info: AddressInfo = serde_json::from_slice(&value)
                        .map_err(|e| SdkError::SerializationError(e.to_string()))?;
                    results.push(info);

                    if results.len() >= limit {
                        break;
                    }
                }
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_address_indexer_creation() {
        let temp_dir = tempdir().unwrap();
        let indexer = AddressIndexer::new(temp_dir.path().to_str().unwrap()).unwrap();
        assert!(true); // Just test creation
    }

    #[test]
    fn test_address_indexing() {
        let temp_dir = tempdir().unwrap();
        let indexer = AddressIndexer::new(temp_dir.path().to_str().unwrap()).unwrap();

        let tx = crate::core::Transaction::default();
        let timestamp = 1234567890;

        // Index the transaction
        indexer.index_address_from_transaction(&tx, timestamp).unwrap();

        // Check that sender was indexed
        let info = indexer.get_address_info(&tx.from).unwrap();
        assert!(info.is_some());

        let info = info.unwrap();
        assert_eq!(info.transaction_count, 1);
        assert_eq!(info.first_seen, timestamp);
        assert_eq!(info.last_seen, timestamp);
    }
}