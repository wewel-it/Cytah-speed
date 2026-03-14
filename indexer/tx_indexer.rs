use rocksdb::{DB, IteratorMode, Options};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::core::{Transaction, Address, BlockHash};
use crate::sdk::errors::SdkError;

/// Transaction indexer for efficient transaction queries
pub struct TransactionIndexer {
    db: Arc<DB>,
}

impl TransactionIndexer {
    /// Create a new transaction indexer
    pub fn new(db_path: &str) -> Result<Self, SdkError> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.set_max_open_files(10000);

        let db = DB::open(&opts, db_path)
            .map_err(|e| SdkError::TransactionError(format!("Failed to open transaction index DB: {}", e)))?;

        Ok(TransactionIndexer { db: Arc::new(db) })
    }

    /// Index a transaction
    pub fn index_transaction(&self, tx: &Transaction, block_hash: Option<&BlockHash>) -> Result<(), SdkError> {
        let tx_hash = tx.hash();
        let tx_key = format!("tx:{}", hex::encode(&tx_hash));
        let tx_data = serde_json::to_vec(tx)
            .map_err(|e| SdkError::SerializationError(e.to_string()))?;

        self.db.put(tx_key.as_bytes(), tx_data)
            .map_err(|e| SdkError::TransactionError(format!("Failed to index transaction: {}", e)))?;

        // Index by sender
        let sender_key = format!("sender:{}:{}", hex::encode(tx.from), hex::encode(&tx_hash));
        self.db.put(sender_key.as_bytes(), &tx_hash)
            .map_err(|e| SdkError::TransactionError(format!("Failed to index sender: {}", e)))?;

        // Index by recipient (for transfer transactions)
        if let crate::core::TxPayload::Transfer { to, .. } = &tx.payload {
            let recipient_key = format!("recipient:{}:{}", hex::encode(to), hex::encode(&tx_hash));
            self.db.put(recipient_key.as_bytes(), &tx_hash)
                .map_err(|e| SdkError::TransactionError(format!("Failed to index recipient: {}", e)))?;
        }

        // Index by contract (for contract transactions)
        if let crate::core::TxPayload::ContractCall { contract_address, .. } = &tx.payload {
            let contract_key = format!("contract:{}:{}", hex::encode(contract_address), hex::encode(&tx_hash));
            self.db.put(contract_key.as_bytes(), &tx_hash)
                .map_err(|e| SdkError::TransactionError(format!("Failed to index contract: {}", e)))?;
        }

        // Index by block if provided
        if let Some(block_hash) = block_hash {
            let block_tx_key = format!("block_tx:{}:{}", hex::encode(block_hash), hex::encode(&tx_hash));
            self.db.put(block_tx_key.as_bytes(), &tx_hash)
                .map_err(|e| SdkError::TransactionError(format!("Failed to index block transaction: {}", e)))?;
        }

        Ok(())
    }

    /// Get transaction by hash
    pub fn get_transaction(&self, tx_hash: &[u8]) -> Result<Option<Transaction>, SdkError> {
        let key = format!("tx:{}", hex::encode(tx_hash));
        match self.db.get(key.as_bytes())
            .map_err(|e| SdkError::TransactionError(format!("Failed to get transaction: {}", e)))? {
            Some(data) => {
                let tx: Transaction = serde_json::from_slice(&data)
                    .map_err(|e| SdkError::SerializationError(e.to_string()))?;
                Ok(Some(tx))
            }
            None => Ok(None),
        }
    }

    /// Get transactions by sender
    pub fn get_transactions_by_sender(&self, sender: &Address, limit: Option<usize>) -> Result<Vec<String>, SdkError> {
        let prefix = format!("sender:{}:", hex::encode(sender));
        self.get_transactions_by_prefix(&prefix, limit)
    }

    /// Get transactions by recipient
    pub fn get_transactions_by_recipient(&self, recipient: &Address, limit: Option<usize>) -> Result<Vec<String>, SdkError> {
        let prefix = format!("recipient:{}:", hex::encode(recipient));
        self.get_transactions_by_prefix(&prefix, limit)
    }

    /// Get transactions by contract
    pub fn get_transactions_by_contract(&self, contract: &Address, limit: Option<usize>) -> Result<Vec<String>, SdkError> {
        let prefix = format!("contract:{}:", hex::encode(contract));
        self.get_transactions_by_prefix(&prefix, limit)
    }

    /// Get transactions by block
    pub fn get_transactions_by_block(&self, block_hash: &BlockHash, limit: Option<usize>) -> Result<Vec<String>, SdkError> {
        let prefix = format!("block_tx:{}:", hex::encode(block_hash));
        self.get_transactions_by_prefix(&prefix, limit)
    }

    /// Helper method to get transactions by prefix
    fn get_transactions_by_prefix(&self, prefix: &str, limit: Option<usize>) -> Result<Vec<String>, SdkError> {
        let mut transactions = Vec::new();
        let max_results = limit.unwrap_or(1000);

        let iter = self.db.iterator(IteratorMode::From(prefix.as_bytes(), rocksdb::Direction::Forward));
        for item in iter {
            let (key, value) = item
                .map_err(|e| SdkError::TransactionError(format!("Iterator error: {}", e)))?;

            let key_str = String::from_utf8_lossy(&key);
            if key_str.starts_with(prefix) {
                let tx_hash = hex::encode(value);
                transactions.push(tx_hash);

                if transactions.len() >= max_results {
                    break;
                }
            } else {
                break; // Past the prefix
            }
        }

        Ok(transactions)
    }

    /// Get transaction count
    pub fn get_transaction_count(&self) -> Result<u64, SdkError> {
        let prefix = b"tx:";
        let mut count = 0u64;

        let iter = self.db.iterator(IteratorMode::From(prefix, rocksdb::Direction::Forward));
        for item in iter {
            let (key, _) = item
                .map_err(|e| SdkError::TransactionError(format!("Iterator error: {}", e)))?;

            let key_str = String::from_utf8_lossy(&key);
            if key_str.starts_with("tx:") {
                count += 1;
            } else {
                break;
            }
        }

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_transaction_indexer_creation() {
        let temp_dir = tempdir().unwrap();
        let indexer = TransactionIndexer::new(temp_dir.path().to_str().unwrap()).unwrap();
        assert!(true); // Just test creation
    }

    #[test]
    fn test_transaction_indexing() {
        let temp_dir = tempdir().unwrap();
        let indexer = TransactionIndexer::new(temp_dir.path().to_str().unwrap()).unwrap();

        let tx = crate::core::Transaction::default();

        // Index the transaction
        indexer.index_transaction(&tx, None).unwrap();

        // Retrieve the transaction
        let retrieved = indexer.get_transaction(&tx.hash()).unwrap();
        assert!(retrieved.is_some());
    }
}