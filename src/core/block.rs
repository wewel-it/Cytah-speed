use sha2::{Sha256, Digest};
use std::fmt;
use std::collections::HashSet;
use serde::{Serialize, Deserialize};
use crate::core::transaction::Transaction;

pub type BlockHash = [u8; 32];
pub type TransactionId = String;

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct Block {
    pub hash: BlockHash,
    pub parent_hashes: Vec<BlockHash>,
    pub timestamp: u64,
    pub transactions: Vec<Transaction>,
    pub nonce: u64,
    pub version: u32,
}

impl Block {
    /// Create new block with given parameters
    pub fn new(
        parent_hashes: Vec<BlockHash>,
        timestamp: u64,
        transactions: Vec<Transaction>,
        nonce: u64,
    ) -> Self {
        let mut block = Block {
            hash: [0u8; 32],
            parent_hashes,
            timestamp,
            transactions,
            nonce,
            version: 1,
        };
        block.hash = block.calculate_hash();
        block
    }

    /// Calculate SHA256 hash of entire block content
    pub fn calculate_hash(&self) -> BlockHash {
        let mut hasher = Sha256::new();

        // Hash parents
        for parent in &self.parent_hashes {
            hasher.update(parent);
        }

        // Hash transactions
        for tx in &self.transactions {
            hasher.update(tx.hash());
        }

        // Hash metadata
        hasher.update(self.timestamp.to_le_bytes());
        hasher.update(self.nonce.to_le_bytes());
        hasher.update(self.version.to_le_bytes());

        hasher.finalize().into()
    }

    /// Validate basic block structure
    pub fn validate_basic(&self) -> Result<(), String> {
        // Check if hash matches content
        let calculated_hash = self.calculate_hash();
        if self.hash != calculated_hash {
            return Err("Block hash mismatch".to_string());
        }

        // Check for duplicate transactions
        let tx_hashes: HashSet<_> = self.transactions.iter().map(|t| t.hash()).collect();
        if tx_hashes.len() != self.transactions.len() {
            return Err("Duplicate transactions found".to_string());
        }

        // Check timestamp is reasonable (not in far future)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        if self.timestamp > now + 3600 {
            return Err("Block timestamp too far in future".to_string());
        }

        Ok(())
    }

    /// Validate block has all references to parent blocks
    pub fn validate_references(&self) -> Result<(), String> {
        if self.parent_hashes.is_empty() {
            return Err("Block must have at least one parent (except genesis)".to_string());
        }
        Ok(())
    }

    /// Check if this is a genesis block (no parents or explicit genesis)
    pub fn is_genesis(&self) -> bool {
        self.parent_hashes.is_empty()
    }

    /// Get all transaction hashes in this block
    pub fn transaction_hashes(&self) -> Vec<String> {
        self.transactions.iter().map(|t| hex::encode(t.hash())).collect()
    }

    /// Get size estimate in bytes
    pub fn size_estimate(&self) -> usize {
        std::mem::size_of::<BlockHash>()
            + self.parent_hashes.len() * std::mem::size_of::<BlockHash>()
            + self.transactions.len() * (std::mem::size_of::<TransactionId>() + 256 + 8)
            + 8  // timestamp
            + 8  // nonce
            + 4  // version
    }
}

impl fmt::Display for Block {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Block {{ hash: {}, parents: {:?}, timestamp: {}, txs: {}, nonce: {} }}",
            hex::encode(&self.hash[..std::cmp::min(16, self.hash.len())]),
            &self.parent_hashes,
            self.timestamp,
            self.transactions.len(),
            self.nonce
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_creation() {
        let from: [u8; 20] = [1; 20];
        let to: [u8; 20] = [2; 20];
        let tx = Transaction::new(from, to, 100, 1, 21000);
        assert_eq!(tx.amount, 100);
        assert_eq!(tx.nonce, 1);
    }

    #[test]
    fn test_block_hash_calculation() {
        let from: [u8; 20] = [1; 20];
        let to: [u8; 20] = [2; 20];
        let tx = Transaction::new(from, to, 100, 0, 21000);
        let block = Block::new(vec![], 1000, vec![tx], 42);
        
        assert_eq!(block.hash.len(), 32); // SHA256 produces 32 bytes
    }

    #[test]
    fn test_block_validation_basic() {
        let from: [u8; 20] = [1; 20];
        let to: [u8; 20] = [2; 20];
        let tx = Transaction::new(from, to, 100, 0, 21000);
        let block = Block::new(vec![], 1000, vec![tx], 42);
        
        assert!(block.validate_basic().is_ok());
    }

    #[test]
    fn test_block_genesis_detection() {
        let block = Block::new(vec![], 1000, vec![], 0);
        assert!(block.is_genesis());
    }

    #[test]
    fn test_block_with_multiple_parents() {
        let parent1 = "hash1".to_string();
        let parent2 = "hash2".to_string();
        let from: [u8; 20] = [1; 20];
        let to: [u8; 20] = [2; 20];
        let tx = Transaction::new(from, to, 100, 0, 21000);
        
        let block = Block::new(vec![parent1, parent2], 1000, vec![tx], 42);
        assert_eq!(block.parent_hashes.len(), 2);
        assert!(block.validate_basic().is_ok());
    }

    #[test]
    fn test_duplicate_transaction_detection() {
        let from: [u8; 20] = [1; 20];
        let to: [u8; 20] = [2; 20];
        let tx1 = Transaction::new(from, to, 100, 0, 21000);
        let tx2 = Transaction::new(from, to, 100, 0, 21000); // Exact duplicate
        
        let block = Block::new(vec![], 1000, vec![tx1, tx2], 42);
        assert!(block.validate_basic().is_err());
    }

    #[test]
    fn test_hash_consistency() {
        let from: [u8; 20] = [1; 20];
        let to: [u8; 20] = [2; 20];
        let tx = Transaction::new(from, to, 100, 0, 21000);
        let block1 = Block::new(vec!["parent1".to_string()], 1000, vec![tx.clone()], 42);
        let block2 = Block::new(vec!["parent1".to_string()], 1000, vec![tx], 42);
        
        assert_eq!(block1.hash, block2.hash);
    }
}
