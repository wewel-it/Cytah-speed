use serde::{Serialize, Deserialize};

/// Cryptographic algorithms supported by the system.  Existing history
/// assumes Secp256k1 signatures; newer algorithms may be added later
/// (Schnorr, post-quantum schemes, etc.).  The `Signature` struct tags
/// which algorithm was used so that old data remains interpretable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CryptoAlgorithm {
    Secp256k1,
    Schnorr,
    PostQuantum, // placeholder
}

/// A signature together with the algorithm that produced it.  This
/// indirection enables protocol upgrades without breaking old entries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Signature {
    pub algorithm: CryptoAlgorithm,
    pub data: Vec<u8>,
}

impl Signature {
    pub fn empty() -> Self {
        Self {
            algorithm: CryptoAlgorithm::Secp256k1,
            data: Vec::new(),
        }
    }
}
