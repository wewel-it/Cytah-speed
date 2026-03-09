use secp256k1::{Secp256k1, SecretKey, PublicKey, Message};

// ensure serde serialization is available for secret and public keys
use serde::{Deserialize, Serialize};
use rand::{thread_rng, Rng};
use sha2::{Sha256, Digest};
use std::fs;

/// Wallet structure for managing keys and signing transactions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallet {
    /// Private key for signing
    pub private_key: SecretKey,
    /// Public key derived from private key
    pub public_key: PublicKey,
    /// Address derived from public key
    pub address: String,
}

impl Wallet {
    /// Generate a new wallet with random keys
    pub fn new() -> Self {
        let mut rng = thread_rng();
        let mut secret_bytes = [0u8; 32];
        rng.fill(&mut secret_bytes);
        let secret_key = SecretKey::from_slice(&secret_bytes).expect("Invalid secret key");
        let secp = Secp256k1::new();
        let public_key = PublicKey::from_secret_key(&secp, &secret_key);

        let address = Self::public_key_to_address(&public_key);

        Self {
            private_key: secret_key,
            public_key,
            address,
        }
    }

    /// Create wallet from existing private key
    pub fn from_private_key(private_key: SecretKey) -> Self {
        let secp = Secp256k1::new();
        let public_key = PublicKey::from_secret_key(&secp, &private_key);
        let address = Self::public_key_to_address(&public_key);

        Self {
            private_key,
            public_key,
            address,
        }
    }

    /// Convert public key to address format: cyt + hash(public_key)
    fn public_key_to_address(public_key: &PublicKey) -> String {
        let pub_key_bytes = public_key.serialize();
        let mut hasher = Sha256::new();
        hasher.update(&pub_key_bytes);
        let hash = hasher.finalize();
        let hash_hex = hex::encode(&hash[..20]); // Take first 20 bytes
        format!("cyt{}", hash_hex)
    }

    /// Sign a transaction message
    pub fn sign_message(&self, message: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let secp = Secp256k1::new();
        let message = Message::from_digest_slice(message)?;
        let sig = secp.sign_ecdsa_recoverable(&message, &self.private_key);
        let (recovery_id, sig_bytes) = sig.serialize_compact();
        let mut signature = sig_bytes.to_vec();
        signature.push(recovery_id.to_i32() as u8);
        Ok(signature)
    }

    /// Verify a signature
    pub fn verify_signature(&self, message: &[u8], signature: &[u8]) -> Result<bool, Box<dyn std::error::Error>> {
        let secp = Secp256k1::new();
        let message = Message::from_digest_slice(message)?;
        let recovery_id = secp256k1::ecdsa::RecoveryId::from_i32(signature[64] as i32)?;
        let sig = secp256k1::ecdsa::RecoverableSignature::from_compact(&signature[0..64], recovery_id)?;
        let recovered_pubkey = secp.recover_ecdsa(&message, &sig)?;
        Ok(recovered_pubkey == self.public_key)
    }

    /// Save wallet to file (serde serializes keys automatically)
    pub fn save_to_file(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Load wallet from file
    pub fn load_from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let json = fs::read_to_string(path)?;
        let wallet: Wallet = serde_json::from_str(&json)?;
        Ok(wallet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wallet_generation() {
        let wallet = Wallet::new();
        assert!(wallet.address.starts_with("cyt"));
        assert_eq!(wallet.address.len(), 43); // cyt + 40 hex chars
    }

    #[test]
    fn test_sign_and_verify() {
        let wallet = Wallet::new();
        let message = b"test message";
        let signature = wallet.sign_message(message).unwrap();
        assert!(wallet.verify_signature(message, &signature).unwrap());
    }

    #[test]
    fn test_save_and_load() {
        let wallet = Wallet::new();
        let path = "/tmp/test_wallet.json";
        wallet.save_to_file(path).unwrap();
        let loaded = Wallet::load_from_file(path).unwrap();
        assert_eq!(wallet.address, loaded.address);
        fs::remove_file(path).unwrap();
    }
}
