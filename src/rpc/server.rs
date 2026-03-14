use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use parking_lot::{RwLock, Mutex};
use crate::core::{Block, Transaction};
use crate::dag::blockdag::BlockDAG;
use crate::mempool::tx_dag_mempool::TxDagMempool;
use crate::state::state_manager::StateManager; // Address not needed
use crate::network::p2p_node::P2PNode;


/// RPC server state - ensure all fields implement Send + Sync
#[derive(Clone)]
pub struct RpcState {
    pub dag: Arc<RwLock<BlockDAG>>,
    pub mempool: Arc<Mutex<Arc<TxDagMempool>>>,
    pub state_manager: Arc<Mutex<StateManager>>,
    // P2P node is optional; we wrap it in a tokio RwLock since Swarm inside
    // P2PNode isn't Send and we need an async-safe lock for cross-thread
    // access.
    pub p2p_node: Option<Arc<tokio::sync::RwLock<P2PNode>>>,
}

/// Transaction submission request
#[derive(Deserialize)]
pub struct SendTxRequest {
    pub transaction: Transaction,
}

/// Balance response
#[derive(Serialize)]
pub struct BalanceResponse {
    pub address: String,
    pub balance: u64,
    pub nonce: u64,
}

/// Block response
#[derive(Serialize)]
pub struct BlockResponse {
    pub hash: String,
    pub block: Option<Block>,
}

/// Transaction response
#[derive(Serialize)]
pub struct TransactionResponse {
    pub hash: String,
    pub transaction: Option<Transaction>,
}

/// DAG info response
#[derive(Serialize)]
pub struct DagResponse {
    pub tips: Vec<String>,
    pub total_blocks: usize,
    pub stats: serde_json::Value,
}

/// Node info response
#[derive(Serialize)]
pub struct NodeInfoResponse {
    pub peer_id: String,
    pub connected_peers: Vec<String>,
    pub mempool_size: usize,
    pub dag_height: usize,
}

/// Create the RPC server router
pub fn create_router(state: RpcState) -> Router {
    Router::new()
        .route("/send_tx", post(send_transaction))
        .route("/balance/:address", get(get_balance))
        .route("/block/:hash", get(get_block))
        .route("/tx/:hash", get(get_transaction))
        .route("/dag", get(get_dag_info))
        .route("/node/info", get(get_node_info))
        .route("/contract/deploy", post(deploy_contract))
        .route("/contract/call", post(call_contract))
        .with_state(state)
}

