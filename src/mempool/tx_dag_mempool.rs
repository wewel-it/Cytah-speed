use std::collections::{HashMap, HashSet};
use parking_lot::RwLock;
use std::sync::Arc;
use crate::core::{Transaction, Address};

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
#[derive(Clone)]
pub struct TxDagMempool {
    /// Map dari tx hash ke mempool transaction
    transactions: Arc<RwLock<HashMap<[u8; 32], MempoolTransaction>>>,
    /// Map dari tx hash ke child tx hashes
    parent_child_map: Arc<RwLock<HashMap<[u8; 32], HashSet<[u8; 32]>>>>,
    /// Transaksi pending dari sender (untuk multi-tx dari satu sender)
    /// Map dari address ke vec of tx hashes
    pending_by_sender: Arc<RwLock<HashMap<Address, Vec<[u8; 32]>>>>,
    /// Ukuran max mempool
    max_size: usize,
}

impl TxDagMempool {
    /// Buat mempool baru
    pub fn new(max_size: usize) -> Self {
        Self {
            transactions: Arc::new(RwLock::new(HashMap::new())),
            parent_child_map: Arc::new(RwLock::new(HashMap::new())),
            pending_by_sender: Arc::new(RwLock::new(HashMap::new())),
            max_size,
        }
    }

    /// Tambahkan transaksi ke mempool
    pub fn add_transaction(
        &self,
        tx: Transaction,
        parent_tx_hashes: Vec<[u8; 32]>,
    ) -> Result<(), String> {
        let mut txs = self.transactions.write();

        // Validasi basic
        if txs.len() >= self.max_size {
            return Err("Mempool is full".to_string());
        }

        // Sign jika belum
        if tx.signature.is_empty() {
            return Err("Transaction must be signed before adding to mempool".to_string());
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
        if tx.amount == 0 {
            return Err("Transaction amount must be > 0".to_string());
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

    /// Dapatkan jumlah transaksi di mempool
    pub fn size(&self) -> usize {
        self.transactions.read().len()
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
    use secp256k1::SecretKey;

    fn create_test_transaction(from: Address, to: Address, amount: u64, nonce: u64) -> Transaction {
        let mut tx = Transaction::new(from, to, amount, nonce, 21000);
        
        // Sign transaction
        let secret_key = SecretKey::from_slice(&[1; 32]).unwrap();
        tx.sign(&secret_key).ok();
        
        tx
    }

    #[test]
    fn test_mempool_creation() {
        let mempool = TxDagMempool::new(1000);
        assert_eq!(mempool.size(), 0);
    }

    #[test]
    fn test_add_transaction() {
        let mempool = TxDagMempool::new(1000);
        let from = [1u8; 20];
        let to = [2u8; 20];
        
        let tx = create_test_transaction(from, to, 100, 0);
        let result = mempool.add_transaction(tx, vec![]);
        
        assert!(result.is_ok());
        assert_eq!(mempool.size(), 1);
    }

    #[test]
    fn test_validate_transaction() {
        let mempool = TxDagMempool::new(1000);
        let from = [1u8; 20];
        let to = [2u8; 20];
        
        let tx = create_test_transaction(from, to, 100, 0);
        let result = mempool.validate_transaction(&tx);
        
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_ready_transactions() {
        let mempool = TxDagMempool::new(1000);
        let from = [1u8; 20];
        let to = [2u8; 20];
        
        let tx = create_test_transaction(from, to, 100, 0);
        mempool.add_transaction(tx, vec![]).unwrap();
        
        let ready = mempool.get_ready_transactions();
        assert_eq!(ready.len(), 1);
        assert!(ready[0].is_ready);
    }

    #[test]
    fn test_remove_transaction() {
        let mempool = TxDagMempool::new(1000);
        let from = [1u8; 20];
        let to = [2u8; 20];
        
        let tx = create_test_transaction(from, to, 100, 0);
        mempool.add_transaction(tx.clone(), vec![]).unwrap();
        assert_eq!(mempool.size(), 1);
        
        let tx_hash_vec = tx.hash();
        let tx_hash: [u8; 32] = tx_hash_vec.try_into().unwrap();
        mempool.remove_transaction(&tx_hash);
        
        assert_eq!(mempool.size(), 0);
    }

    #[test]
    fn test_mempool_overflow() {
        let mempool = TxDagMempool::new(2);
        let from = [1u8; 20];
        let to = [2u8; 20];
        
        let tx1 = create_test_transaction(from, to, 100, 0);
        let tx2 = create_test_transaction(from, to, 100, 1);
        let tx3 = create_test_transaction(from, to, 100, 2);
        
        mempool.add_transaction(tx1, vec![]).unwrap();
        mempool.add_transaction(tx2, vec![]).unwrap();
        
        let result = mempool.add_transaction(tx3, vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_transaction_dependencies() {
        let mempool = TxDagMempool::new(1000);
        let from = [1u8; 20];
        let to = [2u8; 20];
        
        let tx1 = create_test_transaction(from, to, 100, 0);
        let tx1_hash_vec = tx1.hash();
        let tx1_hash: [u8; 32] = tx1_hash_vec.try_into().unwrap();
        
        let tx2 = create_test_transaction(from, to, 50, 1);
        
        // Add tx1 without dependencies
        mempool.add_transaction(tx1, vec![]).unwrap();
        assert_eq!(mempool.get_ready_transactions().len(), 1);
        
        // Add tx2 with tx1 as dependency
        mempool.add_transaction(tx2, vec![tx1_hash]).unwrap();
        
        // tx2 should not be ready (tx1 still in mempool)
        let ready = mempool.get_ready_transactions();
        assert_eq!(ready.len(), 1); // Only tx1 is ready
    }
}
