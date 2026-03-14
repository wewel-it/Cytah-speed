use rocksdb::{DB, IteratorMode, Options};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::core::{Block, BlockHash, Transaction, Address};
use crate::sdk::errors::SdkError;

/// Block indexer for efficient block data queries
pub struct BlockIndexer {
    db: Arc<DB>,
}

impl BlockIndexer {
    /// Create a new block indexer
    pub fn new(db_path: &str) -> Result<Self, SdkError> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.set_max_open_files(10000);

        let db = DB::open(&opts, db_path)
            .map_err(|e| SdkError::TransactionError(format!("Failed to open block index DB: {}", e)))?;

        Ok(BlockIndexer { db: Arc::new(db) })
    }

    /// Index a block
    pub fn index_block(&self, block: &Block) -> Result<(), SdkError> {
        let block_key = format!("block:{}", hex::encode(&block.hash()));
        let block_data = serde_json::to_vec(block)
            .map_err(|e| SdkError::SerializationError(e.to_string()))?;

        self.db.put(block_key.as_bytes(), block_data)
            .map_err(|e| SdkError::TransactionError(format!("Failed to index block: {}", e)))?;

        // Index by height
        let height_key = format!("height:{}", block.height);
        let hash_data = block.hash();
        self.db.put(height_key.as_bytes(), hash_data)
            .map_err(|e| SdkError::TransactionError(format!("Failed to index block height: {}", e)))?;

        // Index transactions in this block
        for (tx_index, tx) in block.transactions.iter().enumerate() {
            let tx_key = format!("block_tx:{}:{}", hex::encode(&block.hash()), tx_index);
            let tx_hash = tx.hash();
            self.db.put(tx_key.as_bytes(), tx_hash)
                .map_err(|e| SdkError::TransactionError(format!("Failed to index block transaction: {}", e)))?;
        }

        Ok(())
    }

    /// Get block by hash
    pub fn get_block(&self, hash: &BlockHash) -> Result<Option<Block>, SdkError> {
        let key = format!("block:{}", hex::encode(hash));
        match self.db.get(key.as_bytes())
            .map_err(|e| SdkError::TransactionError(format!("Failed to get block: {}", e)))? {
            Some(data) => {
                let block: Block = serde_json::from_slice(&data)
                    .map_err(|e| SdkError::SerializationError(e.to_string()))?;
                Ok(Some(block))
            }
            None => Ok(None),
        }
    }

    /// Get block by height
    pub fn get_block_by_height(&self, height: u64) -> Result<Option<Block>, SdkError> {
        let height_key = format!("height:{}", height);
        match self.db.get(height_key.as_bytes())
            .map_err(|e| SdkError::TransactionError(format!("Failed to get block by height: {}", e)))? {
            Some(hash_data) => {
                let hash = hash_data.try_into()
                    .map_err(|_| SdkError::TransactionError("Invalid block hash data".to_string()))?;
                self.get_block(&hash)
            }
            None => Ok(None),
        }
    }

    /// Get transaction hashes in a block
    pub fn get_block_transactions(&self, block_hash: &BlockHash) -> Result<Vec<String>, SdkError> {
        let prefix = format!("block_tx:{}:", hex::encode(block_hash));
        let mut transactions = Vec::new();

        let iter = self.db.iterator(IteratorMode::From(prefix.as_bytes(), rocksdb::Direction::Forward));
        for item in iter {
            let (key, value) = item
                .map_err(|e| SdkError::TransactionError(format!("Iterator error: {}", e)))?;

            let key_str = String::from_utf8_lossy(&key);
            if key_str.starts_with(&prefix) {
                let tx_hash = hex::encode(value);
                transactions.push(tx_hash);
            } else {
                break; // Past the prefix
            }
        }

        Ok(transactions)
    }

    /// Get latest block height
    pub fn get_latest_height(&self) -> Result<Option<u64>, SdkError> {
        let prefix = b"height:";
        let mut latest_height = None;

        let iter = self.db.iterator(IteratorMode::From(prefix, rocksdb::Direction::Reverse));
        if let Some(item) = iter.next() {
            let (key, _) = item
                .map_err(|e| SdkError::TransactionError(format!("Iterator error: {}", e)))?;

            let key_str = String::from_utf8_lossy(&key);
            if key_str.starts_with("height:") {
                if let Some(height_str) = key_str.strip_prefix("height:") {
                    if let Ok(height) = height_str.parse::<u64>() {
                        latest_height = Some(height);
                    }
                }
            }
        }

        Ok(latest_height)
    }

    /// Get blocks in a range
    pub fn get_blocks_in_range(&self, start_height: u64, end_height: u64) -> Result<Vec<Block>, SdkError> {
        let mut blocks = Vec::new();

        for height in start_height..=end_height {
            if let Some(block) = self.get_block_by_height(height)? {
                blocks.push(block);
            }
        }

        Ok(blocks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_block_indexer_creation() {
        let temp_dir = tempdir().unwrap();
        let indexer = BlockIndexer::new(temp_dir.path().to_str().unwrap()).unwrap();
        assert!(true); // Just test creation
    }

    #[test]
    fn test_block_indexing() {
        let temp_dir = tempdir().unwrap();
        let indexer = BlockIndexer::new(temp_dir.path().to_str().unwrap()).unwrap();

        let block = Block::default();

        // Index the block
        indexer.index_block(&block).unwrap();

        // Retrieve the block
        let retrieved = indexer.get_block(&block.hash()).unwrap();
        assert!(retrieved.is_some());

        // Retrieve by height
        let retrieved_by_height = indexer.get_block_by_height(block.height).unwrap();
        assert!(retrieved_by_height.is_some());
    }
}