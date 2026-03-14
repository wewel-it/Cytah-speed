use std::collections::{HashMap, HashSet};
use parking_lot::{RwLock, Mutex};
use std::sync::Arc;
use crate::core::{Transaction, Address};
use crate::state::state_manager::StateManager;

/// Rate limit window in seconds (per source)
const RATE_LIMIT_WINDOW_SECS: u64 = 60;
/// Max transactions allowed from a single source per window
const MAX_TX_PER_SOURCE: usize = 100;
/// Default minimum gas price when base fee is not yet initialized
pub(crate) const DEFAULT_MIN_GAS_PRICE: u64 = 1;

/// Transaksi dalam mempool dengan metadata
#[derive(Clone, Debug)]
pub struct MempoolTransaction {
    pub transaction: Transaction,
    /// Hash dari transaksi parent (dependencies)
    pub parent_tx_hashes: Vec<[u8; 32]>,
    /// Waktu transaksi ditambahkan ke mempool
    pub timestamp: u64,
    /// Apakah transaksi sudah siap dieksekusi (semua dependency terpenuhi)
    pub is_ready: bool,
}

impl MempoolTransaction {
    pub fn new(
        transaction: Transaction,
        parent_tx_hashes: Vec<[u8; 32]>,
        timestamp: u64,
    ) -> Self {
        let is_ready = parent_tx_hashes.is_empty();
        Self {
            transaction,
            parent_tx_hashes,
            timestamp,
            is_ready,
        }
    }
}

/// DAG-based mempool untuk menyimpan dan mengatur transaksi
/// 
/// Fitur:
/// - Menyimpan transaksi dengan dependency DAG
/// - Melacak parent-child relationships
/// - Hanya mengembalikan transaksi yang siap dieksekusi
/// - Mekanisme fee market (base fee, priority fee sorting)
/// - Proteksi rate limit / spam / akun kosong
#[derive(Clone)]
pub struct TxDagMempool {
    /// Map dari tx hash ke mempool transaction
    transactions: Arc<RwLock<HashMap<[u8; 32], MempoolTransaction>>>,
    /// Map dari tx hash ke child tx hashes
    parent_child_map: Arc<RwLock<HashMap<[u8; 32], HashSet<[u8; 32]>>>>,
    /// Transaksi pending dari sender (untuk multi-tx dari satu sender)
    /// Map dari address ke vec of tx hashes
    pending_by_sender: Arc<RwLock<HashMap<Address, Vec<[u8; 32]>>>>,
    /// Ukuran max mempool (jumlah transaksi)
    max_size: usize,

    /// Reference ke StateManager untuk pengecekan saldo
    state: Arc<Mutex<StateManager>>,

    /// Current base fee used for admission/ordering
    current_base_fee: Arc<RwLock<u64>>,

    /// Rate-limit tracking (source string -> (count, window_start))
    rate_limits: Arc<RwLock<HashMap<String, (usize, u64)>>>,

    /// Minimum gas price enforced (could be base fee or higher)
    min_gas_price: u64,
}

impl TxDagMempool {
    /// Buat mempool baru
    pub fn new(max_size: usize, state: Arc<Mutex<StateManager>>, min_gas_price: u64) -> Self {
        Self {
            transactions: Arc::new(RwLock::new(HashMap::new())),
            parent_child_map: Arc::new(RwLock::new(HashMap::new())),
            pending_by_sender: Arc::new(RwLock::new(HashMap::new())),
            max_size,
            state,
            current_base_fee: Arc::new(RwLock::new(min_gas_price)),
            rate_limits: Arc::new(RwLock::new(HashMap::new())),
            min_gas_price,
        }
    }

