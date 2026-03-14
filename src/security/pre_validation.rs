use std::collections::HashSet;
use tracing::{debug, warn, error};
use crate::core::{Transaction, Block};
use crate::core::transaction::TxPayload;
use crate::crypto;

/// Pre-validation result
#[derive(Debug, Clone)]
pub struct PreValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
}

/// Validation error types
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationError {
    InvalidFormat(String),
    InvalidSignature,
    InsufficientBalance,
    InvalidNonce,
    GasLimitExceeded,
    InvalidGasPrice,
    DuplicateTransaction,
    BlacklistedAddress,
}

/// Stateless transaction pre-validator
/// Performs fast validation checks without accessing full blockchain state
pub struct TransactionPreValidator {
    /// Maximum gas limit per transaction
    max_gas_limit: u64,
    /// Minimum gas price
    min_gas_price: u64,
    /// Maximum transaction size in bytes
    max_tx_size: usize,
    /// Blacklisted addresses
    blacklisted_addresses: HashSet<[u8; 20]>,
    /// Known transaction hashes (for duplicate detection in mempool)
    known_tx_hashes: HashSet<[u8; 32]>,
}

impl TransactionPreValidator {
    /// Create new pre-validator
    pub fn new(max_gas_limit: u64, min_gas_price: u64, max_tx_size: usize) -> Self {
        Self {
            max_gas_limit,
            min_gas_price,
            max_tx_size,
            blacklisted_addresses: HashSet::new(),
            known_tx_hashes: HashSet::new(),
        }
    }

    /// Pre-validate a single transaction
    pub fn pre_validate_transaction(&self, tx: &Transaction) -> PreValidationResult {
        let mut errors = Vec::new();

        // Format validation
        if let Err(e) = self.validate_format(tx) {
            errors.push(e);
        }

        // Signature validation
        if let Err(e) = self.validate_signature(tx) {
            errors.push(e);
        }

        // Gas validation
        if let Err(e) = self.validate_gas(tx) {
            errors.push(e);
        }

        // Address validation
        if let Err(e) = self.validate_addresses(tx) {
            errors.push(e);
        }

        // Duplicate check (if we have known hashes)
        if let Err(e) = self.check_duplicate(tx) {
            errors.push(e);
        }

        PreValidationResult {
            is_valid: errors.is_empty(),
            errors,
        }
    }

    /// Pre-validate multiple transactions
    pub fn pre_validate_transactions(&self, transactions: &[Transaction]) -> Vec<PreValidationResult> {
        transactions.iter()
            .map(|tx| self.pre_validate_transaction(tx))
            .collect()
    }

    /// Validate transaction format and basic structure
    fn validate_format(&self, tx: &Transaction) -> Result<(), ValidationError> {
        // Check transaction size
        let tx_size = std::mem::size_of_val(tx);
        if tx_size > self.max_tx_size {
            return Err(ValidationError::InvalidFormat(
                format!("Transaction size {} exceeds maximum {}", tx_size, self.max_tx_size)
            ));
        }

        // Check amount based on payload type
        let amount = match &tx.payload {
            TxPayload::Transfer { amount, .. } => *amount,
            TxPayload::ContractDeploy { .. } => 0, // Deploy can have 0 amount
            TxPayload::ContractCall { .. } => 0, // Call can have 0 amount
        };

        if amount == 0 && matches!(tx.payload, TxPayload::Transfer { .. }) {
            return Err(ValidationError::InvalidFormat("Transfer transaction amount cannot be zero".to_string()));
        }

        // Check gas limit is reasonable
        if tx.gas_limit == 0 {
            return Err(ValidationError::InvalidFormat("Gas limit cannot be zero".to_string()));
        }

        // Check addresses are not zero
        if tx.from == [0u8; 20] {
            return Err(ValidationError::InvalidFormat("From address cannot be zero".to_string()));
        }

        // Check payload-specific validation
        match &tx.payload {
            TxPayload::Transfer { to, .. } => {
                if *to == [0u8; 20] {
                    return Err(ValidationError::InvalidFormat("To address cannot be zero".to_string()));
                }
                // Check from != to (prevent self-transfers that might be used for DoS)
                if tx.from == *to {
                    return Err(ValidationError::InvalidFormat("From and to addresses cannot be the same".to_string()));
                }
            }
            TxPayload::ContractDeploy { wasm_code, .. } => {
                if wasm_code.is_empty() {
                    return Err(ValidationError::InvalidFormat("Contract deploy must include WASM code".to_string()));
                }
            }
            TxPayload::ContractCall { contract_address, method, .. } => {
                if *contract_address == [0u8; 20] {
                    return Err(ValidationError::InvalidFormat("Contract address cannot be zero".to_string()));
                }
                if method.is_empty() {
                    return Err(ValidationError::InvalidFormat("Contract method cannot be empty".to_string()));
                }
            }
        }

        Ok(())
    }

