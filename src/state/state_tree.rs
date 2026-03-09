use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type Address = [u8; 20];
pub type Hash = [u8; 32];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Account {
    pub balance: u64,
    pub nonce: u64,
}

impl Account {
    pub fn new(balance: u64, nonce: u64) -> Self {
        Self { balance, nonce }
    }

    pub fn hash(&self) -> Hash {
        let mut hasher = Sha256::new();
        hasher.update(&self.balance.to_le_bytes());
        hasher.update(&self.nonce.to_le_bytes());
        hasher.finalize().into()
    }
}

#[derive(Debug, Clone)]
pub struct MerkleNode {
    pub hash: Hash,
    pub left: Option<Box<MerkleNode>>,
    pub right: Option<Box<MerkleNode>>,
    pub key: Option<Address>,
    pub value: Option<Account>,
}

impl MerkleNode {
    pub fn new_leaf(key: Address, value: Account) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(&key);
        hasher.update(&value.hash());
        let hash = hasher.finalize().into();
        Self {
            hash,
            left: None,
            right: None,
            key: Some(key),
            value: Some(value),
        }
    }

    pub fn new_internal(left: MerkleNode, right: MerkleNode) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(&left.hash);
        hasher.update(&right.hash);
        let hash = hasher.finalize().into();
        Self {
            hash,
            left: Some(Box::new(left)),
            right: Some(Box::new(right)),
            key: None,
            value: None,
        }
    }

    pub fn is_leaf(&self) -> bool {
        self.key.is_some() && self.value.is_some()
    }
}

#[derive(Debug, Clone)]
pub struct SparseMerkleTree {
    pub root: Option<MerkleNode>,
    pub leaves: HashMap<Address, Account>,
}

impl SparseMerkleTree {
    pub fn new() -> Self {
        Self {
            root: None,
            leaves: HashMap::new(),
        }
    }

    pub fn get_account(&self, address: &Address) -> Option<&Account> {
        self.leaves.get(address)
    }

    pub fn update_account(&mut self, address: Address, account: Account) {
        self.leaves.insert(address, account);
        self.rebuild_tree();
    }

    pub fn calculate_root(&self) -> Hash {
        if let Some(ref root) = self.root {
            root.hash
        } else {
            [0; 32] // Empty tree hash
        }
    }

    fn rebuild_tree(&mut self) {
        if self.leaves.is_empty() {
            self.root = None;
            return;
        }

        let mut nodes: Vec<MerkleNode> = self.leaves.iter()
            .map(|(k, v)| MerkleNode::new_leaf(*k, v.clone()))
            .collect();

        while nodes.len() > 1 {
            let mut new_nodes = Vec::new();
            for chunk in nodes.chunks(2) {
                if chunk.len() == 2 {
                    new_nodes.push(MerkleNode::new_internal(chunk[0].clone(), chunk[1].clone()));
                } else {
                    new_nodes.push(chunk[0].clone());
                }
            }
            nodes = new_nodes;
        }

        self.root = nodes.into_iter().next();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_creation() {
        let account = Account::new(1000, 1);
        assert_eq!(account.balance, 1000);
        assert_eq!(account.nonce, 1);
    }

    #[test]
    fn test_sparse_merkle_tree() {
        let mut tree = SparseMerkleTree::new();
        let addr1: Address = [1; 20];
        let addr2: Address = [2; 20];
        let account1 = Account::new(100, 1);
        let account2 = Account::new(200, 2);

        tree.update_account(addr1, account1.clone());
        tree.update_account(addr2, account2.clone());

        assert_eq!(tree.get_account(&addr1), Some(&account1));
        assert_eq!(tree.get_account(&addr2), Some(&account2));

        let root = tree.calculate_root();
        assert_ne!(root, [0; 32]);
    }

    #[test]
    fn test_state_root_changes() {
        let mut tree = SparseMerkleTree::new();
        let addr: Address = [1; 20];
        let root1 = tree.calculate_root();

        tree.update_account(addr, Account::new(100, 1));
        let root2 = tree.calculate_root();

        assert_ne!(root1, root2);
    }
}