    /// Tambahkan transaksi ke mempool
    pub fn add_transaction(
        &self,
        tx: Transaction,
        parent_tx_hashes: Vec<[u8; 32]>,
        source: Option<String>,
    ) -> Result<(), String> {
        // rate limiting per source
        if let Some(src) = source.clone() {
            let now = chrono::Utc::now().timestamp() as u64;
            let mut rates = self.rate_limits.write();
            let entry = rates.entry(src.clone()).or_insert((0, now));
            if now >= entry.1 + RATE_LIMIT_WINDOW_SECS {
                entry.0 = 0;
                entry.1 = now;
            }
            if entry.0 >= MAX_TX_PER_SOURCE {
                return Err("Rate limit exceeded for source".to_string());
            }
            entry.0 += 1;
        }

        let mut txs = self.transactions.write();

        // handle capacity: if full, drop lowest-fee tx if this one is better
        if txs.len() >= self.max_size {
            // compute smallest gas price in pool
            if let Some((low_hash, low_tx)) = txs
                .iter()
                .min_by_key(|(_, mt)| mt.transaction.gas_price)
                .map(|(h, mt)| (*h, mt.clone()))
            {
                if tx.gas_price > low_tx.transaction.gas_price {
                    txs.remove(&low_hash);
                } else {
                    return Err("Mempool is full".to_string());
                }
            } else {
                return Err("Mempool is full".to_string());
            }
        }

        // Sign harus ada
        if tx.signature.data.is_empty() {
            return Err("Transaction must be signed before adding to mempool".to_string());
        }

        // enforce minimum gas price
        let base = *self.current_base_fee.read();
        let threshold = std::cmp::max(base, self.min_gas_price);
        if tx.gas_price < threshold {
            return Err(format!("Gas price {} below minimum {}", tx.gas_price, threshold));
        }

        // Enforce sender balance > 0 (basic check)
        let balance = self.state.lock().get_balance(tx.from);
        if balance == 0 {
            return Err("Sender account has zero balance".to_string());
        }

        let tx_hash = tx.hash();
        let tx_hash_array: [u8; 32] = tx_hash.try_into()
            .map_err(|_| "Invalid transaction hash".to_string())?;

        // Jangan tambahkan duplikat
        if txs.contains_key(&tx_hash_array) {
            return Err("Transaction already in mempool".to_string());
        }

        // Validasi dependency
        for parent_hash in &parent_tx_hashes {
            if !txs.contains_key(parent_hash) {
                // Parent tidak ditemukan, bisa jadi belum ada atau sudah finalized
                // Untuk sekarang, izinkan dengan asumsi parent akan ada atau sudah confirmed
            }
        }

        // Tentukan apakah transaksi ready
        let is_ready = parent_tx_hashes.is_empty() || 
            parent_tx_hashes.iter().all(|parent_hash| !txs.contains_key(parent_hash));

        let mempool_tx = MempoolTransaction::new(
            tx.clone(),
            parent_tx_hashes.clone(),
            chrono::Utc::now().timestamp() as u64,
        );

        let mut mempool_tx_mut = mempool_tx.clone();
        mempool_tx_mut.is_ready = is_ready;

        txs.insert(tx_hash_array, mempool_tx_mut);
        drop(txs);

        // Update parent-child map
        let mut parent_child = self.parent_child_map.write();
        for parent_hash in &parent_tx_hashes {
            parent_child
                .entry(*parent_hash)
                .or_insert_with(HashSet::new)
                .insert(tx_hash_array);
        }
        drop(parent_child);

        // Update pending_by_sender
        let mut pending = self.pending_by_sender.write();
        pending
            .entry(tx.from)
            .or_insert_with(Vec::new)
            .push(tx_hash_array);

        Ok(())
    }

    /// Validasi transaksi
    /// Pengecekan basic: signature, format, dll
    pub fn validate_transaction(&self, tx: &Transaction) -> Result<(), String> {
        // Validasi signature
        tx.validate_basic()?;

        // Validasi additional rules
        if let crate::core::transaction::TxPayload::Transfer { amount, .. } = &tx.payload {
            if *amount == 0 {
                return Err("Transaction amount must be > 0".to_string());
            }
        }

        if tx.gas_limit == 0 {
            return Err("Gas limit must be > 0".to_string());
        }

        Ok(())
    }