    /// Validate transaction signature
    fn validate_signature(&self, tx: &Transaction) -> Result<(), ValidationError> {
        // Check if signature exists
        // Basic signature format check (full verification happens in batch verifier)
        if tx.signature.data.len() != 65 {
            return Err(ValidationError::InvalidSignature);
        }

        Ok(())
    }

    /// Validate gas parameters
    fn validate_gas(&self, tx: &Transaction) -> Result<(), ValidationError> {
        // Check gas limit
        if tx.gas_limit > self.max_gas_limit {
            return Err(ValidationError::GasLimitExceeded);
        }

        // Check gas price
        if tx.gas_price < self.min_gas_price {
            return Err(ValidationError::InvalidGasPrice);
        }

        // Check gas price is not unreasonably high (potential DoS)
        if tx.gas_price > self.min_gas_price * 1000 {
            return Err(ValidationError::InvalidGasPrice);
        }

        Ok(())
    }

    /// Validate addresses
    fn validate_addresses(&self, tx: &Transaction) -> Result<(), ValidationError> {
        // Check if addresses are blacklisted
        if self.blacklisted_addresses.contains(&tx.from) {
            return Err(ValidationError::BlacklistedAddress);
        }

        match &tx.payload {
            TxPayload::Transfer { to, .. } | TxPayload::ContractCall { contract_address: to, .. } => {
                if self.blacklisted_addresses.contains(to) {
                    return Err(ValidationError::BlacklistedAddress);
                }
            }
            TxPayload::ContractDeploy { .. } => {
                // No additional address to check for deploy
            }
        }

        Ok(())
    }

    /// Check for duplicate transactions
    fn check_duplicate(&self, tx: &Transaction) -> Result<(), ValidationError> {
        let tx_hash = tx.hash();
        if self.known_tx_hashes.contains(&tx_hash) {
            return Err(ValidationError::DuplicateTransaction);
        }

        Ok(())
    }

    /// Add transaction hash to known set (for duplicate detection)
    pub fn add_known_transaction(&mut self, tx_hash: [u8; 32]) {
        self.known_tx_hashes.insert(tx_hash);
    }

    /// Remove transaction hash from known set
    pub fn remove_known_transaction(&mut self, tx_hash: &[u8; 32]) {
        self.known_tx_hashes.remove(tx_hash);
    }

    /// Clear known transactions (periodic cleanup)
    pub fn clear_known_transactions(&mut self) {
        self.known_tx_hashes.clear();
    }

    /// Add address to blacklist
    pub fn blacklist_address(&mut self, address: [u8; 20]) {
        self.blacklisted_addresses.insert(address);
    }

    /// Remove address from blacklist
    pub fn unblacklist_address(&mut self, address: &[u8; 20]) {
        self.blacklisted_addresses.remove(address);
    }

    /// Get current blacklist size
    pub fn blacklist_size(&self) -> usize {
        self.blacklisted_addresses.len()
    }

    /// Get known transactions count
    pub fn known_transactions_count(&self) -> usize {
        self.known_tx_hashes.len()
    }

    /// Validate block transactions (basic checks)
    pub fn validate_block_transactions(&self, block: &Block) -> PreValidationResult {
        let mut all_errors = Vec::new();
        let mut seen_hashes = HashSet::new();

        for (idx, tx) in block.transactions.iter().enumerate() {
            // Individual validation
            let result = self.pre_validate_transaction(tx);
            if !result.is_valid {
                for error in result.errors {
                    all_errors.push(ValidationError::InvalidFormat(
                        format!("Transaction {}: {:?}", idx, error)
                    ));
                }
            }

            // Check for duplicates within block
            let tx_hash = tx.hash();
            if seen_hashes.contains(&tx_hash) {
                all_errors.push(ValidationError::InvalidFormat(
                    format!("Duplicate transaction in block at index {}", idx)
                ));
            }
            seen_hashes.insert(tx_hash);
        }

        PreValidationResult {
            is_valid: all_errors.is_empty(),
            errors: all_errors,
        }
    }

