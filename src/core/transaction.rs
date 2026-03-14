use sha2::{Digest, Sha256};
use secp256k1::{Message, PublicKey, Secp256k1, SecretKey};
use serde::{Deserialize, Serialize};

use crate::crypto::{CryptoAlgorithm, Signature};

pub type Address = [u8; 20];
pub type BlockHash = [u8; 32];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TxPayload {
    Transfer { to: Address, amount: u64 },
    ContractDeploy { wasm_code: Vec<u8>, init_args: Vec<u8> },
    ContractCall { contract_address: Address, method: String, args: Vec<u8> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Transaction {
    pub from: Address,
    pub payload: TxPayload,
    pub nonce: u64,
    pub gas_limit: u64,
    pub gas_price: u64,
    pub signature: Signature,
}

impl Transaction {
    /// Generic constructor (alias for new_transfer) to preserve existing test expectations
    pub fn new(from: Address, to: Address, amount: u64, nonce: u64, gas_limit: u64, gas_price: u64) -> Self {
        Self::new_transfer(from, to, amount, nonce, gas_limit, gas_price)
    }

    /// convenience ctor for transfer payload
    pub fn new_transfer(from: Address, to: Address, amount: u64, nonce: u64, gas_limit: u64, gas_price: u64) -> Self {
        Self {
            from,
            payload: TxPayload::Transfer { to, amount },
            nonce,
            gas_limit,
            gas_price,
            signature: Signature::empty(),
        }
    }

    /// Create a minimal default transaction
    pub fn default_transfer() -> Self {
        Self::new_transfer([0; 20], [0; 20], 0, 0, 0, 0)
    }

    pub fn new_deploy(from: Address, wasm_code: Vec<u8>, init_args: Vec<u8>, nonce: u64, gas_limit: u64, gas_price: u64) -> Self {
        Self {
            from,
            payload: TxPayload::ContractDeploy { wasm_code, init_args },
            nonce,
            gas_limit,
            gas_price,
            signature: Signature::empty(),
        }
    }

    pub fn new_call(from: Address, contract_address: Address, method: String, args: Vec<u8>, nonce: u64, gas_limit: u64, gas_price: u64) -> Self {
        Self {
            from,
            payload: TxPayload::ContractCall { contract_address, method, args },
            nonce,
            gas_limit,
            gas_price,
            signature: Signature::empty(),
        }
    }

    pub fn sign(&mut self, private_key: &SecretKey) -> Result<(), String> {
        let secp = Secp256k1::new();
        let message = self.hash();
        let message = Message::from_digest_slice(&message).map_err(|e| format!("Invalid message: {}", e))?;
        let sig = secp.sign_ecdsa_recoverable(&message, private_key);
        let (recovery_id, sig_bytes) = sig.serialize_compact();
        let mut full_sig = Vec::with_capacity(65);
        full_sig.extend_from_slice(&sig_bytes);
        full_sig.push(recovery_id.to_i32() as u8);
        self.signature = Signature {
            algorithm: CryptoAlgorithm::Secp256k1,
            data: full_sig,
        };
        Ok(())
    }

    pub fn hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(&self.from);
        match &self.payload {
            TxPayload::Transfer { to, amount } => {
                hasher.update(&[0u8]);
                hasher.update(to);
                hasher.update(&amount.to_le_bytes());
            }
            TxPayload::ContractDeploy { wasm_code, init_args } => {
                hasher.update(&[1u8]);
                hasher.update(&Sha256::digest(wasm_code));
                hasher.update(&Sha256::digest(init_args));
            }
            TxPayload::ContractCall { contract_address, method, args } => {
                hasher.update(&[2u8]);
                hasher.update(contract_address);
                hasher.update(method.as_bytes());
                hasher.update(&Sha256::digest(args));
            }
        }
        hasher.update(&self.nonce.to_le_bytes());
        hasher.update(&self.gas_limit.to_le_bytes());
        hasher.update(&self.gas_price.to_le_bytes());
        hasher.finalize().into()
    }

    pub fn verify_signature(&self) -> Result<PublicKey, String> {
        if self.signature.algorithm != CryptoAlgorithm::Secp256k1 {
            return Err("Unsupported signature algorithm".to_string());
        }
        if self.signature.data.len() != 65 {
            return Err("Invalid signature length".to_string());
        }
        let secp = Secp256k1::new();
        let message = self.hash();
        let message = Message::from_digest_slice(&message).map_err(|e| format!("Invalid message: {}", e))?;
        let recovery_id = secp256k1::ecdsa::RecoveryId::from_i32(self.signature.data[64] as i32)
            .map_err(|e| format!("Invalid recovery id: {}", e))?;
        let sig = secp256k1::ecdsa::RecoverableSignature::from_compact(&self.signature.data[0..64], recovery_id)
            .map_err(|e| format!("Invalid signature: {}", e))?;
        let pubkey = secp.recover_ecdsa(&message, &sig)
            .map_err(|e| format!("Signature recovery failed: {}", e))?;
        Ok(pubkey)
    }

    pub fn validate_basic(&self) -> Result<(), String> {
        // gas limit must be >0
        if self.gas_limit == 0 {
            return Err("Gas limit must be greater than 0".to_string());
        }
        // gas price must be >0
        if self.gas_price == 0 {
            return Err("Gas price must be greater than 0".to_string());
        }
        // note: transfer must have amount >0
        if let TxPayload::Transfer { amount, .. } = &self.payload {
            if *amount == 0 {
                return Err("Amount must be greater than 0".to_string());
            }
        }
        // signature verification same as before
        let pubkey = self.verify_signature()?;
        let pubkey_hash = Sha256::digest(&pubkey.serialize()[1..]); // Skip compression byte
        let address: [u8; 20] = pubkey_hash[12..32].try_into().unwrap();
        if address != self.from {
            return Err("Signature does not match from address".to_string());
        }
        Ok(())
    }
}

impl Default for Transaction {
    fn default() -> Self {
        Transaction::default_transfer()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secp256k1::Secp256k1;

    #[test]
    fn test_transfer_transaction_creation() {
        let from: Address = [1; 20];
        let to: Address = [2; 20];
        let tx = Transaction::new_transfer(from, to, 100, 1, 21000, 1);
        if let TxPayload::Transfer { amount, .. } = tx.payload {
            assert_eq!(amount, 100);
        } else {
            panic!("Expected transfer payload");
        }
        assert_eq!(tx.nonce, 1);
    }

    #[test]
    fn test_transaction_hash_length() {
        let from: Address = [1; 20];
        let to: Address = [2; 20];
        let tx = Transaction::new_transfer(from, to, 100, 1, 21000, 1);
        let hash = tx.hash();
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn test_transaction_sign_and_verify() {
        let secp = Secp256k1::new();
        let secret_key = SecretKey::from_slice(&[1; 32]).unwrap();
        let public_key = secret_key.public_key(&secp);
        let pubkey_hash = Sha256::digest(&public_key.serialize()[1..]);
        let from: Address = pubkey_hash[12..32].try_into().unwrap();

        let to: Address = [2; 20];
        let mut tx = Transaction::new_transfer(from, to, 100, 1, 21000, 1);
        tx.sign(&secret_key).unwrap();

        assert!(tx.validate_basic().is_ok());
    }

    #[test]
    fn test_invalid_signature() {
        let from: Address = [1; 20];
        let to: Address = [2; 20];
        let mut tx = Transaction::new_transfer(from, to, 100, 1, 21000, 1);
        tx.signature = Signature { algorithm: CryptoAlgorithm::Secp256k1, data: vec![0; 65] }; // Invalid signature
        assert!(tx.validate_basic().is_err());
    }
}