    /// Dapatkan transaksi yang siap dieksekusi
    /// Transaksi siap jika semua dependencies sudah fulfilled atau tidak ada dependencies
    pub fn get_ready_transactions(&self) -> Vec<MempoolTransaction> {
        let txs = self.transactions.read();
        txs.values()
            .filter(|tx| tx.is_ready)
            .cloned()
            .collect()
    }

    /// Return number of transactions currently in the mempool
    pub fn tx_count(&self) -> usize {
        let txs = self.transactions.read();
        txs.len()
    }

    /// Mark transaksi sebagai sudah dieksekusi (included dalam block)
    pub fn remove_transaction(&self, tx_hash: &[u8; 32]) -> Option<MempoolTransaction> {
        let mut txs = self.transactions.write();
        
        let removed = txs.remove(tx_hash);

        if removed.is_some() {
            // Update parent-child map
            let mut parent_child = self.parent_child_map.write();
            parent_child.remove(tx_hash);
            
            // Update children's ready status
            if let Some(children) = parent_child.get(tx_hash) {
                for child_hash in children {
                    // To avoid borrowing txs mutably and immutably at same time,
                    // first clone parent_tx_hashes then check.
                    if let Some(child) = txs.get_mut(child_hash) {
                        let deps = child.parent_tx_hashes.clone();
                        // release mutable borrow before checking
                        let _ = child;
                        let all_dependencies_fulfilled = deps.iter()
                            .all(|p| !txs.contains_key(p));
                        if let Some(child2) = txs.get_mut(child_hash) {
                            child2.is_ready = all_dependencies_fulfilled;
                        }
                    }
                }
            }
            drop(parent_child);

            // Update pending_by_sender
            if let Some(removed_tx) = &removed {
                let mut pending = self.pending_by_sender.write();
                if let Some(pending_list) = pending.get_mut(&removed_tx.transaction.from) {
                    pending_list.retain(|h| h != tx_hash);
                }
            }
        }

        removed
    }

    /// Dapatkan semua transaksi di mempool
    pub fn get_all_transactions(&self) -> Vec<MempoolTransaction> {
        self.transactions.read().values().cloned().collect()
    }

    /// Get a specific transaction by hash (if present), typically used by RPC.
    pub fn get_transaction(&self, hash: &[u8; 32]) -> Option<MempoolTransaction> {
        self.transactions.read().get(hash).cloned()
    }

    /// Dapatkan jumlah transaksi di mempool
    pub fn size(&self) -> usize {
        self.transactions.read().len()
    }

    /// Update the current base fee (called by producer / node)
    pub fn set_base_fee(&self, fee: u64) {
        *self.current_base_fee.write() = fee;
    }

    /// Clear mempool
    pub fn clear(&self) {
        self.transactions.write().clear();
        self.parent_child_map.write().clear();
        self.pending_by_sender.write().clear();
    }

