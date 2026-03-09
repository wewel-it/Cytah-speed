use serde::{Deserialize, Serialize};
use crate::core::{Block, Transaction, BlockHash};

/// Message types for P2P network communication
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NetworkMessage {
    /// Broadcast a new block
    NewBlock(Block),
    /// Broadcast a new transaction
    NewTransaction(Transaction),
    /// Request a specific block by hash
    RequestBlock(BlockHash),
    /// Request the entire DAG
    RequestDag,
    /// Response with DAG blocks
    DagResponse(Vec<Block>),
}