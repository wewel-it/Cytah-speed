use crate::state::state_manager::StateManager;
use crate::Block;
use serde::{Deserialize, Serialize};
use crate::vm::contract_executor::ContractExecutor;

pub type Hash = [u8; 32];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub new_state_root: Hash,
    pub executed_transactions: usize,
    pub success: bool,
    pub total_fees: u64,
}

#[derive(Debug)]
pub struct TransactionExecutor {
    pub contract_executor: ContractExecutor,
}

impl TransactionExecutor {
    pub fn new() -> Self {
        let state_mgr = StateManager::new();
        Self {
            contract_executor: ContractExecutor::new(state_mgr),
        }
    }

    pub fn execute_block(&mut self, block: &Block) -> ExecutionResult {
        let mut executed = 0;
        let mut success = true;
        let mut total_fees: u64 = 0;

        for tx in &block.transactions {
            match self.contract_executor.execute_transaction(tx) {
                Ok(used) => {
                    executed += 1;
                    total_fees = total_fees.saturating_add(used.saturating_mul(tx.gas_price));
                }
                Err(_e) => {
                    success = false;
                    break; // Stop on first failure
                }
            }
        }

        ExecutionResult {
            new_state_root: self.contract_executor.state_manager.get_state_root(),
            executed_transactions: executed,
            success,
            total_fees,
        }
    }

    pub fn execute_blocks_in_order(&mut self, blocks: &[Block]) -> Vec<ExecutionResult> {
        blocks.iter().map(|block| self.execute_block(block)).collect()
    }

    pub fn get_current_state_root(&self) -> Hash {
        self.contract_executor.state_manager.get_state_root()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::transaction::{Transaction, Address};
    use secp256k1::{Secp256k1, SecretKey};
    use sha2::{Digest, Sha256};
    use rand::{Rng, thread_rng};

    fn create_signed_transaction(from: Address, to: Address, amount: u64, nonce: u64, private_key: &SecretKey) -> Transaction {
        let mut tx = Transaction::new_transfer(from, to, amount, nonce, 21000, 1);
        tx.sign(private_key).unwrap();
        tx
    }

    fn create_test_block(transactions: Vec<Transaction>) -> Block {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        Block::new(vec![], timestamp, transactions, 0, 0, 0, [0;20], [0;32])
    }

    #[test]
    fn test_execute_block() {
        let secp = Secp256k1::new();
        let mut rng = rand::thread_rng();
        let secret_bytes: [u8; 32] = rng.gen();
        let secret_key = SecretKey::from_slice(&secret_bytes).unwrap();
        let public_key = secret_key.public_key(&secp);
        let pubkey_hash = Sha256::digest(&public_key.serialize()[1..]);
        let from: Address = pubkey_hash[12..32].try_into().unwrap();
        let to: Address = [2; 20];

        let mut executor = TransactionExecutor::new();
        executor.contract_executor.state_manager.state_tree.update_account(from, crate::state::state_tree::Account::new(1000, 0));

        let tx = create_signed_transaction(from, to, 100, 0, &secret_key);
        let block = create_test_block(vec![tx]);

        let result = executor.execute_block(&block);
        assert!(result.success);
        assert_eq!(result.executed_transactions, 1);
        assert_ne!(result.new_state_root, [0; 32]);
        assert!(result.total_fees > 0);

    }

    #[test]
    fn test_execute_block_with_invalid_transaction() {
        let secp = Secp256k1::new();
        let mut rng = rand::thread_rng();
        let secret_bytes: [u8; 32] = rng.gen();
        let secret_key = SecretKey::from_slice(&secret_bytes).unwrap();
        let from: Address = [1; 20];
        let to: Address = [2; 20];

        let mut executor = TransactionExecutor::new();
        executor.contract_executor.state_manager.state_tree.update_account(from, crate::state::state_tree::Account::new(50, 0));

        let tx = create_signed_transaction(from, to, 100, 0, &secret_key); // Insufficient balance
        let block = create_test_block(vec![tx]);

        let result = executor.execute_block(&block);
        assert!(!result.success);
        assert_eq!(result.executed_transactions, 0);
        assert_eq!(result.total_fees, 0);

    }

    #[test]
    fn test_execute_multiple_blocks() {
        let secp = Secp256k1::new();
        let mut rng = rand::thread_rng();
        let secret_bytes: [u8; 32] = rng.gen();
        let secret_key = SecretKey::from_slice(&secret_bytes).unwrap();
        let public_key = secret_key.public_key(&secp);
        let pubkey_hash = Sha256::digest(&public_key.serialize()[1..]);
        let from: Address = pubkey_hash[12..32].try_into().unwrap();
        let to: Address = [2; 20];

        let mut executor = TransactionExecutor::new();
        executor.contract_executor.state_manager.state_tree.update_account(from, crate::state::state_tree::Account::new(1000, 0));

        let tx1 = create_signed_transaction(from, to, 100, 0, &secret_key);
        let tx2 = create_signed_transaction(from, to, 100, 1, &secret_key);
        let block1 = create_test_block(vec![tx1]);
        let block2 = create_test_block(vec![tx2]);

        let results = executor.execute_blocks_in_order(&[block1, block2]);
        assert_eq!(results.len(), 2);
        assert!(results[0].success);
        assert!(results[1].success);
        assert_eq!(results[0].executed_transactions, 1);
        assert_eq!(results[1].executed_transactions, 1);
        assert!(results[0].total_fees > 0);
        assert!(results[1].total_fees > 0);
    }
}