    /// Dapatkan transaksi pending dari sender tertentu
    pub fn get_sender_pending(&self, from: Address) -> Vec<[u8; 32]> {
        self.pending_by_sender
            .read()
            .get(&from)
            .cloned()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secp256k1::{Secp256k1, SecretKey};
    use sha2::{Sha256, Digest};

    fn create_test_transaction(to: Address, amount: u64, nonce: u64, secret_key: &SecretKey) -> Transaction {
        let secp = Secp256k1::new();
        let pubkey = secret_key.public_key(&secp);
        let pubkey_hash = Sha256::digest(&pubkey.serialize()[1..]);
        let from: Address = pubkey_hash[12..32].try_into().unwrap();

        let mut tx = Transaction::new(from, to, amount, nonce, 21000, 1);
        tx.sign(secret_key).unwrap();
        tx
    }

    #[test]
    fn test_mempool_creation() {
        let state = Arc::new(Mutex::new(StateManager::new()));
        let mempool = TxDagMempool::new(1000, state.clone(), DEFAULT_MIN_GAS_PRICE);
        assert_eq!(mempool.size(), 0);
    }

    #[test]
    fn test_add_transaction() {
        let state = Arc::new(Mutex::new(StateManager::new()));
        let secret_key = SecretKey::from_slice(&[1; 32]).unwrap();
        let secp = Secp256k1::new();
        let pubkey = secret_key.public_key(&secp);
        let pubkey_hash = Sha256::digest(&pubkey.serialize()[1..]);
        let from: Address = pubkey_hash[12..32].try_into().unwrap();

        // fund the derived sender account so balance isn't zero
        state.lock().state_tree.update_account(from, crate::state::state_tree::Account::new(1000, 0));
        let mempool = TxDagMempool::new(1000, state.clone(), DEFAULT_MIN_GAS_PRICE);
        let to = [2u8; 20];

        let tx = create_test_transaction(to, 100, 0, &secret_key);
        let result = mempool.add_transaction(tx, vec![], Some("peer1".to_string()));

        assert!(result.is_ok());
        assert_eq!(mempool.size(), 1);
    }

    #[test]
    fn test_validate_transaction() {
        let state = Arc::new(Mutex::new(StateManager::new()));
        let secret_key = SecretKey::from_slice(&[1; 32]).unwrap();
        let secp = Secp256k1::new();
        let pubkey = secret_key.public_key(&secp);
        let pubkey_hash = Sha256::digest(&pubkey.serialize()[1..]);
        let from: Address = pubkey_hash[12..32].try_into().unwrap();

        // fund the derived sender account so balance isn't zero
        state.lock().state_tree.update_account(from, crate::state::state_tree::Account::new(1000, 0));
        let mempool = TxDagMempool::new(1000, state.clone(), DEFAULT_MIN_GAS_PRICE);
        let to = [2u8; 20];

        let tx = create_test_transaction(to, 100, 0, &secret_key);
        let result = mempool.validate_transaction(&tx);

        assert!(result.is_ok());
    }

    #[test]
    fn test_get_ready_transactions() {
        let state = Arc::new(Mutex::new(StateManager::new()));
        let secret_key = SecretKey::from_slice(&[1; 32]).unwrap();
        let secp = Secp256k1::new();
        let pubkey = secret_key.public_key(&secp);
        let pubkey_hash = Sha256::digest(&pubkey.serialize()[1..]);
        let from: Address = pubkey_hash[12..32].try_into().unwrap();

        // fund the derived sender account so balance isn't zero
        state.lock().state_tree.update_account(from, crate::state::state_tree::Account::new(1000, 0));
        let mempool = TxDagMempool::new(1000, state.clone(), DEFAULT_MIN_GAS_PRICE);
        let to = [2u8; 20];

        let tx = create_test_transaction(to, 100, 0, &secret_key);
        mempool.add_transaction(tx, vec![], Some("peer2".to_string())).unwrap();

        let ready = mempool.get_ready_transactions();
        assert_eq!(ready.len(), 1);
        assert!(ready[0].is_ready);
    }

    #[test]
    fn test_remove_transaction() {
        let state = Arc::new(Mutex::new(StateManager::new()));
        let secret_key = SecretKey::from_slice(&[1; 32]).unwrap();
        let secp = Secp256k1::new();
        let pubkey = secret_key.public_key(&secp);
        let pubkey_hash = Sha256::digest(&pubkey.serialize()[1..]);
        let from: Address = pubkey_hash[12..32].try_into().unwrap();

        // fund the derived sender account so balance isn't zero
        state.lock().state_tree.update_account(from, crate::state::state_tree::Account::new(1000, 0));
        let mempool = TxDagMempool::new(1000, state.clone(), DEFAULT_MIN_GAS_PRICE);
        let to = [2u8; 20];

        let tx = create_test_transaction(to, 100, 0, &secret_key);
        mempool.add_transaction(tx.clone(), vec![], Some("src".to_string())).unwrap();
        assert_eq!(mempool.size(), 1);

        let tx_hash_vec = tx.hash();
        let tx_hash: [u8; 32] = tx_hash_vec.try_into().unwrap();
        mempool.remove_transaction(&tx_hash);

        assert_eq!(mempool.size(), 0);
    }

    // legacy duplicates removed above; additional feature tests:
    #[test]
    fn test_reject_zero_balance() {
        let state = Arc::new(Mutex::new(StateManager::new()));
        let mempool = TxDagMempool::new(10, state.clone(), DEFAULT_MIN_GAS_PRICE);
        let to = [2u8; 20];
        let secret_key = SecretKey::from_slice(&[1; 32]).unwrap();
        let tx = create_test_transaction(to, 50, 0, &secret_key);
        let res = mempool.add_transaction(tx, vec![], Some("peer3".to_string()));
        assert!(res.is_err());
    }


    #[test]
    fn test_reject_low_gas_price() {
        let state = Arc::new(Mutex::new(StateManager::new()));
        let secret_key = SecretKey::from_slice(&[1; 32]).unwrap();
        let secp = Secp256k1::new();
        let pubkey = secret_key.public_key(&secp);
        let pubkey_hash = Sha256::digest(&pubkey.serialize()[1..]);
        let from: Address = pubkey_hash[12..32].try_into().unwrap();

        // fund the derived sender account so balance isn't zero
        state.lock().state_tree.update_account(from, crate::state::state_tree::Account::new(1000, 0));
        let mempool = TxDagMempool::new(10, state.clone(), 100);
        let to = [2u8; 20];
        let mut tx = create_test_transaction(to, 10, 0, &secret_key);
        tx.gas_price = 50; // below min 100
        let res = mempool.add_transaction(tx, vec![], Some("peer4".to_string()));
        assert!(res.is_err());
    }

    #[test]
    fn test_rate_limit() {
        let state = Arc::new(Mutex::new(StateManager::new()));
        let secret_key = SecretKey::from_slice(&[1; 32]).unwrap();
        let secp = Secp256k1::new();
        let pubkey = secret_key.public_key(&secp);
        let pubkey_hash = Sha256::digest(&pubkey.serialize()[1..]);
        let from: Address = pubkey_hash[12..32].try_into().unwrap();

        // fund the derived sender account so it has non-zero balance
        state.lock().state_tree.update_account(from, crate::state::state_tree::Account::new(1000, 0));
        let mempool = TxDagMempool::new(1000, state.clone(), DEFAULT_MIN_GAS_PRICE);
        let to = [2u8; 20];
        for _ in 0..MAX_TX_PER_SOURCE {
            let tx = create_test_transaction(to, 1, 0, &secret_key);
            let _ = mempool.add_transaction(tx, vec![], Some("peer5".to_string()));
        }
        let tx = create_test_transaction(to, 1, 1, &secret_key);
        let res = mempool.add_transaction(tx, vec![], Some("peer5".to_string()));
        assert!(res.is_err());
    }

    #[test]
    fn test_drop_low_fee_when_full() {
        let state = Arc::new(Mutex::new(StateManager::new()));
        let secret_key = SecretKey::from_slice(&[1; 32]).unwrap();
        let secp = Secp256k1::new();
        let pubkey = secret_key.public_key(&secp);
        let pubkey_hash = Sha256::digest(&pubkey.serialize()[1..]);
        let from: Address = pubkey_hash[12..32].try_into().unwrap();

        // fund the derived sender account so it can add transactions
        state.lock().state_tree.update_account(from, crate::state::state_tree::Account::new(1000, 0));
        let mempool = TxDagMempool::new(1, state.clone(), DEFAULT_MIN_GAS_PRICE);
        let to = [2u8; 20];
        let mut tx1 = create_test_transaction(to, 1, 0, &secret_key);
        tx1.gas_price = 10;
        mempool.add_transaction(tx1.clone(), vec![], Some("peer6".to_string())).unwrap();
        let mut tx2 = create_test_transaction(to, 1, 1, &secret_key);
        tx2.gas_price = 20;
        // should drop tx1 and accept tx2
        mempool.add_transaction(tx2.clone(), vec![], Some("peer6".to_string())).unwrap();
        assert_eq!(mempool.size(), 1);
        let entries = mempool.get_all_transactions();
        assert_eq!(entries[0].transaction.gas_price, 20);
    }
}
