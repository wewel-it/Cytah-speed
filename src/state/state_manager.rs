use crate::core::transaction::{Transaction, Address};
use crate::state::state_tree::{SparseMerkleTree, Account};

pub type Hash = [u8; 32];

#[derive(Debug, Clone)]
pub struct StateManager {
    pub state_tree: SparseMerkleTree,
    pub current_state_root: Hash,
}

impl StateManager {
    pub fn new() -> Self {
        let state_tree = SparseMerkleTree::new();
        let current_state_root = state_tree.calculate_root();
        Self {
            state_tree,
            current_state_root,
        }
    }

    pub fn apply_transaction(&mut self, tx: &crate::core::transaction::Transaction) -> Result<(), String> {
        use crate::core::transaction::TxPayload;

        match &tx.payload {
            TxPayload::Transfer { to, amount } => self.apply_transfer(tx.from, *to, *amount, tx.nonce),
            _ => {
                // Contract transactions are handled by ContractExecutor
                // For now silently success for contract types
                Ok(())
            }
        }
    }

    fn apply_transfer(&mut self, from: crate::core::transaction::Address, to: crate::core::transaction::Address, amount: u64, nonce: u64) -> Result<(), String> {
        // Validate transaction
        if amount == 0 {
            return Err("Amount must be greater than 0".to_string());
        }

        // Get sender account
        let sender_account = self.state_tree.get_account(&from)
            .cloned()
            .unwrap_or(Account::new(0, 0));

        // Check nonce
        if sender_account.nonce != nonce {
            return Err(format!("Invalid nonce: expected {}, got {}", sender_account.nonce, nonce));
        }

        // Check balance
        if sender_account.balance < amount {
            return Err("Insufficient balance".to_string());
        }

        // Update sender
        let new_sender_balance = sender_account.balance - amount;
        let new_sender_nonce = sender_account.nonce + 1;
        let new_sender_account = Account::new(new_sender_balance, new_sender_nonce);
        self.state_tree.update_account(from, new_sender_account);

        // Get receiver account
        let receiver_account = self.state_tree.get_account(&to)
            .cloned()
            .unwrap_or(Account::new(0, 0));

        // Update receiver
        let new_receiver_balance = receiver_account.balance + amount;
        let new_receiver_account = Account::new(new_receiver_balance, receiver_account.nonce);
        self.state_tree.update_account(to, new_receiver_account);

        // Update state root
        self.current_state_root = self.state_tree.calculate_root();

        Ok(())
    }

    pub fn apply_block(&mut self, _transactions: &[crate::core::transaction::Transaction]) -> Result<(), String> {
        // Transactions are applied by ContractExecutor
        Ok(())
    }

    pub fn get_state_root(&self) -> Hash {
        self.current_state_root
    }

    pub fn get_account(&self, address: &Address) -> Option<&Account> {
        self.state_tree.get_account(address)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secp256k1::{Secp256k1, SecretKey};
    use rand::{Rng, thread_rng};

    fn create_signed_transaction(from: Address, to: Address, amount: u64, nonce: u64, private_key: &SecretKey) -> Transaction {
        let mut tx = Transaction::new(from, to, amount, nonce, 21000);
        tx.sign(private_key).unwrap();
        tx
    }
}