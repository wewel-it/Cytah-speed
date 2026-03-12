use sha2::{Sha256, Digest};
use std::fmt;
use std::collections::HashSet;
use serde::{Serialize, Deserialize};
use crate::core::transaction::Transaction;

pub type BlockHash = [u8; 32];
pub type TransactionId = String;

/// Header portion of a block which is used for hashing and PoW
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BlockHeader {
    pub parent_hashes: Vec<BlockHash>,
    pub timestamp: u64,
    pub nonce: u64,
    pub difficulty: u32, // expressed as leading-zero bits requirement
    pub base_fee: u64,   // EIP-1559 style base fee burned per gas unit
    pub state_root: [u8;32], // merkle root of state after applying this block
    pub version: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Block {
    pub hash: BlockHash,
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
    pub producer: [u8; 20],
    pub reward: f64,
}

impl Block {
    /// Create new block with given parameters
    /// Create new block with given parameters.  The `difficulty` field is
    /// typically supplied by the miner and affects the PoW target; for regular
    /// construction it can be zero.
    pub fn new(
        parent_hashes: Vec<BlockHash>,
        timestamp: u64,
        transactions: Vec<Transaction>,
        nonce: u64,
        difficulty: u32,
        base_fee: u64,
        producer: [u8;20],
        state_root: [u8;32],
    ) -> Self {
        let reward = crate::consensus::calculate_reward(transactions.len())
            + transactions.iter().map(|tx| tx.gas_price.saturating_sub(base_fee)).sum::<u64>() as f64;
        let header = BlockHeader {
            parent_hashes,
            timestamp,
            nonce,
            difficulty,
            base_fee,
            state_root,
            version: 1,
        };
        let mut block = Block {
            hash: [0u8; 32],
            header,
            transactions,
            producer,
            reward,
        };
        block.hash = block.calculate_hash();
        block
    }

    /// Calculate SHA256 hash of entire block content
    pub fn calculate_hash(&self) -> BlockHash {
        let mut hasher = Sha256::new();

        // Hash parents
        for parent in &self.header.parent_hashes {
            hasher.update(parent);
        }

        // Hash transactions
        for tx in &self.transactions {
            hasher.update(tx.hash());
        }

        // Hash header metadata
        hasher.update(self.header.timestamp.to_le_bytes());
        hasher.update(self.header.nonce.to_le_bytes());
        hasher.update(self.header.difficulty.to_le_bytes());
        hasher.update(self.header.base_fee.to_le_bytes());
        hasher.update(&self.header.state_root);
        hasher.update(self.header.version.to_le_bytes());
        // include producer in hash so that different miners produce different blocks
        hasher.update(&self.producer);

        hasher.finalize().into()
    }

    /// Validate basic block structure
    pub fn validate_basic(&self) -> Result<(), String> {
        // Check if hash matches content
        // verify header/hash consistency
        let calculated_hash = self.calculate_hash();
        if self.hash != calculated_hash {
            return Err("Block hash mismatch".to_string());
        }

        // verify PoW meets difficulty (difficulty of 0 is treated as no-PoW)
        if self.header.difficulty > 0 &&
           !crate::consensus::meets_difficulty(&self.hash, self.header.difficulty)
        {
            return Err("Block does not satisfy PoW difficulty".to_string());
        }
        // state_root validity is checked elsewhere (during sync)

        // verify reward matches expectation (base reward + total priority fees)
        let base = crate::consensus::calculate_reward(self.transactions.len());
        let total_tips: u64 = self.transactions
            .iter()
            .map(|tx| tx.gas_price.saturating_sub(self.header.base_fee))
            .sum();
        let expected = base + total_tips as f64;
        if (self.reward - expected).abs() > std::f64::EPSILON {
            return Err(format!("Unexpected block reward: {} vs {}", self.reward, expected));
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
        
        if self.header.timestamp > now + 3600 {
            return Err("Block timestamp too far in future".to_string());
        }

        Ok(())
    }

    /// Validate block has all references to parent blocks
    pub fn validate_references(&self) -> Result<(), String> {
        if self.header.parent_hashes.is_empty() {
            return Err("Block must have at least one parent (except genesis)".to_string());
        }
        Ok(())
    }

    /// Check if this is a genesis block (no parents or explicit genesis)
    pub fn is_genesis(&self) -> bool {
        self.header.parent_hashes.is_empty()
    }

    /// Get all transaction hashes in this block
    pub fn transaction_hashes(&self) -> Vec<String> {
        self.transactions.iter().map(|t| hex::encode(t.hash())).collect()
    }

    /// Get size estimate in bytes
    pub fn size_estimate(&self) -> usize {
        std::mem::size_of::<BlockHash>()
            + self.header.parent_hashes.len() * std::mem::size_of::<BlockHash>()
            + self.transactions.len() * (std::mem::size_of::<TransactionId>() + 256 + 8)
            + 8  // timestamp
            + 8  // nonce
            + 4  // version
            + 4  // difficulty
            + 20 // producer address
            + 8  // reward
    }
}

impl fmt::Display for Block {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Block {{ hash: {}, parents: {:?}, ts: {}, txs: {}, nonce: {}, diff: {}, producer: {}, reward: {} }}",
            hex::encode(&self.hash[..std::cmp::min(16, self.hash.len())]),
            &self.header.parent_hashes,
            self.header.timestamp,
            self.transactions.len(),
            self.header.nonce,
            self.header.difficulty,
            hex::encode(self.producer),
            self.reward
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
        let tx = Transaction::new_transfer(from, to, 100, 1, 21000, 1);
        if let crate::core::transaction::TxPayload::Transfer { amount, .. } = tx.payload {
            assert_eq!(amount, 100);
        } else {
            panic!("Expected transfer payload");
        }
        assert_eq!(tx.nonce, 1);
    }

    #[test]
    fn test_block_hash_calculation() {
        let from: [u8; 20] = [1; 20];
        let to: [u8; 20] = [2; 20];
        let tx = Transaction::new(from, to, 100, 0, 21000, 1);
        let block = Block::new(vec![], 1000, vec![tx], 42, 0, 0, [0;20], [0;32]);
        
        assert_eq!(block.hash.len(), 32); // SHA256 produces 32 bytes
    }

    #[test]
    fn test_block_validation_basic() {
        let from: [u8; 20] = [1; 20];
        let to: [u8; 20] = [2; 20];
        let tx = Transaction::new(from, to, 100, 0, 21000, 1);
        let block = Block::new(vec![], 1000, vec![tx], 42, 0, 0, [0;20], [0;32]);
        
        assert!(block.validate_basic().is_ok());
    }

    #[test]
    fn test_block_genesis_detection() {
        let block = Block::new(vec![], 1000, vec![], 0, 0, 0, [0;20], [0;32]);
        assert!(block.is_genesis());
    }

    #[test]
    fn test_block_with_multiple_parents() {
        let parent1 = "hash1".to_string();
        let parent2 = "hash2".to_string();
        let from: [u8; 20] = [1; 20];
        let to: [u8; 20] = [2; 20];
        let tx = Transaction::new(from, to, 100, 0, 21000, 1);
        
        let block = Block::new(vec![parent1, parent2], 1000, vec![tx], 42, 0, 0, [0;20], [0;32]);
        assert_eq!(block.header.parent_hashes.len(), 2);
        assert!(block.validate_basic().is_ok());
    }

    #[test]
    fn test_duplicate_transaction_detection() {
        let from: [u8; 20] = [1; 20];
        let to: [u8; 20] = [2; 20];
        let tx1 = Transaction::new(from, to, 100, 0, 21000, 1);
        let tx2 = Transaction::new(from, to, 100, 0, 21000, 1); // Exact duplicate
        
        let block = Block::new(vec![], 1000, vec![tx1, tx2], 42, 0, 0, [0;20], [0;32]);
        assert!(block.validate_basic().is_err());
    }

    #[test]
    fn test_hash_consistency() {
        let from: [u8; 20] = [1; 20];
        let to: [u8; 20] = [2; 20];
        let tx = Transaction::new(from, to, 100, 0, 21000, 1);
        let block1 = Block::new(vec!["parent1".to_string()], 1000, vec![tx.clone()], 42, 0, 0, [0;20], [0;32]);
        let block2 = Block::new(vec!["parent1".to_string()], 1000, vec![tx], 42, 0, 0, [0;20], [0;32]);
        
        assert_eq!(block1.hash, block2.hash); // same inputs including difficulty 0
    }

    #[test]
    fn test_reward_calculation_matches_consensus() {
        let from: [u8; 20] = [1; 20];
        let to: [u8; 20] = [2; 20];
        // empty block
        let b0 = Block::new(vec![], 1000, vec![], 0, 0, 0, [0;20], [0;32]);
        assert_eq!(b0.reward, crate::consensus::calculate_reward(0));
        // moderate tx count
        let mut txs = Vec::new();
        for i in 0..50 {
            txs.push(Transaction::new(from, to, 100, i, 21000, 1));
        }
        let b1 = Block::new(vec![], 1000, txs.clone(), 0, 0, 0, [0;20], [0;32]);
        assert_eq!(b1.reward, crate::consensus::calculate_reward(50));
    }

    #[test]
    fn test_pow_check_in_validation() {
        // build a block with difficulty 1 (very easy)
        let from: [u8; 20] = [1; 20];
        let to: [u8; 20] = [2; 20];
        let tx = Transaction::new(from, to, 100, 0, 21000, 1);
        let mut block = Block::new(vec![], 1000, vec![tx], 0, 1, 0, [0;20], [0;32]);
        // we may need to adjust nonce until satisfies
        let mut nonce = 0;
        while !crate::consensus::meets_difficulty(&block.hash, block.header.difficulty) && nonce < 1000 {
            nonce += 1;
            block.header.nonce = nonce;
            block.hash = block.calculate_hash();
        }
        assert!(crate::consensus::meets_difficulty(&block.hash, 1));
        assert!(block.validate_basic().is_ok());
    }
}
