use serde::{Deserialize, Serialize};
use crate::core::block::BlockHeader;
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
    /// Response with full DAG blocks
    DagResponse(Vec<Block>),

    /// Request block headers starting after the given hash (inclusive or
    /// exclusive as agreed by protocol).  The `max` field limits how many
    /// headers the peer should return.
    GetHeaders { from: BlockHash, max: usize },
    /// Reply to `GetHeaders` with a sequence of (block hash, header) pairs.
    /// Including the block hash enables the receiver to request the full blocks.
    Headers(Vec<(BlockHash, BlockHeader)>),

    /// Request full blocks by hash list.
    GetBlocks(Vec<BlockHash>),
    /// Response containing full blocks matching a previous `GetBlocks`.
    Blocks(Vec<Block>),

    /// Ask a peer for its latest state snapshot (used during fast sync).
    RequestState,
    /// Provide a serialized state snapshot.  Receiver should verify the root
    /// matches the agreed-upon value from the chain tip.
    StateSnapshot(Vec<u8>),
}