use serde::{Deserialize, Serialize};
use crate::crypto::Keypair;
use crate::errors::SdkError;
use cytah_core::core::transaction::{Transaction, Address};

/// Represents a wallet with a keypair and derived address.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Wallet {
    /// Secret key bytes (32 bytes)
    pub secret_key: Vec<u8>,
    /// Public key bytes (compressed, 33 bytes)
    pub public_key: Vec<u8>,
    /// Derived address used on Cytah-Speed
    pub address: Address,
}

impl Wallet {
    /// Create a new random wallet.
    pub fn create_wallet() -> Result<Self, SdkError> {
        let kp = Keypair::generate()?;
        let address = kp.derive_address();

        Ok(Self {
            secret_key: kp.secret_bytes().to_vec(),
            public_key: kp.public_bytes().to_vec(),
            address,
        })
    }

    /// Import a wallet from an existing private key (hex or raw bytes).
    pub fn import_private_key(secret_bytes: &[u8]) -> Result<Self, SdkError> {
        let kp = Keypair::from_secret_bytes(secret_bytes)?;
        let address = kp.derive_address();

        Ok(Self {
            secret_key: kp.secret_bytes().to_vec(),
            public_key: kp.public_bytes().to_vec(),
            address,
        })
    }

    /// Export the private key as hex string
    pub fn export_private_key_hex(&self) -> String {
        hex::encode(&self.secret_key)
    }

    /// Sign a transaction (mutates and sets signature) using this wallet.
    pub fn sign_transaction(&self, tx: &mut Transaction) -> Result<(), SdkError> {
        let kp = Keypair::from_secret_bytes(&self.secret_key)?;
        tx.sign(&kp.secret).map_err(|e| SdkError::TransactionError(e))
    }

    /// Derive address from keypair (same as stored address)
    pub fn derive_address(&self) -> Address {
        self.address
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::transaction::Transaction;

    #[test]
    fn test_wallet_create_and_sign() {
        let wallet = Wallet::create_wallet().expect("create wallet");
        let addr = wallet.derive_address();
        assert_eq!(addr, wallet.address);

        let to = [2u8; 20];
        let mut tx = Transaction::new_transfer(addr, to, 100, 1, 21000, 1);
        wallet.sign_transaction(&mut tx).expect("sign tx");
        assert!(tx.validate_basic().is_ok());
    }

    #[test]
    fn test_import_export_private_key() {
        let wallet = Wallet::create_wallet().expect("create wallet");
        let hex = wallet.export_private_key_hex();
        let secret = hex::decode(&hex).expect("decode hex");
        let imported = Wallet::import_private_key(&secret).expect("import");
        assert_eq!(imported.address, wallet.address);
    }
}
