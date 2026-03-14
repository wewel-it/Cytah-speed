use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use secp256k1::{Message, PublicKey, Secp256k1, SecretKey};

use crate::errors::SdkError;

/// Keypair used by wallet
pub struct Keypair {
    pub secret: SecretKey,
    pub public: PublicKey,
}

impl Keypair {
    /// Generate a new random keypair using secure randomness
    pub fn generate() -> Result<Self, SdkError> {
        let secp = Secp256k1::new();
        let mut rng = OsRng;
        let secret = SecretKey::new(&mut rng);
        let public = PublicKey::from_secret_key(&secp, &secret);
        Ok(Self { secret, public })
    }

    /// Import from a 32-byte secret key
    pub fn from_secret_bytes(bytes: &[u8]) -> Result<Self, SdkError> {
        if bytes.len() != 32 {
            return Err(SdkError::CryptoError("Secret key must be 32 bytes".to_string()));
        }
        let secp = Secp256k1::new();
        let secret = SecretKey::from_slice(bytes).map_err(|e| SdkError::CryptoError(e.to_string()))?;
        let public = PublicKey::from_secret_key(&secp, &secret);
        Ok(Self { secret, public })
    }

    /// Export secret key as 32 bytes
    pub fn secret_bytes(&self) -> [u8; 32] {
        self.secret.secret_bytes()
    }

    /// Export public key in compressed format (33 bytes)
    pub fn public_bytes(&self) -> [u8; 33] {
        self.public.serialize()
    }

    /// Derive the wallet address used by Cytah-Speed from the public key.
    /// This matches the address derivation used in Transaction::validate_basic.
    pub fn derive_address(&self) -> [u8; 20] {
        // The system takes SHA256 over the uncompressed public key (skipping the 0x04 prefix)
        let serialized = self.public.serialize();
        let hash = Sha256::digest(&serialized[1..]);
        let mut addr = [0u8; 20];
        addr.copy_from_slice(&hash[12..32]);
        addr
    }

    /// Sign arbitrary data using secp256k1 ECDSA recoverable signature.
    pub fn sign(&self, data: &[u8]) -> Result<Vec<u8>, SdkError> {
        let secp = Secp256k1::new();
        let digest = Sha256::digest(data);
        let msg = Message::from_digest_slice(&digest).map_err(|e| SdkError::CryptoError(e.to_string()))?;
        let sig = secp.sign_ecdsa_recoverable(&msg, &self.secret);
        let (rec_id, sig_bytes) = sig.serialize_compact();
        let mut output = Vec::with_capacity(65);
        output.extend_from_slice(&sig_bytes);
        output.push(rec_id.to_i32() as u8);
        Ok(output)
    }

    /// Verify signature against message digest and expected public key
    pub fn verify(&self, data: &[u8], signature: &[u8]) -> Result<bool, SdkError> {
        if signature.len() != 65 {
            return Err(SdkError::CryptoError("Signature must be 65 bytes".to_string()));
        }
        let secp = Secp256k1::new();
        let digest = Sha256::digest(data);
        let msg = Message::from_digest_slice(&digest).map_err(|e| SdkError::CryptoError(e.to_string()))?;
        let rec_id = secp256k1::ecdsa::RecoveryId::from_i32(signature[64] as i32)
            .map_err(|e| SdkError::CryptoError(e.to_string()))?;
        let sig = secp256k1::ecdsa::RecoverableSignature::from_compact(&signature[0..64], rec_id)
            .map_err(|e| SdkError::CryptoError(e.to_string()))?;
        let recovered = secp.recover_ecdsa(&msg, &sig)
            .map_err(|e| SdkError::CryptoError(e.to_string()))?;
        Ok(recovered == self.public)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_keypair_and_address() {
        let kp = Keypair::generate().expect("should generate keypair");
        let addr = kp.derive_address();
        assert_eq!(addr.len(), 20);
    }

    #[test]
    fn test_sign_and_verify() {
        let kp = Keypair::generate().expect("should generate keypair");
        let msg = b"test message";
        let sig = kp.sign(msg).expect("should sign");
        assert!(kp.verify(msg, &sig).expect("verify should not error"));
    }
}
