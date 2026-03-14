//! Cytah-Speed SDK
//!
//! This module provides a lightweight developer SDK for building wallets, dApps,
//! bots, and integrations with Cytah-Speed blockchain.
//!
//! The SDK is intentionally kept minimal and modular; users can pick and choose
//! the components they need (RPC client, wallet utilities, transaction builders,
//! contract helpers, etc.).

pub mod client;
pub mod wallet;
pub mod transaction;
pub mod contract;
pub mod network;
pub mod crypto;
pub mod mobile;
pub mod errors;

pub use client::Client;
pub use errors::SdkError;
pub use mobile::{MobileClient, MobileWallet};