/// POST /send_tx - Submit transaction to mempool
#[axum::debug_handler]
pub async fn send_transaction(
    State(state): State<RpcState>,
    Json(request): Json<SendTxRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Validate transaction
    if let Err(_e) = request.transaction.validate_basic() {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Add to mempool
    let mempool = state.mempool.lock().clone();
    if let Err(_e) = mempool.add_transaction(request.transaction, vec![], None) {
        tracing::error!("Failed to add transaction to mempool");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    Ok(Json(serde_json::json!({
        "status": "success",
        "message": "Transaction submitted to mempool"
    })))
}

/// GET /balance/{address} - Get account balance
pub async fn get_balance(
    State(state): State<RpcState>,
    Path(address_str): Path<String>,
) -> Result<Json<BalanceResponse>, StatusCode> {
    // Parse cyt address
    if !address_str.starts_with("cyt") {
        return Err(StatusCode::BAD_REQUEST);
    }
    let addr_bytes = match hex::decode(&address_str[3..]) {
        Ok(b) if b.len() == 20 => {
            let mut arr = [0u8; 20];
            arr.copy_from_slice(&b);
            arr
        }
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    // Get account from state
    let state_manager = state.state_manager.lock().clone();
    let account = match state_manager.get_account(&addr_bytes) {
        Some(acc) => acc,
        None => return Err(StatusCode::NOT_FOUND),
    };
    let balance = account.balance;
    let nonce = account.nonce;

    Ok(Json(BalanceResponse {
        address: address_str.clone(),
        balance,
        nonce,
    }))
}

/// GET /block/{hash} - Get block by hash
pub async fn get_block(
    State(state): State<RpcState>,
    Path(hash_str): Path<String>,
) -> Result<Json<BlockResponse>, StatusCode> {
    // Parse hash
    let hash_bytes_result = hex::decode(&hash_str);
    let hash_bytes = match hash_bytes_result {
        Ok(b) if b.len() == 32 => b,
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&hash_bytes);

    // Get block from DAG
    let dag = state.dag.read();
    let block = dag.get_block(&hash);

    Ok(Json(BlockResponse {
        hash: hash_str,
        block,
    }))
}

/// GET /tx/{hash} - Get transaction by hash
pub async fn get_transaction(
    State(state): State<RpcState>,
    Path(hash_str): Path<String>,
) -> Result<Json<TransactionResponse>, StatusCode> {
    // Parse tx hash
    let hash_bytes = match hex::decode(&hash_str) {
        Ok(b) if b.len() == 32 => b,
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&hash_bytes);

    // First search in mempool
    let mempool = state.mempool.lock().clone();
    if let Some(mempool_tx) = mempool.get_transaction(&hash) {
        return Ok(Json(TransactionResponse {
            hash: hash_str,
            transaction: Some(mempool_tx.transaction.clone()),
        }));
    }

    // Search in blocks
    let dag = state.dag.read();
    for block in dag.get_all_blocks() {
        for tx in &block.transactions {
            if tx.hash() == hash {
                return Ok(Json(TransactionResponse {
                    hash: hash_str,
                    transaction: Some(tx.clone()),
                }));
            }
        }
    }

    Ok(Json(TransactionResponse {
        hash: hash_str,
        transaction: None,
    }))
}

/// GET /dag - Get DAG information
pub async fn get_dag_info(State(state): State<RpcState>) -> Json<DagResponse> {
    let dag = state.dag.read();
    let tips: Vec<String> = dag.get_tips().into_iter().map(|h| hex::encode(h)).collect();
    let total_blocks = dag.get_all_blocks().len();
    let stats = serde_json::json!({
        "tips_count": tips.len(),
        "total_blocks": total_blocks
    });

    Json(DagResponse {
        tips,
        total_blocks,
        stats,
    })
}

/// helper to parse a cyt-style address (cyt + 40 hex chars)
fn parse_address(addr_str: &str) -> Result<[u8; 20], StatusCode> {
    if !addr_str.starts_with("cyt") {
        return Err(StatusCode::BAD_REQUEST);
    }
    let bytes = hex::decode(&addr_str[3..]).map_err(|_| StatusCode::BAD_REQUEST)?;
    if bytes.len() != 20 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut arr = [0u8; 20];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

/// GET /node/info - Get node information
pub async fn get_node_info(State(state): State<RpcState>) -> Result<Json<NodeInfoResponse>, StatusCode> {
    // peer id and peers come from optional P2P node
    let (peer_id, connected_peers) = if let Some(node_arc) = &state.p2p_node {
        let node = node_arc.read().await;
        // PeerManager does not expose local id, so leave as unknown for now
        let peers: Vec<String> = node
            .get_connected_peers()
            .into_iter()
            .map(|p| p.to_string())
            .collect();
        ("unknown".to_string(), peers)
    } else {
        ("unknown".to_string(), Vec::new())
    };

    // Get mempool size
    let mempool = state.mempool.lock().clone();
    let mempool_size = mempool.tx_count();

    // Get DAG height (just number of blocks in this simple implementation)
    let dag = state.dag.read();
    let dag_height = dag.block_count();

    Ok(Json(NodeInfoResponse {
        peer_id,
        connected_peers,
        mempool_size,
        dag_height,
    }))
}

#[derive(Deserialize)]
pub struct DeployContractRequest {
    pub from: String,
    pub nonce: u64,
    pub wasm_code: String, // hex-encoded
    pub init_args: Option<String>,
}

#[derive(Serialize)]
pub struct DeployContractResponse {
    pub contract_address: String,
    pub tx_hash: String,
}

#[derive(Deserialize)]
pub struct CallContractRequest {
    pub from: String,
    pub nonce: u64,
    pub contract_address: String,
    pub method: String,
    pub args: Option<String>, // hex-encoded
}

#[derive(Serialize)]
pub struct CallContractResponse {
    pub status: String,
    pub result: Option<String>,
}

/// POST /contract/deploy - Deploy a contract
#[axum::debug_handler]
pub async fn deploy_contract(
    State(state): State<RpcState>,
    Json(request): Json<DeployContractRequest>,
) -> Result<Json<DeployContractResponse>, StatusCode> {
    // Decode wasm code
    let wasm_code = hex::decode(&request.wasm_code).map_err(|_| StatusCode::BAD_REQUEST)?;
    let init_args = if let Some(args_hex) = request.init_args {
        hex::decode(&args_hex).unwrap_or_default()
    } else {
        Vec::new()
    };

    // Parse sender address provided by client
    let from = parse_address(&request.from)?;
    let tx = crate::core::transaction::Transaction::new_deploy(
        from,
        wasm_code,
        init_args,
        request.nonce,
        1_000_000,
        1, // gas price
    );

    // Submit to mempool
    let mempool = state.mempool.lock().clone();
    if let Err(_e) = mempool.add_transaction(tx.clone(), vec![], None) {
        tracing::error!("Failed to add contract deploy transaction to mempool");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    // Derive contract address from tx hash
    let tx_hash = {
        use sha2::{Digest, Sha256};
        Sha256::digest(&serde_json::to_vec(&tx).unwrap_or_default())
    };

    Ok(Json(DeployContractResponse {
        contract_address: format!("cyt{}", hex::encode(&tx_hash[0..20])),
        tx_hash: hex::encode(&tx_hash),
    }))
}

/// POST /contract/call - Call a contract
#[axum::debug_handler]
pub async fn call_contract(
    State(state): State<RpcState>,
    Json(request): Json<CallContractRequest>,
) -> Result<Json<CallContractResponse>, StatusCode> {
    // Parse contract address
    if !request.contract_address.starts_with("cyt") {
        return Err(StatusCode::BAD_REQUEST);
    }
    let addr_bytes = match hex::decode(&request.contract_address[3..]) {
        Ok(b) if b.len() == 20 => {
            let mut arr = [0u8; 20];
            arr.copy_from_slice(&b);
            arr
        }
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    // Decode arguments
    let args = if let Some(args_hex) = request.args {
        hex::decode(&args_hex).unwrap_or_default()
    } else {
        Vec::new()
    };

    // Parse sender address from request
    let from = parse_address(&request.from)?;
    let tx = crate::core::transaction::Transaction::new_call(
        from,
        addr_bytes,
        request.method.clone(),
        args,
        request.nonce,
        1_000_000,
        1, // gas price
    );

    // Submit to mempool
    let mempool = state.mempool.lock().clone();
    if let Err(_e) = mempool.add_transaction(tx, vec![], None) {
        tracing::error!("Failed to add contract call transaction to mempool");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    Ok(Json(CallContractResponse {
        status: "success".to_string(),
        result: None,
    }))
}

/// Start the RPC server
pub async fn start_server(
    addr: &str,
    dag: Arc<RwLock<BlockDAG>>,
    mempool: Arc<TxDagMempool>,
    state_manager: Arc<Mutex<StateManager>>,
    p2p_node: Option<Arc<tokio::sync::RwLock<P2PNode>>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let state = RpcState {
        dag,
        mempool: Arc::new(Mutex::new(mempool)),
        state_manager,
        p2p_node,
    };

    let app = create_router(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("RPC server listening on {}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}