    /// Estimate gas usage for transaction (rough estimate)
    pub fn estimate_gas_usage(&self, tx: &Transaction) -> u64 {
        // Base gas for transaction
        let mut gas = 21000;

        // Additional gas based on payload
        match &tx.payload {
            TxPayload::Transfer { .. } => {
                // Basic transfer, no additional cost
            }
            TxPayload::ContractDeploy { wasm_code, init_args } => {
                // Gas for code deployment (16 gas per byte for code, 4 for init args)
                gas += (wasm_code.len() as u64) * 16;
                gas += (init_args.len() as u64) * 4;
            }
            TxPayload::ContractCall { args, .. } => {
                // Gas for contract call arguments
                gas += (args.len() as u64) * 4;
            }
        }

        gas.min(tx.gas_limit)
    }
}

impl Default for TransactionPreValidator {
    fn default() -> Self {
        Self::new(
            8_000_000,  // max gas limit
            1_000_000_000, // min gas price (1 gwei)
            128 * 1024, // max tx size (128KB)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secp256k1::{Secp256k1, SecretKey, PublicKey};
    use sha3::{Digest, Keccak256};

    fn create_test_transaction(amount: u64, nonce: u64) -> Transaction {
        let secp = Secp256k1::new();
        let secret_key = SecretKey::from_slice(&[1; 32]).unwrap();
        let public_key = PublicKey::from_secret_key(&secp, &secret_key);

        // Derive address from public key using Keccak256
        use sha3::{Digest, Keccak256};
        let pubkey_bytes = &public_key.serialize()[1..]; // Skip the 0x04 prefix
        let hash = Keccak256::digest(pubkey_bytes);
        let mut from = [0u8; 20];
        from.copy_from_slice(&hash[12..32]); // Take last 20 bytes

        let to = [2u8; 20];

        let mut tx = Transaction::new_transfer(from, to, amount, nonce, 21000, 1_000_000_000);
        tx.sign(&secret_key).unwrap();
        tx
    }

    #[test]
    fn test_valid_transaction() {
        let validator = TransactionPreValidator::default();
        let tx = create_test_transaction(100, 0);

        let result = validator.pre_validate_transaction(&tx);
        assert!(result.is_valid);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_zero_amount() {
        let validator = TransactionPreValidator::default();
        let mut tx = create_test_transaction(0, 0);

        let result = validator.pre_validate_transaction(&tx);
        assert!(!result.is_valid);
        assert!(matches!(result.errors[0], ValidationError::InvalidFormat(_)));
    }

    #[test]
    fn test_self_transfer() {
        let validator = TransactionPreValidator::default();
        let mut tx = create_test_transaction(100, 0);
        // Modify the payload to make it a self-transfer
        if let TxPayload::Transfer { to, .. } = &mut tx.payload {
            *to = tx.from;
        }

        let result = validator.pre_validate_transaction(&tx);
        assert!(!result.is_valid);
        assert!(matches!(result.errors[0], ValidationError::InvalidFormat(_)));
    }

    #[test]
    fn test_gas_limit_exceeded() {
        let validator = TransactionPreValidator::new(1000, 1_000_000_000, 128 * 1024);
        let mut tx = create_test_transaction(100, 0);
        tx.gas_limit = 2000; // Exceeds limit

        let result = validator.pre_validate_transaction(&tx);
        assert!(!result.is_valid);
        assert!(matches!(result.errors[0], ValidationError::GasLimitExceeded));
    }

    #[test]
    fn test_duplicate_detection() {
        let mut validator = TransactionPreValidator::default();
        let tx = create_test_transaction(100, 0);
        let tx_hash = tx.hash();

        // First time should be valid
        let result1 = validator.pre_validate_transaction(&tx);
        assert!(result1.is_valid);

        // Add to known transactions
        validator.add_known_transaction(tx_hash);

        // Second time should be duplicate
        let result2 = validator.pre_validate_transaction(&tx);
        assert!(!result2.is_valid);
        assert!(matches!(result2.errors[0], ValidationError::DuplicateTransaction));
    }

    #[test]
    fn test_blacklist() {
        let mut validator = TransactionPreValidator::default();
        let tx = create_test_transaction(100, 0);

        // Add sender to blacklist
        validator.blacklist_address(tx.from);

        let result = validator.pre_validate_transaction(&tx);
        assert!(!result.is_valid);
        assert!(matches!(result.errors[0], ValidationError::BlacklistedAddress));
    }

    #[test]
    fn test_gas_estimation() {
        let validator = TransactionPreValidator::default();
        let mut tx = create_test_transaction(100, 0);
        tx.payload = TxPayload::ContractDeploy { 
            wasm_code: vec![1, 2, 3, 0, 0], 
            init_args: vec![] 
        };

        let gas = validator.estimate_gas_usage(&tx);
        assert!(gas >= 21000); // Base gas
        assert!(gas <= tx.gas_limit);
    }
}