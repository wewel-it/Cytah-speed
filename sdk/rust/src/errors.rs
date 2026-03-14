use thiserror::Error;

/// Unified SDK error type for Cytah-Speed SDK.
#[derive(Error, Debug)]
pub enum SdkError {
    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("RPC error: {0}")]
    RpcError(String),

    #[error("Crypto error: {0}")]
    CryptoError(String),

    #[error("Transaction error: {0}")]
    TransactionError(String),

    #[error("Contract error: {0}")]
    ContractError(String),

    #[error("Config error: {0}")]
    ConfigError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Unknown error")]
    Unknown,
}
